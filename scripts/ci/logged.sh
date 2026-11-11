#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Runs a command, and if it fails puts what matters of its output on the run's page as annotations, which anyone
# can read there: a public repository's full logs need signing in.
set -u

log="$(mktemp)"
"$@" 2>&1 | tee "$log"
status=${PIPESTATUS[0]}

# Newlines and percent signs as a workflow command needs them.
encode() { cut -c1-300 | tr -d '\r' | sed -e 's/%/%25/g' | awk '{printf "%s%%0A", $0}'; }

if [ "$status" -ne 0 ]; then
    echo "::error title=$1 failed with exit code $status (the lines that say why)::$(grep -E '^error|^ +--> |panicked at|FAILED|failed:|Smoke test failed|FAIL ' "$log" | head -n 40 | encode)"
    echo "::error title=$1 failed with exit code $status (the end of its output)::$(tail -n 50 "$log" | encode)"
fi
rm -f "$log"
exit "$status"
