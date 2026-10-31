#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 zickrian
# Makes system-wide .deb and .rpm packages in dist/ from a release tarball (scripts/package.sh's output).
# Usage: scripts/package-system.sh <tarball> [deb] [rpm]; the .deb needs dpkg-dev, the .rpm rpmbuild and file.
set -euo pipefail

[ -f "${1:-}" ] || { echo "Usage: $0 <keenu-…-linux.tar.gz> [deb] [rpm]" >&2; exit 2; }
TARBALL="$(readlink -f "$1")"
shift
FORMATS=("$@")
[ "${#FORMATS[@]}" -gt 0 ] || FORMATS=(deb rpm)

cd "$(dirname "$0")/.."
umask 022
UUID="isle@zickrian.github.io"
MAINTAINER="${KEENU_MAINTAINER:-groot-code24 <groot-code24@users.noreply.github.com>}"
SUMMARY="A notch-style island for the GNOME Shell top bar"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

tar -C "$WORK" -xzf "$TARBALL"
SRC="$(find "$WORK" -mindepth 1 -maxdepth 1 -type d -name 'keenu-*' | head -n1)"
[ -x "$SRC/bin/keenud" ] || { echo "$TARBALL is not a release package." >&2; exit 1; }
VERSION="$(grep -oE '"version-name": *"[^"]*"' "$SRC/gnome-extension/metadata.json" | cut -d'"' -f4)"
# The architecture is the tarball's (keenu-<version>-<machine>-linux), not this computer's.
MACHINE="$(basename "$SRC" -linux)"
MACHINE="${MACHINE##*-}"
case "$MACHINE" in
    x86_64) DEB_ARCH=amd64 ;;
    aarch64) DEB_ARCH=arm64 ;;
    *) echo "Unknown architecture in $TARBALL: $MACHINE" >&2; exit 1 ;;
esac
mkdir -p dist

# A service file from data/, pointing at /usr/bin and the system's copy of the README.
service() {
    sed -e 's|@BINDIR@|/usr/bin|' -e 's|file://%h/.local/share/doc/keenu|file:///usr/share/doc/keenu|' \
        "$SRC/data/$1" > "$WORK/$1"
    install -Dm644 "$WORK/$1" "$2/$1"
}

# Everything but the license, under $1, with Inter in $2 (each system has its own fonts folder).
stage() {
    local root="$1" fonts="$1$2"
    install -Dm755 -t "$root/usr/bin" "$SRC/bin/keenud" "$SRC/bin/keenu-shelf" "$SRC/bin/keenu-whisper" \
        "$SRC/scripts/keenu-charge-limit"
    install -Dm755 "$SRC/scripts/setup-voice.sh" "$root/usr/bin/keenu-setup-voice"
    # The name people type.
    ln -s keenud "$root/usr/bin/keenu"

    local ext="$root/usr/share/gnome-shell/extensions/$UUID"
    mkdir -p "$ext"
    cp -r "$SRC/gnome-extension/." "$ext/"
    # The settings schema goes where the system compiles it; the extension then finds it there.
    install -Dm644 -t "$root/usr/share/glib-2.0/schemas" "$ext"/schemas/*.gschema.xml
    rm -rf "$ext/schemas"
    chmod -R u=rwX,go=rX "$ext"

    service io.github.zickrian.isle.Daemon.service "$root/usr/share/dbus-1/services"
    service io.github.zickrian.isle.ShelfWindow.service "$root/usr/share/dbus-1/services"
    service keenud.service "$root/usr/lib/systemd/user"
    install -Dm644 -t "$fonts" "$SRC/data/fonts/InterVariable.ttf" "$SRC/data/fonts/Inter-LICENSE.txt"
    install -Dm644 -t "$root/usr/share/doc/keenu" "$SRC/README.md" "$SRC/CHANGELOG.md"
}

# The .deb, its dependencies read from the libraries the programs link.
build_deb() {
    local root="$WORK/deb" depends
    stage "$root" /usr/share/fonts/truetype/keenu
    install -Dm644 "$SRC/LICENSE" "$root/usr/share/doc/keenu/LICENSE"
    cat > "$root/usr/share/doc/keenu/copyright" <<EOF
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: Keenu
Source: https://github.com/groot-code24/keenu

Files: *
Copyright: Keenu contributors
License: GPL-3.0-or-later
 On Debian systems, the full text is in /usr/share/common-licenses/GPL-3.

Files: usr/share/fonts/truetype/keenu/*
Copyright: The Inter Project Authors
License: OFL-1.1
 The full text is in /usr/share/fonts/truetype/keenu/Inter-LICENSE.txt.
EOF
    # The libraries the programs link, as package names and versions (libc6, libgtk-4-1, …).
    mkdir -p "$WORK/shlibs/debian"
    printf 'Source: keenu\n\nPackage: keenu\nArchitecture: %s\n' "$DEB_ARCH" > "$WORK/shlibs/debian/control"
    depends="$(cd "$WORK/shlibs" && dpkg-shlibdeps -O --warnings=0 "$root/usr/bin/keenud" "$root/usr/bin/keenu-shelf" "$root/usr/bin/keenu-whisper" |
        sed -n 's/^shlibs:Depends=//p')"

    mkdir -p "$root/DEBIAN"
    cat > "$root/DEBIAN/control" <<EOF
Package: keenu
Version: ${VERSION//-/\~}
Architecture: $DEB_ARCH
Maintainer: $MAINTAINER
Installed-Size: $(du -sk --exclude=DEBIAN "$root" | cut -f1)
Depends: $depends, gnome-shell (>= 46), dbus-user-session | default-dbus-session-bus
Recommends: libvulkan1
Section: gnome
Priority: optional
Homepage: https://github.com/groot-code24/keenu
Description: $SUMMARY
 Keenu turns the middle of the GNOME Shell top bar into a notch-style island: music
 with synced lyrics, calls, a focus timer, reminders, a calendar, a file shelf,
 app usage, charging and system alerts, and an optional voice assistant.
 .
 Each user logs out and back in, then turns the extension on once: gnome-extensions enable $UUID
EOF
    dpkg-deb --root-owner-group -Zxz --build "$root" "dist/keenu_${VERSION//-/\~}_${DEB_ARCH}.deb" >/dev/null
    echo "dist/keenu_${VERSION//-/\~}_${DEB_ARCH}.deb"
}

# The .rpm, from a spec wrapped around the staged files.
build_rpm() {
    local stage="$WORK/rpm-root" top="$WORK/rpmbuild"
    stage "$stage" /usr/share/fonts/keenu
    install -Dm644 "$SRC/LICENSE" "$stage/usr/share/licenses/keenu/LICENSE"
    mkdir -p "$top/SPECS"
    cat > "$top/SPECS/keenu.spec" <<EOF
Name: keenu
Version: ${VERSION//-/\~}
Release: 1
Summary: $SUMMARY
License: GPL-3.0-or-later AND OFL-1.1
URL: https://github.com/groot-code24/keenu
Packager: $MAINTAINER
Requires: gnome-shell >= 46
# Speech recognition on the graphics card; without it, on the processor.
Recommends: vulkan-loader
# Built by package.sh already, and stripped there.
%global debug_package %{nil}
%global __strip /bin/true

%description
Keenu turns the middle of the GNOME Shell top bar into a notch-style island: music
with synced lyrics, calls, a focus timer, reminders, a calendar, a file shelf,
app usage, charging and system alerts, and an optional voice assistant.

Each user logs out and back in, then turns the extension on once: gnome-extensions enable $UUID

%install
cp -a "$stage/." "%{buildroot}/"

%files
%license /usr/share/licenses/keenu/LICENSE
%doc /usr/share/doc/keenu
/usr/bin/keenu
/usr/bin/keenud
/usr/bin/keenu-shelf
/usr/bin/keenu-whisper
/usr/bin/keenu-setup-voice
/usr/bin/keenu-charge-limit
/usr/share/gnome-shell/extensions/$UUID
/usr/share/glib-2.0/schemas/org.gnome.shell.extensions.isle.gschema.xml
/usr/share/dbus-1/services/io.github.zickrian.isle.Daemon.service
/usr/share/dbus-1/services/io.github.zickrian.isle.ShelfWindow.service
/usr/lib/systemd/user/keenud.service
/usr/share/fonts/keenu
EOF
    rpmbuild --quiet -bb --target "$MACHINE" --define "_topdir $top" --define '_build_id_links none' \
        "$top/SPECS/keenu.spec"
    local built
    built="$(find "$top/RPMS" -name '*.rpm' | head -n1)"
    cp "$built" dist/
    echo "dist/$(basename "$built")"
}

for format in "${FORMATS[@]}"; do
    case "$format" in
        deb) build_deb ;;
        rpm) build_rpm ;;
        *) echo "Unknown format: $format (deb or rpm)" >&2; exit 2 ;;
    esac
done
