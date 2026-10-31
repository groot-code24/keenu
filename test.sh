#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Runs Keenu's tests on Linux or macOS, and says plainly what ran, what passed and what was skipped.
set -uo pipefail
cd "$(dirname "$0")" || exit 1
# shellcheck source=scripts/common.sh
. scripts/common.sh

check_tools
[ "$TOOLS_OK" = 1 ] || exit 1

FAILED=0
report=""
result() { report="$report$(printf '%-28s %-5s %s' "$1" "$2" "${3:-}")"$'\n'; [ "$2" != FAIL ] || FAILED=1; }

step "Formatting"
if cargo fmt --all -- --check; then result "cargo fmt" PASS; else result "cargo fmt" FAIL; fi

shared=(-p keenu-core -p keenu-security -p keenu-integrations -p keenu-platform -p keenu-ui -p keenu-app -p keenu-release)
step "Unit and integration tests"
case "$(uname -s)" in
    Linux)
        if cargo test --locked "${shared[@]}" -p keenu-daemon; then result "unit and integration tests" PASS; else result "unit and integration tests" FAIL; fi
        if command -v node >/dev/null; then
            step "The extension's tests"
            if node --test tests/extension/*.test.js tests/website/*.test.js; then result "extension and website" PASS; else result "extension and website" FAIL; fi
        else
            result "extension and website" SKIP "node is not installed"
        fi
        ;;
    Darwin)
        if cargo test --locked "${shared[@]}" -p keenu-macos; then result "unit and integration tests" PASS; else result "unit and integration tests" FAIL; fi
        result "extension and website" SKIP "the extension is for Linux"
        ;;
esac
result "Windows tests" SKIP "these run on Windows, or in CI"

printf '\n%s' "$report"
exit "$FAILED"
