#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 zickrian
# Packs dist/keenu-<version>-<arch>-linux.tar.gz (+ .sha256) with ready-built programs; arg: version (default Cargo.toml).
# Needs libgtk-4-dev for keenu-shelf, and cmake g++ libclang-dev libvulkan-dev glslc for keenu-whisper.
set -euo pipefail

cd "$(dirname "$0")/.."
VERSION="${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)}"
NAME="keenu-$VERSION-$(uname -m)-linux"
OUT="dist/$NAME"

cargo build --release --locked -p keenu-daemon -p keenu-shelf -p keenu-whisper

rm -rf "$OUT" "dist/$NAME.tar.gz" "dist/$NAME.tar.gz.sha256"
mkdir -p "$OUT/bin"
BUILT="${CARGO_TARGET_DIR:-target}/release"
install -m755 "$BUILT/keenud" "$BUILT/keenu-shelf" "$BUILT/keenu-whisper" "$OUT/bin/"
strip "$OUT/bin/keenud" "$OUT/bin/keenu-shelf" "$OUT/bin/keenu-whisper"
cp -r gnome-extension data install.sh uninstall.sh README.md LICENSE CHANGELOG.md "$OUT/"
# What install.sh installs as commands: the voice set-up and the battery charge limit.
mkdir -p "$OUT/scripts"
install -m755 scripts/setup-voice.sh scripts/keenu-charge-limit "$OUT/scripts/"
# Compiled on install, for the GLib there.
rm -f "$OUT/gnome-extension/schemas/gschemas.compiled"

tar -C dist -czf "dist/$NAME.tar.gz" "$NAME"
(cd dist && sha256sum "$NAME.tar.gz" > "$NAME.tar.gz.sha256")
rm -rf "$OUT"
echo "dist/$NAME.tar.gz"
