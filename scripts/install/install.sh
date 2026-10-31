#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Installs Keenu on Linux or macOS from an official release, with no development tools needed.
#
#   curl -fsSL https://YOUR-KEENU-DOMAIN/install.sh | sh
#
# To read it first, which is a good idea with any script from the internet:
#
#   curl -fsSLO https://YOUR-KEENU-DOMAIN/install.sh
#   less install.sh
#   sh install.sh
#
# Environment: KEENU_VERSION picks a version, KEENU_CHANNEL=prerelease allows test versions, KEENU_YES=1 asks
# nothing, KEENU_MANIFEST_URL points at another manifest, KEENU_FORCE=1 installs on an untested GNOME Shell.
# `sh install.sh --uninstall` takes Keenu off again; your data stays.
set -eu

# Set by the release workflow when it publishes this script.
DEFAULT_MANIFEST="https://YOUR-KEENU-DOMAIN/keenu-release.json"
# The release signing key. Empty until a release sets it; the manifest's signature is then not checked.
PUBLIC_KEY=""

say() { printf '\033[1m==> %s\033[0m\n' "$*"; }
note() { printf '%s\n' "$*"; }
fail() { printf 'Keenu: %s\n' "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

secure_url() {
    case "$1" in
        https://*|http://127.0.0.1:*|http://127.0.0.1/*|http://localhost:*|http://localhost/*) return 0 ;;
        *) return 1 ;;
    esac
}

[ "$(id -u)" -ne 0 ] || fail "run this as yourself, without sudo: Keenu installs to your own home folder."

OS="$(uname -s)"
case "$(uname -m)" in
    x86_64|amd64) ARCH=x64 ;;
    aarch64|arm64) ARCH=arm64 ;;
    *) fail "this processor ($(uname -m)) has no Keenu build." ;;
esac
case "$OS" in
    Linux) PLATFORM="linux-$ARCH"; KINDS="tar.gz" ;;
    Darwin) PLATFORM="macos-$ARCH"; KINDS="dmg" ;;
    *) fail "this system ($OS) has no Keenu build here; on Windows, use install.ps1." ;;
esac

if [ "${1:-}" = "--uninstall" ]; then
    case "$OS" in
        Linux)
            UNINSTALL="$HOME/.local/bin/keenu-uninstall"
            [ -x "$UNINSTALL" ] || fail "no install from this script was found ($UNINSTALL)."
            exec "$UNINSTALL"
            ;;
        Darwin)
            say "Removing Keenu (your data stays in ~/Library/Application Support/Keenu)"
            pkill -x keenu-macos 2>/dev/null || true
            rm -rf "$HOME/Applications/Keenu.app"
            say "Keenu is removed."
            exit 0
            ;;
    esac
fi

for tool in curl tar; do have "$tool" || fail "please install $tool first."; done
if have sha256sum; then
    sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif have shasum; then
    sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
    fail "please install sha256sum: without it the download cannot be checked."
fi
have python3 || fail "please install python3: the release list is JSON and this script reads it with Python."

if [ "$OS" = Linux ]; then
    have gnome-shell || fail "GNOME Shell was not found; on Linux, Keenu is a GNOME Shell extension. Other desktops are not supported yet."
    SHELL_VERSION="$(gnome-shell --version 2>/dev/null | grep -oE '[0-9]+' | head -n1 || true)"
    if [ -z "$SHELL_VERSION" ] || [ "$SHELL_VERSION" -lt 46 ] || [ "$SHELL_VERSION" -gt 51 ]; then
        [ "${KEENU_FORCE:-}" = 1 ] || fail "Keenu is made for GNOME Shell 46 to 51, and this is ${SHELL_VERSION:-unknown}. Nothing was changed. (KEENU_FORCE=1 installs anyway.)"
    fi
fi

MANIFEST="${KEENU_MANIFEST_URL:-$DEFAULT_MANIFEST}"
case "$MANIFEST" in
    *YOUR-KEENU-DOMAIN*) fail "this copy of the installer is not set up with a release address yet. Set KEENU_MANIFEST_URL, or install from a release page." ;;
esac
secure_url "$MANIFEST" || fail "$MANIFEST is not a secure address."

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT INT TERM

say "Looking up the release for $PLATFORM"
curl -fsSL "$MANIFEST" -o "$WORK/manifest.json" || fail "could not fetch the release list from $MANIFEST."

if [ -n "$PUBLIC_KEY" ]; then
    curl -fsSL "$MANIFEST.sig" -o "$WORK/manifest.sig" || fail "the release list has no signature."
    if have openssl && openssl version 2>/dev/null | grep -qE '^OpenSSL 3'; then
        # A DER SubjectPublicKeyInfo: the fixed Ed25519 prefix, then the 32 bytes of the key.
        { printf '\060\052\060\005\006\003\053\145\160\003\041\000'; printf '%s' "$PUBLIC_KEY" | openssl base64 -d -A; } > "$WORK/key.der"
        openssl base64 -d -A -in "$WORK/manifest.sig" -out "$WORK/manifest.sig.bin"
        openssl pkeyutl -verify -pubin -inkey "$WORK/key.der" -keyform DER -rawin \
            -in "$WORK/manifest.json" -sigfile "$WORK/manifest.sig.bin" >/dev/null 2>&1 ||
            fail "the release list failed its signature check, so nothing was installed."
        say "Release list signature verified"
    else
        note "Note: OpenSSL 3 was not found, so the release list's signature could not be checked. Its address and each download's checksum still are."
    fi
fi

# version, kind, file name, URL, SHA-256 and signed, one per line, for the best file of the newest release.
PICK="$(KINDS="$KINDS" PLATFORM="$PLATFORM" WANTED="${KEENU_VERSION:-}" CHANNEL="${KEENU_CHANNEL:-stable}" python3 -I - "$WORK/manifest.json" <<'EOF'
import json, os, sys
manifest = json.load(open(sys.argv[1], encoding="utf-8"))
if manifest.get("product") != "Keenu":
    sys.exit("that address does not hold a Keenu release list.")
wanted, channel = os.environ["WANTED"], os.environ["CHANNEL"]
for release in manifest["releases"]:
    if wanted and release["version"] != wanted:
        continue
    if not wanted and channel != "prerelease" and release["channel"] != "stable":
        continue
    for kind in os.environ["KINDS"].split():
        for artifact in release["artifacts"]:
            if artifact["platform"] == os.environ["PLATFORM"] and artifact["kind"] == kind:
                print(release["version"], artifact["kind"], artifact["file_name"], artifact["url"], artifact["sha256"],
                      "signed" if artifact["signature"]["signed"] else "unsigned", sep="\n")
                sys.exit(0)
sys.exit("no matching release has a file for this computer.")
EOF
)" || fail "$PICK"
VERSION="$(printf '%s\n' "$PICK" | sed -n 1p)"
KIND="$(printf '%s\n' "$PICK" | sed -n 2p)"
NAME="$(printf '%s\n' "$PICK" | sed -n 3p)"
URL="$(printf '%s\n' "$PICK" | sed -n 4p)"
SUM="$(printf '%s\n' "$PICK" | sed -n 5p)"
SIGNED="$(printf '%s\n' "$PICK" | sed -n 6p)"
case "$NAME" in */*|*..*) fail "the release list names a file oddly: $NAME" ;; esac
secure_url "$URL" || fail "$URL is not a secure address."

if [ "${KEENU_YES:-}" != 1 ] && [ -t 0 ]; then
    printf 'Install Keenu %s (%s)? [Y/n] ' "$VERSION" "$KIND"
    read -r answer
    case "$answer" in ''|y*|Y*) ;; *) say "Nothing was changed."; exit 0 ;; esac
fi

say "Downloading $NAME"
curl -fL --progress-bar "$URL" -o "$WORK/$NAME" || fail "the download failed."
[ "$(sha256 "$WORK/$NAME")" = "$SUM" ] ||
    fail "the download does not match its published checksum, so it was not installed. Try again; if it keeps happening, report it."
say "Checksum verified"

case "$OS" in
    Linux)
        mkdir "$WORK/unpacked"
        tar -xzf "$WORK/$NAME" -C "$WORK/unpacked"
        INSTALLER="$(find "$WORK/unpacked" -mindepth 2 -maxdepth 2 -name install.sh | head -n1)"
        [ -n "$INSTALLER" ] || fail "that package holds no installer."
        bash "$INSTALLER"
        # The package's uninstaller stays around as a command.
        install -Dm755 "$(dirname "$INSTALLER")/uninstall.sh" "$HOME/.local/bin/keenu-uninstall"
        ;;
    Darwin)
        [ "$SIGNED" = signed ] || note "Note: this build is not notarized, so macOS will refuse to open it until you allow it in System Settings > Privacy & Security."
        MOUNT="$WORK/mount"
        mkdir "$MOUNT"
        hdiutil attach -quiet -nobrowse -mountpoint "$MOUNT" "$WORK/$NAME" || fail "could not open the disk image."
        pkill -x keenu-macos 2>/dev/null || true
        mkdir -p "$HOME/Applications"
        rm -rf "$HOME/Applications/Keenu.app"
        cp -R "$MOUNT/Keenu.app" "$HOME/Applications/"
        hdiutil detach -quiet "$MOUNT" || true
        ;;
esac

say "Keenu $VERSION is installed"
note "To remove it later: sh install.sh --uninstall"
