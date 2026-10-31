#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Installs and starts Keenu on Linux or macOS from an official release, with no development tools needed.
#
#   curl -fsSL https://raw.githubusercontent.com/groot-code24/keenu/main/scripts/install/install.sh | sh
#
# To read it first, which is a good idea with any script from the internet:
#
#   curl -fsSLO https://raw.githubusercontent.com/groot-code24/keenu/main/scripts/install/install.sh
#   less install.sh
#   sh install.sh
#
# Environment: KEENU_VERSION picks a version, KEENU_CHANNEL=prerelease prefers test versions, KEENU_YES=1 asks
# nothing, KEENU_NO_START=1 does not start it, KEENU_MANIFEST_URL points at another manifest, KEENU_FORCE=1 installs on
# an untested GNOME Shell. `sh install.sh --uninstall`, or `curl ... | sh -s -- --uninstall`, takes Keenu off again; your
# data stays.
set -eu

DEFAULT_MANIFEST="https://raw.githubusercontent.com/groot-code24/keenu/main/website/keenu-release.json"
# Signs the official release list. Another list, from KEENU_MANIFEST_URL, is yours to trust.
PUBLIC_KEY="lYDq7UwsRPdxHCtkc6IYT2cIbtZ+ik69NTsMF5+R8qk="

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
            pkill -x Keenu 2>/dev/null || true
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

if [ "$OS" = Linux ]; then
    have python3 || fail "please install python3: the release list is JSON and this script reads it with Python."
    have gnome-shell || fail "GNOME Shell was not found; on Linux, Keenu is a GNOME Shell extension. Other desktops are not supported yet."
    SHELL_VERSION="$(gnome-shell --version 2>/dev/null | grep -oE '[0-9]+' | head -n1 || true)"
    if [ -z "$SHELL_VERSION" ] || [ "$SHELL_VERSION" -lt 46 ] || [ "$SHELL_VERSION" -gt 51 ]; then
        [ "${KEENU_FORCE:-}" = 1 ] || fail "Keenu is made for GNOME Shell 46 to 51, and this is ${SHELL_VERSION:-unknown}. Nothing was changed. (KEENU_FORCE=1 installs anyway.)"
    fi
fi

MANIFEST="${KEENU_MANIFEST_URL:-$DEFAULT_MANIFEST}"
secure_url "$MANIFEST" || fail "$MANIFEST is not a secure address."

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT INT TERM

say "Looking up the release for $PLATFORM"
curl -fsSL "$MANIFEST" -o "$WORK/manifest.json" || fail "could not fetch the release list from $MANIFEST. If no release has been published yet, there is nothing to install."

if [ "$MANIFEST" = "$DEFAULT_MANIFEST" ]; then
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

# version, kind, file name, URL, SHA-256, signed and channel, one per line, for the best file of the newest release:
# the newest stable one, or the newest at all if none is stable yet.
if [ "$OS" = Darwin ]; then
    # Read with what every Mac has: its python3 is only a prompt to install the developer tools.
    PICK="$(KINDS="$KINDS" PLATFORM="$PLATFORM" WANTED="${KEENU_VERSION:-}" CHANNEL="${KEENU_CHANNEL:-stable}" osascript -l JavaScript - "$WORK/manifest.json" <<'EOF'
ObjC.import('Foundation');
function run(argv) {
    const environment = $.NSProcessInfo.processInfo.environment;
    const env = name => ObjC.unwrap(environment.objectForKey(name)) || '';
    const text = ObjC.unwrap($.NSString.stringWithContentsOfFileEncodingError(argv[0], $.NSUTF8StringEncoding, null));
    const manifest = JSON.parse(text);
    if (manifest.product !== 'Keenu')
        throw new Error('that address does not hold a Keenu release list.');
    const wanted = env('WANTED');
    let releases = manifest.releases;
    if (wanted)
        releases = releases.filter(release => release.version === wanted);
    else if (env('CHANNEL') !== 'prerelease') {
        const stable = releases.filter(release => release.channel === 'stable');
        if (stable.length > 0)
            releases = stable;
    }
    for (const release of releases) {
        for (const kind of env('KINDS').split(' ')) {
            const artifact = release.artifacts.find(item => item.platform === env('PLATFORM') && item.kind === kind);
            if (artifact)
                return [release.version, artifact.kind, artifact.file_name, artifact.url, artifact.sha256,
                    artifact.signature.signed ? 'signed' : 'unsigned', release.channel].join('\n');
        }
    }
    throw new Error('no matching release has a file for this computer.');
}
EOF
)" || fail "the release list has no file for this computer."
else
    PICK="$(KINDS="$KINDS" PLATFORM="$PLATFORM" WANTED="${KEENU_VERSION:-}" CHANNEL="${KEENU_CHANNEL:-stable}" python3 -I - "$WORK/manifest.json" <<'EOF'
import json, os, sys
manifest = json.load(open(sys.argv[1], encoding="utf-8"))
if manifest.get("product") != "Keenu":
    sys.exit("that address does not hold a Keenu release list.")
wanted, channel = os.environ["WANTED"], os.environ["CHANNEL"]
releases = manifest["releases"]
if wanted:
    releases = [release for release in releases if release["version"] == wanted]
elif channel != "prerelease":
    releases = [release for release in releases if release["channel"] == "stable"] or releases
for release in releases:
    for kind in os.environ["KINDS"].split():
        for artifact in release["artifacts"]:
            if artifact["platform"] == os.environ["PLATFORM"] and artifact["kind"] == kind:
                print(release["version"], artifact["kind"], artifact["file_name"], artifact["url"], artifact["sha256"],
                      "signed" if artifact["signature"]["signed"] else "unsigned", release["channel"], sep="\n")
                sys.exit(0)
sys.exit("no matching release has a file for this computer.")
EOF
)" || fail "the release list has no file for this computer."
fi
VERSION="$(printf '%s\n' "$PICK" | sed -n 1p)"
KIND="$(printf '%s\n' "$PICK" | sed -n 2p)"
NAME="$(printf '%s\n' "$PICK" | sed -n 3p)"
URL="$(printf '%s\n' "$PICK" | sed -n 4p)"
SUM="$(printf '%s\n' "$PICK" | sed -n 5p)"
SIGNED="$(printf '%s\n' "$PICK" | sed -n 6p)"
RELEASE_CHANNEL="$(printf '%s\n' "$PICK" | sed -n 7p)"
case "$NAME" in */*|*..*) fail "the release list names a file oddly: $NAME" ;; esac
secure_url "$URL" || fail "$URL is not a secure address."
[ "$RELEASE_CHANNEL" = stable ] || note "Note: Keenu $VERSION is a test version; no stable version has been released yet."

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
        MOUNT="$WORK/mount"
        mkdir "$MOUNT"
        hdiutil attach -quiet -nobrowse -mountpoint "$MOUNT" "$WORK/$NAME" || fail "could not open the disk image."
        pkill -x Keenu 2>/dev/null || true
        mkdir -p "$HOME/Applications"
        rm -rf "$HOME/Applications/Keenu.app"
        cp -R "$MOUNT/Keenu.app" "$HOME/Applications/"
        hdiutil detach -quiet "$MOUNT" || true
        ;;
esac

say "Keenu $VERSION is installed"
if [ "$OS" = Darwin ] && [ "${KEENU_NO_START:-}" != 1 ]; then
    [ "$SIGNED" = signed ] || note "This build is not notarized by Apple: if macOS refuses to open it, allow it in System Settings > Privacy & Security."
    open "$HOME/Applications/Keenu.app" && say "Keenu is running: look for it at the top of your screen and in the menu bar."
fi
note "To remove it later: curl -fsSL https://raw.githubusercontent.com/groot-code24/keenu/main/scripts/install/install.sh | sh -s -- --uninstall"
