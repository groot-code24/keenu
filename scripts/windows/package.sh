#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Makes Keenu's Windows installers from a built keenu.exe: an MSI, a setup program and a portable ZIP, in dist/.
# Runs anywhere with wixl (msitools), makensis and zip; the release workflow runs it on Linux and signs on Windows.
#
#   scripts/windows/package.sh <keenu.exe> [version] [arch]
set -euo pipefail

cd "$(dirname "$0")/../.."
EXE="${1:?Usage: $0 <keenu.exe> [version] [arch]}"
VERSION="${2:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)}"
ARCH="${3:-x64}"
PUBLISHER="${KEENU_PUBLISHER:-Keenu contributors}"

for tool in wixl makensis zip; do
    command -v "$tool" >/dev/null || { echo "Please install $tool (packages: wixl, nsis, zip)." >&2; exit 1; }
done
[ -f "$EXE" ] || { echo "$EXE was not found." >&2; exit 1; }
EXE="$(readlink -f "$EXE")"

# Windows Installer versions are numbers only: 2.0.0-alpha.1 goes in as 2.0.0.
NUMERIC="${VERSION%%-*}"
OUT=dist
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$OUT"
BASE="Keenu-$VERSION-windows-$ARCH"
cp "$EXE" "$WORK/keenu.exe"
cp LICENSE "$WORK/LICENSE.txt"
cp packaging/icons/keenu.ico packaging/windows/Package.wxs "$WORK/"

# The MSI. wixl wants its files by relative paths. Per-user, it needs no administrator.
(cd "$WORK" && wixl -a x64 \
    -D "Version=$NUMERIC" -D "Publisher=$PUBLISHER" \
    -D "ExePath=keenu.exe" -D "IconPath=keenu.ico" -D "LicensePath=LICENSE.txt" \
    -o "$OLDPWD/$OUT/$BASE.msi" Package.wxs)
echo "$OUT/$BASE.msi"

# The setup program.
makensis -V2 -DVERSION="$VERSION" -DNUMERIC_VERSION="$NUMERIC.0" -DEXE="$EXE" \
    -DICON="$PWD/packaging/icons/keenu.ico" -DLICENSE="$WORK/LICENSE.txt" \
    -DOUT="$PWD/$OUT/$BASE-setup.exe" packaging/windows/installer.nsi
echo "$OUT/$BASE-setup.exe"

# The portable ZIP.
rm -f "$OUT/$BASE-portable.zip"
(cd "$WORK" && zip -q -9 "$OLDPWD/$OUT/$BASE-portable.zip" keenu.exe LICENSE.txt)
echo "$OUT/$BASE-portable.zip"
