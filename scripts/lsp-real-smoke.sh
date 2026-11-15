#!/usr/bin/env bash
# Manual real-language-server switching smoke.
#
# CI does not run this: runners do not reliably have all four servers. The
# script skips explicitly when any binary is absent.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
root="$(mktemp -d "${TMPDIR:-/tmp}/anvil-lsp-real.XXXXXX")"
workspace="$root/workspace"
runtime="$root/runtime"
state="$root/state"
session="anvil-lsp-real-$$"
keep=0

default_anvil="$repo_root/dist/anvil/bin/anvil"
default_ctl="$repo_root/dist/anvil/bin/anvil-ctl"
anvil_bin="${ANVIL_BIN:-$default_anvil}"
anvil_ctl="${ANVIL_CTL:-$default_ctl}"
timeout_secs="${ANVIL_LSP_SMOKE_TIMEOUT:-60}"

cleanup() {
	"$anvil_ctl" --workspace "$workspace" quit --force >/dev/null 2>&1 || true
	tmux -L "$session" kill-server >/dev/null 2>&1 || true
	if [ "$keep" -eq 1 ]; then
		printf 'preserved LSP smoke state at %s\n' "$root" >&2
	else
		rm -rf "$root"
	fi
}
trap cleanup EXIT

missing=()
for command in tmux clangd jdtls rust-analyzer gopls; do
	if ! command -v "$command" >/dev/null 2>&1; then
		missing+=("$command")
	fi
done
if [ ! -x "$anvil_bin" ]; then
	missing+=("anvil")
fi
if [ ! -x "$anvil_ctl" ]; then
	missing+=("anvil-ctl")
fi
if [ "${#missing[@]}" -gt 0 ]; then
	printf 'SKIP: missing %s; install them to run the real LSP smoke\n' "${missing[*]}"
	exit 0
fi

mkdir -p "$workspace" "$runtime" "$state"
printf 'int main(void) { return 0; }\n' >"$workspace/main.c"
printf 'class Main { public static void main(String[] args) {} }\n' >"$workspace/Main.java"
printf 'fn main() {}\n' >"$workspace/main.rs"
printf 'package main\n\nfunc main() {}\n' >"$workspace/main.go"
printf '[package]\nname = "lsp-smoke"\nversion = "0.1.0"\nedition = "2021"\n' >"$workspace/Cargo.toml"
printf 'module example.com/lsp-smoke\n\ngo 1.23\n' >"$workspace/go.mod"

export XDG_RUNTIME_DIR="$runtime"
export XDG_STATE_HOME="$state"
export ANVIL_SKIP_TERMINAL_QUERY=1
export TERM="${TERM:-screen-256color}"

fail() {
	keep=1
	printf 'FAIL: %s\n' "$1" >&2
	tmux -L "$session" capture-pane -p -t anvil -S -100 2>/dev/null || true
	exit 1
}

count() {
	pgrep -fc "$1" 2>/dev/null || true
}

wait_count_above() {
	local pattern="$1" baseline="$2" label="$3"
	local deadline=$((SECONDS + timeout_secs))
	while [ "$SECONDS" -lt "$deadline" ]; do
		if [ "$(count "$pattern")" -gt "$baseline" ]; then
			return 0
		fi
		sleep 0.25
	done
	fail "$label did not start within ${timeout_secs}s"
}

wait_count_equal() {
	local pattern="$1" expected="$2" label="$3"
	local deadline=$((SECONDS + timeout_secs))
	while [ "$SECONDS" -lt "$deadline" ]; do
		if [ "$(count "$pattern")" -eq "$expected" ]; then
			return 0
		fi
		sleep 0.25
	done
	fail "$label did not stop within ${timeout_secs}s"
}

ctl() {
	"$anvil_ctl" --workspace "$workspace" --timeout-ms 5000 "$@"
}

open_with_lsp() {
	local path="$1" pattern="$2" label="$3" baseline="$4"
	ctl open "$path" >/dev/null
	wait_count_above "$pattern" "$baseline" "$label"
}

clangd_base="$(count '[c]langd')"
jdtls_base="$(count '[j]dtls|org[.]eclipse[.]jdt[.]ls')"
rust_base="$(count '[r]ust-analyzer')"
gopls_base="$(count '[g]opls')"

tmux -L "$session" new-session -d -s anvil -x 120 -y 40 "$anvil_bin '$workspace/main.c'"
for _ in $(seq 1 100); do
	if ctl status >/dev/null 2>&1; then
		break
	fi
	sleep 0.1
done
ctl status >/dev/null 2>&1 || fail "control socket did not come up"

open_with_lsp "$workspace/main.c" '[c]langd' "clangd" "$clangd_base"
wait_count_equal '[j]dtls|org[.]eclipse[.]jdt[.]ls' "$jdtls_base" "clangd handoff"
wait_count_equal '[r]ust-analyzer' "$rust_base" "clangd handoff"
wait_count_equal '[g]opls' "$gopls_base" "clangd handoff"

open_with_lsp "$workspace/Main.java" '[j]dtls|org[.]eclipse[.]jdt[.]ls' "jdtls" "$jdtls_base"
wait_count_equal '[c]langd' "$clangd_base" "jdtls handoff"

open_with_lsp "$workspace/main.rs" '[r]ust-analyzer' "rust-analyzer" "$rust_base"
wait_count_equal '[j]dtls|org[.]eclipse[.]jdt[.]ls' "$jdtls_base" "rust-analyzer handoff"

open_with_lsp "$workspace/main.go" '[g]opls' "gopls" "$gopls_base"
wait_count_equal '[r]ust-analyzer' "$rust_base" "gopls handoff"

printf 'PASS: clangd -> jdtls -> rust-analyzer -> gopls, one active server per switch\n'
