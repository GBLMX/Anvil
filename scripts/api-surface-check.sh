#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

snapshot="docs/api-surface.sha256"
crates=(
    crates/anvil-api/src
    crates/anvil-ipc/src
    crates/anvil-control/src
    crates/anvil-module/src
    crates/anvil-session/src
    crates/anvil-editor/src
)

actual="$(python3 scripts/api_surface.py "${crates[@]}")"
expected="$(tr -d '[:space:]' < "$snapshot")"

if [ "$actual" != "$expected" ]; then
    echo "api-surface: public kernel declarations changed" >&2
    echo "review the change and update $snapshot deliberately" >&2
    echo "expected $expected, found $actual" >&2
    exit 1
fi

echo "api-surface: ok"
