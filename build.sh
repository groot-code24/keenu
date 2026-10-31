#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Builds Keenu for this system.
#
#   ./build.sh            the programs, in target/release
#   ./build.sh --package  also what is published: a tarball, .deb and .rpm on Linux, an app and DMG on macOS
set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=scripts/common.sh
. scripts/common.sh

check_tools
[ "$TOOLS_OK" = 1 ] || exit 1

case "$(uname -s)" in
    Linux)
        step "Building"
        if [ "${1:-}" = "--package" ]; then
            tarball="$(scripts/package.sh | tail -n1)"
            scripts/package-system.sh "$tarball"
        else
            cargo build --release --locked -p keenu-daemon
        fi
        ;;
    Darwin)
        step "Building"
        cargo build --release --locked -p keenu-macos
        if [ "${1:-}" = "--package" ]; then
            packaging/macos/build-app.sh
        fi
        ;;
esac
step "Done"
