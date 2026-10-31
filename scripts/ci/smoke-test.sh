#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 zickrian
# Run as root in a throwaway container: installs an Keenu package (tarball, .deb, .rpm or Arch), or runs the
# one-command installer given as an https:// address or a path ending in install.sh, and a headless GNOME Shell as
# "tester", failing on any "JS ERROR", the extension not loading, or keenud never on the bus.
set -euo pipefail

UUID="isle@zickrian.github.io"
DAEMON="io.github.zickrian.isle.Daemon"
# Seconds to wait for keenud, then to keep the shell running and watch its log.
WAIT=60
SETTLE=10

# A step heading, in bold.
say() { printf '\033[1m==> %s\033[0m\n' "$*"; }
# Stops the test with a reason.
fail() { printf 'Smoke test failed: %s\n' "$*" >&2; exit 1; }

# GNOME Shell and what installing needs, on Ubuntu/Debian, Fedora or Arch.
install_shell() {
    if command -v apt-get >/dev/null; then
        export DEBIAN_FRONTEND=noninteractive
        apt-get update -qq
        apt-get install -y -qq --no-install-recommends gnome-shell mutter xwayland dbus dbus-user-session \
            libglib2.0-bin gnome-shell-common gir1.2-gtk-4.0 libgtk-4-1 adwaita-icon-theme fonts-cantarell \
            librsvg2-common fontconfig curl ca-certificates procps util-linux python3 >/dev/null
    elif command -v dnf >/dev/null; then
        dnf -y -q install --setopt=install_weak_deps=False gnome-shell xorg-x11-server-Xwayland dbus-daemon \
            dbus-tools glib2 librsvg2 adwaita-icon-theme mesa-dri-drivers fontconfig curl tar findutils \
            procps-ng util-linux shadow-utils python3
    elif command -v pacman >/dev/null; then
        pacman -Syu --noconfirm --needed gnome-shell xorg-xwayland dbus glib2 librsvg adwaita-icon-theme \
            fontconfig curl procps-ng util-linux >/dev/null
    else
        fail "no apt-get, dnf or pacman to install GNOME Shell with."
    fi
}

# As root: the system around the shell, then the session as "tester".
main() {
    local package="${1:-}"
    [ -f "$package" ] || [[ "$package" == https://* ]] ||
        fail "usage: $0 <keenu-….tar.gz | keenu_….deb | keenu-….rpm | keenu-bin-….pkg.tar.zst | install.sh | https://…/install.sh>"
    [ "$(id -u)" -eq 0 ] || fail "run this as root, in a throwaway container."
    [[ "$package" == https://* ]] || package="$(readlink -f "$package")"

    if ! command -v gnome-shell >/dev/null || ! command -v dbus-run-session >/dev/null; then
        say "Installing GNOME Shell"
        install_shell
    fi
    gnome-shell --version

    case "$package" in
        *.deb)
            say "Installing $(basename "$package")"
            apt-get install -y -qq "$package" >/dev/null ;;
        *.rpm)
            say "Installing $(basename "$package")"
            dnf -y -q install "$package" ;;
        *.pkg.tar.zst)
            say "Installing $(basename "$package")"
            pacman -U --noconfirm "$package" >/dev/null ;;
        *.tar.gz | *install.sh) ;;
        *) fail "unknown package type: $package" ;;
    esac

    id tester >/dev/null 2>&1 || useradd -m tester
    local uid
    uid="$(id -u tester)"
    # Xwayland's socket folder must belong to root; the user's runtime folder to the user.
    install -d -m 1777 /tmp/.X11-unix
    install -d -m 700 -o tester -g tester "/run/user/$uid"
    # No systemd or logind here: GNOME Shell would look for them if /run/systemd were there.
    rm -rf /run/systemd
    mkdir -p /run/dbus
    [ -S /run/dbus/system_bus_socket ] || dbus-daemon --system --fork

    local script
    script="$(readlink -f "$0")"
    # A copy the user can read, wherever the package is.
    if [[ "$package" != https://* ]]; then
        install -D -m 644 "$package" "/tmp/keenu-smoke/$(basename "$package")"
        package="/tmp/keenu-smoke/$(basename "$package")"
    fi
    runuser -u tester -- env -i HOME="/home/tester" USER=tester PATH="/home/tester/.local/bin:$PATH" \
        XDG_RUNTIME_DIR="/run/user/$uid" GSETTINGS_BACKEND=keyfile LANG=C.UTF-8 KEENU_MANIFEST_URL="${KEENU_MANIFEST_URL:-}" \
        dbus-run-session -- bash "$script" --session "$package" "$(dirname "$script")/../.."
}

# As "tester", on a fresh session bus: install for the user if a tarball, then run the shell.
session() {
    local package="$1"
    cd "$HOME"
    if [[ "$package" == https://* ]]; then
        say "Installing with the one command from $package"
        curl -fsSL "$package" | KEENU_YES=1 sh
    elif [[ "$package" == *install.sh ]]; then
        say "Installing with the one command, from $KEENU_MANIFEST_URL"
        KEENU_YES=1 sh "$package"
    elif [[ "$package" == *.tar.gz ]]; then
        say "Installing $(basename "$package") with its install.sh"
        mkdir -p "$HOME/unpacked"
        tar -xzf "$package" -C "$HOME/unpacked"
        bash "$HOME"/unpacked/keenu-*/install.sh
    else
        gsettings set org.gnome.shell enabled-extensions "['$UUID']"
    fi

    say "Starting GNOME Shell"
    local log="$HOME/gnome-shell.log"
    gnome-shell --headless --wayland --virtual-monitor 1280x720 >"$log" 2>&1 &
    local shell=$! waited=0
    until bus_has "$DAEMON"; do
        kill -0 "$shell" 2>/dev/null || { cat "$log"; fail "GNOME Shell exited."; }
        if [ "$waited" -ge "$WAIT" ]; then
            cat "$log"
            fail "keenud never appeared on the session bus ($DAEMON) in $WAIT s."
        fi
        sleep 1
        waited=$((waited + 1))
    done
    say "keenud is on the session bus after ${waited} s"
    sleep "$SETTLE"

    local info state
    info="$(gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
        --method org.gnome.Shell.Extensions.GetExtensionInfo "$UUID" 2>&1 || true)"
    state="$(printf '%s' "$info" | grep -oE "'state': <[0-9.]+>" | grep -oE '[0-9]+' | head -n1 || true)"
    local keenud whisper
    keenud="$(command -v keenud || echo "$HOME/.local/bin/keenud")"
    whisper="$(command -v keenu-whisper || echo "$HOME/.local/bin/keenu-whisper")"
    "$keenud" --version
    "$keenud" --doctor || fail "keenud --doctor failed."
    # Speech recognition starts with no GPU and no Vulkan loader, on the processor.
    say "keenu-whisper --probe"
    local probe
    probe="$("$whisper" --probe 2>&1 || true)"
    printf '%s\n' "$probe"
    [[ "$probe" == *'"device"'* ]] || fail "keenu-whisper --probe did not answer."
    kill "$shell" 2>/dev/null || true
    wait "$shell" 2>/dev/null || true

    echo "--- GNOME Shell's log about Keenu and errors:"
    grep -iE 'keenu|JS ERROR|JS WARNING' "$log" || echo "(nothing)"
    echo "---"
    if grep -q 'JS ERROR' "$log"; then
        fail "GNOME Shell logged a JS ERROR (above)."
    fi
    # ExtensionState: 1 enabled (ACTIVE), 3 error, 4 out of date, 7 activating (GNOME 49 and newer).
    case "$state" in
        1 | 7) say "The extension is loaded (state $state)" ;;
        *) printf '%s\n' "$info"; fail "the extension did not load (state ${state:-unknown})." ;;
    esac
    say "Smoke test passed"
}

# Whether a name is owned on the session bus.
bus_has() {
    gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus \
        --method org.freedesktop.DBus.NameHasOwner "$1" 2>/dev/null | grep -q true
}

if [ "${1:-}" = --session ]; then
    session "$2" "$3"
else
    main "$@"
fi
