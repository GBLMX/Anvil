#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

# Legacy owners are explicit as file:count so adding a new thread or
# unbounded channel inside an already-migrated file still fails. Shrink
# these counts as workers migrate.
allowed_threads="
crates/anvil-control/src/lib.rs:2
crates/anvil-ipc/src/client.rs:7
crates/anvil-lsp-client/src/worker.rs:1
crates/anvil-mod-desktop/src/lib.rs:1
crates/anvil-mod-git/src/lib.rs:2
crates/anvil-plugin-host/src/runtime.rs:3
crates/anvil-session/src/editor_service.rs:2
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
crates/anvil-workspace/src/search.rs:2
"
allowed_channels="
crates/anvil-control/src/lib.rs:2
crates/anvil-session/src/editor_service.rs:2
crates/anvil-session/src/host_session.rs:0
crates/anvil-session/src/plugin_supervisor.rs:0
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

# Resolve inherited workspace dependencies so centralization cannot bypass
# the crate-boundary checks.
python3 - <<'PY'
import json
import subprocess
import sys

metadata = json.loads(
    subprocess.check_output(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        text=True,
    )
)
packages = {package["name"]: package for package in metadata["packages"]}


# Normal dependencies only: dev/build dependencies are not shipped.
def dependencies(name):
    return {
        dependency["name"]
        for dependency in packages[name]["dependencies"]
        if dependency.get("kind") is None
    }


task_forbidden = {"tokio", "anvil-tui", "anvil-platform", "anvil-session"}
task_bad = sorted(dependencies("anvil-task") & task_forbidden)
if task_bad:
    print(
        "architecture: anvil-task must stay std-only and UI/platform-free: "
        + ", ".join(task_bad),
        file=sys.stderr,
    )
    sys.exit(1)

for crate in ("anvil-core", "anvil-editor"):
    bad = sorted(
        dependency
        for dependency in dependencies(crate)
        if dependency.startswith(("anvil-tui", "anvil-platform", "anvil-session"))
    )
    if bad:
        print(
            f"architecture: {crate} must not depend on UI/session/platform crates: "
            + ", ".join(bad),
            file=sys.stderr,
        )
        sys.exit(1)

normal_workspace = set(packages)


def normal_dependencies(name):
    return {
        dependency for dependency in dependencies(name)
        if dependency in normal_workspace
    }


violations = []


def forbidden_by_prefix(dependency, prefixes):
    return dependency.startswith(prefixes)


base_crates = {"anvil-api", "anvil-core", "anvil-ipc", "anvil-task"}
for crate in sorted(base_crates):
    for dependency in normal_dependencies(crate):
        violations.append(("R1 L0", crate, dependency))

for crate in sorted(packages):
    if "anvil-tui" in normal_dependencies(crate) and not crate.startswith(
        "anvil-distro-"
    ):
        violations.append(("R2 TUI reverse", crate, "anvil-tui"))

module_crates = {
    name for name in packages if name.startswith("anvil-mod-")
}
module_dependent_exceptions = {
    "anvil-runtime": "composition root (ADR-0074)",
}
for crate in sorted(packages):
    for dependency in normal_dependencies(crate):
        if dependency not in module_crates:
            continue
        if crate.startswith(("anvil-distro-", "anvil-tui")):
            continue
        if crate in module_dependent_exceptions:
            continue
        violations.append(("R3 module reverse", crate, dependency))

service_crates = {
    "anvil-control",
    "anvil-dap",
    "anvil-lsp-client",
    "anvil-plugin-host",
    "anvil-process",
    "anvil-workspace",
}
service_forbidden = {"anvil-workspace", "anvil-runtime", "anvil-session"}
service_forbidden_prefixes = ("anvil-tui", "anvil-distro-")
for crate in sorted(service_crates):
    for dependency in normal_dependencies(crate):
        if dependency in service_forbidden or forbidden_by_prefix(
            dependency, service_forbidden_prefixes
        ):
            violations.append(("R4 service upward", crate, dependency))

session_forbidden = {
    "anvil-control",
    "anvil-workspace",
    "anvil-runtime",
}
session_forbidden_prefixes = ("anvil-tui", "anvil-distro-", "anvil-mod-")
for dependency in normal_dependencies("anvil-session"):
    if dependency in session_forbidden or forbidden_by_prefix(
        dependency, session_forbidden_prefixes
    ):
        violations.append(("R5 session upward", "anvil-session", dependency))

contract_forbidden = {"anvil-workspace", "anvil-runtime"}
contract_forbidden_prefixes = ("anvil-tui", "anvil-distro-", "anvil-mod-")
for dependency in normal_dependencies("anvil-module"):
    if dependency in contract_forbidden or forbidden_by_prefix(
        dependency, contract_forbidden_prefixes
    ):
        violations.append(("R6 contract inward", "anvil-module", dependency))

if violations:
    for rule, source, dependency in violations:
        print(
            f"architecture: {rule} violation: {source} -> {dependency}",
            file=sys.stderr,
        )
    sys.exit(1)
PY

echo "architecture: ok"
