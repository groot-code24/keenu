#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 zickrian
# Points the PKGBUILD at a published release (packaging/aur/update.sh 1.2.3) and writes .SRCINFO for the AUR.
# The checksum comes from the release's .sha256; .SRCINFO needs makepkg, on Arch or in Docker.
set -euo pipefail

VERSION="${1:?usage: $0 <version>}"
cd "$(dirname "$0")"
URL="https://github.com/YOUR-GITHUB-OWNER/keenu/releases/download/v$VERSION/keenu-$VERSION-x86_64-linux.tar.gz"
SUM="$(curl -fsSL "$URL.sha256" | cut -d' ' -f1)"
[[ "$SUM" =~ ^[0-9a-f]{64}$ ]] || { echo "No checksum found at $URL.sha256" >&2; exit 1; }

sed -i -e "s/^pkgver=.*/pkgver=$VERSION/" -e "s/^pkgrel=.*/pkgrel=1/" \
    -e "s/^sha256sums_x86_64=.*/sha256sums_x86_64=('$SUM')/" PKGBUILD
if command -v makepkg >/dev/null; then
    makepkg --printsrcinfo > .SRCINFO
else
    docker run --rm -v "$PWD:/pkg:ro" archlinux:latest bash -c \
        'useradd -m b && cp /pkg/PKGBUILD /pkg/*.install /home/b/ && cd /home/b && su b -c "makepkg --printsrcinfo"' > .SRCINFO
fi
echo "PKGBUILD and .SRCINFO are at $VERSION ($SUM)."
