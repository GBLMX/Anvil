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

actual="$(
    grep -RhoE \
        '^[[:space:]]*pub(\([^)]*\))?[[:space:]]+(struct|enum|trait|fn|const|type|mod|use)[[:space:]]+[A-Za-z_][A-Za-z0-9_]*' \
        --include='*.rs' "${crates[@]}" \
        | sed 's/^[[:space:]]*//' \
        | sort -u \
        | sha256sum \
        | cut -d' ' -f1
)"
expected="$(tr -d '[:space:]' < "$snapshot")"

if [ "$actual" != "$expected" ]; then
    echo "api-surface: public kernel surface changed" >&2
    echo "review the change and update $snapshot deliberately" >&2
    echo "expected $expected, found $actual" >&2
    exit 1
fi

echo "api-surface: ok"
