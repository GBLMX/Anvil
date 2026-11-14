#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

# Legacy owners are explicit as file:count so adding a new thread or
# unbounded channel inside an already-migrated file still fails. Shrink
# these counts as workers migrate.
allowed_threads="
crates/anvil-control/src/lib.rs:1
crates/anvil-ipc/src/client.rs:7
crates/anvil-lsp-client/src/worker.rs:1
crates/anvil-mod-desktop/src/lib.rs:1
crates/anvil-mod-git/src/lib.rs:2
crates/anvil-plugin-host/src/runtime.rs:2
crates/anvil-session/src/editor_service.rs:3
crates/anvil-session/src/host_session.rs:1
crates/anvil-syntax/src/lib.rs:1
crates/anvil-task/src/lib.rs:1
crates/anvil-tui/src/debug_panel.rs:1
crates/anvil-tui/src/debugger.rs:1
crates/anvil-tui/src/plugin_document.rs:1
crates/anvil-tui/src/task_runner.rs:1
crates/anvil-tui/src/tests/gateway_multi_root.rs:1
crates/anvil-tui/src/tests/interaction.rs:1
crates/anvil-workspace/src/lib.rs:10
"
allowed_channels="
crates/anvil-control/src/lib.rs:2
crates/anvil-session/src/editor_service.rs:2
crates/anvil-session/src/host_session.rs:2
crates/anvil-session/src/plugin_supervisor.rs:2
crates/anvil-tui/src/debug_panel.rs:1
crates/anvil-tui/src/debugger.rs:1
crates/anvil-tui/src/plugin_document.rs:1
"

check_files() {
    local pattern="$1"
    local allowed="$2"
    local label="$3"
    local entry
    local file
    local expected
    local actual

    for entry in $allowed; do
        file="${entry%:*}"
        expected="${entry##*:}"
        actual="$(grep -cE "$pattern" "$file" 2>/dev/null || true)"
        if [ "$actual" -ne "$expected" ]; then
            echo "architecture: $label count changed in $file: expected $expected, found $actual" >&2
            echo "update scripts/architecture-check.sh only with a migration plan" >&2
            exit 1
        fi
    done

    for file in $(grep -RlE "$pattern" crates/*/src --include='*.rs' || true); do
        if ! printf '%s\n' "$allowed" | grep -Fq "$file:"; then
            echo "architecture: new $label in $file" >&2
            echo "add it to scripts/architecture-check.sh only with a migration plan" >&2
            exit 1
        fi
    done
}

check_files '(std::)?thread::(spawn|Builder::new)' "$allowed_threads" "production thread spawn"
check_files 'mpsc::channel\(\)' "$allowed_channels" "unbounded mpsc channel"

if grep -Eq '(^|[" ])(tokio|anvil-tui|anvil-platform|anvil-session)([" =]|$)' crates/anvil-task/Cargo.toml; then
    echo "architecture: anvil-task must stay std-only and UI/platform-free" >&2
    exit 1
fi

for crate in anvil-core anvil-editor; do
    if grep -Eq 'anvil-(tui|tui-kit|platform|session)' "crates/$crate/Cargo.toml"; then
        echo "architecture: $crate must not depend on UI/session/platform crates" >&2
        exit 1
    fi
done

echo "architecture: ok"
