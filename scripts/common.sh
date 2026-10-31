#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# What dev.sh, build.sh and test.sh share: finding the tools, and saying what is missing.

MINIMUM_RUST="1.88.0"
TOOLS_OK=1

step() { printf '\033[1m==> %s\033[0m\n' "$*"; }

# Whether version $1 is at least $2, by each number.
at_least() {
    [ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -n1)" = "$2" ]
}

need() {
    if command -v "$1" >/dev/null; then
        printf '  %-16s found\n' "$1"
    else
        printf '  %-16s MISSING: %s\n' "$1" "$2"
        TOOLS_OK=0
    fi
}

check_tools() {
    printf '%s on %s\n' "$(uname -s)" "$(uname -m)"
    if command -v rustc >/dev/null; then
        local rust
        rust="$(rustc --version | cut -d' ' -f2)"
        if at_least "$rust" "$MINIMUM_RUST"; then
            printf '  %-16s %s\n' rust "$rust"
        else
            printf '  %-16s %s is older than %s: rustup update\n' rust "$rust" "$MINIMUM_RUST"
            TOOLS_OK=0
        fi
    else
        printf '  %-16s MISSING: https://rustup.rs\n' rust
        TOOLS_OK=0
    fi
    case "$(uname -s)" in
        Linux)
            need cc "a C compiler: build-essential (Debian, Ubuntu) or gcc (Fedora)"
            need pkg-config "pkg-config"
            need glib-compile-schemas "libglib2.0-bin (Debian, Ubuntu) or glib2 (Fedora)"
            if command -v pkg-config >/dev/null && ! pkg-config --exists gtk4; then
                printf '  %-16s not found: drag and drop on the shelf will be left out. libgtk-4-dev or gtk4-devel adds it.\n' gtk4
            fi
            if ! command -v cmake >/dev/null; then
                printf '  %-16s not found: speech recognition will be left out. cmake, g++ and libclang-dev add it.\n' cmake
            fi
            ;;
        Darwin)
            need xcrun "the Xcode command line tools: xcode-select --install"
            ;;
        *)
            printf 'This script is for Linux and macOS; on Windows, use the .ps1 scripts.\n' >&2
            TOOLS_OK=0
            ;;
    esac
    if [ "$TOOLS_OK" != 1 ]; then
        printf '\nNothing has been installed for you. Install what is missing and run this again.\n' >&2
    fi
}
