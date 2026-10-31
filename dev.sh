#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Sets up to work on Keenu on Linux or macOS, and starts it. Installs no system software: it says what is
# missing and how to get it.
#
#   ./dev.sh          check, build, and start
#   ./dev.sh --check  only check
set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=scripts/common.sh
. scripts/common.sh

check_tools
[ "${1:-}" != "--check" ] || exit 0
[ "$TOOLS_OK" = 1 ] || exit 1

case "$(uname -s)" in
    Linux)
        step "Building and installing for this user (programs in ~/.local/bin, the extension in your home folder)"
        ./install.sh
        step "Done. Log out and back in if this is the first install. To follow it: journalctl --user -fu keenud"
        ;;
    Darwin)
        step "Building and starting, with its data in .dev-data"
        export KEENU_DATA_DIR="$PWD/.dev-data" KEENU_LOG=debug
        exec cargo run -p keenu-macos
        ;;
esac
