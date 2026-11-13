#!/usr/bin/env bash
# End-to-end Anvil control-plane smoke test.
#
# Drives a real TUI through anvil-ctl: control socket -> TUI actor ->
# editor/gateway/index. Direct file writes are only fixtures and assertions.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
root="$(mktemp -d "${TMPDIR:-/tmp}/anvil-ctl-smoke.XXXXXX")"
workspace="$root/workspace"
runtime="$root/runtime"
state="$root/state"
transcript="$root/transcript.log"
socket_log="$root/gateway.log"
session="anvil-ctl-smoke-$$"
keep=0

default_bin="$repo_root/dist/anvil/bin/anvil"
default_ctl="$repo_root/dist/anvil/bin/anvil-ctl"
if [[ -x "$default_bin" ]]; then
	anvil_bin="${ANVIL_BIN:-$default_bin}"
else
	anvil_bin="${ANVIL_BIN:-$(command -v anvil || true)}"
fi
if [[ -x "$default_ctl" ]]; then
	anvil_ctl="${ANVIL_CTL:-$default_ctl}"
else
	anvil_ctl="${ANVIL_CTL:-$(command -v anvil-ctl || true)}"
fi

fail() {
	keep=1
	{
		printf '\nSMOKE FAILURE: %s\n' "$1"
		printf 'root=%s\n' "$root"
		if command -v tmux >/dev/null 2>&1; then
			tmux -L "$session" capture-pane -p -t anvil -S -200 2>/dev/null || true
		fi
	} >>"$transcript"
	printf 'ctl smoke failed: %s\n' "$1" >&2
	printf 'transcript and logs kept at %s\n' "$root" >&2
	exit 1
}

cleanup() {
	if [[ "$keep" -eq 0 ]]; then
		tmux -L "$session" kill-server >/dev/null 2>&1 || true
		rm -rf "$root"
	else
		printf 'preserved smoke state at %s\n' "$root" >&2
	fi
}
trap cleanup EXIT

for command in git python3 tmux; do
	if ! command -v "$command" >/dev/null 2>&1; then
		printf 'skip: %s is required\n' "$command" >&2
		keep=1
		exit 0
	fi
done
if [[ -z "$anvil_bin" || ! -x "$anvil_bin" ]]; then
	printf 'skip: anvil binary not found; set ANVIL_BIN\n' >&2
	keep=1
	exit 0
fi
if [[ -z "$anvil_ctl" || ! -x "$anvil_ctl" ]]; then
	printf 'skip: anvil-ctl binary not found; set ANVIL_CTL\n' >&2
	keep=1
	exit 0
fi

mkdir -p "$workspace" "$runtime" "$state"
printf 'one\ntwo\n' >"$workspace/a.txt"
git -C "$workspace" init -q
git -C "$workspace" -c user.name=Test -c user.email=test@example.com add a.txt
git -C "$workspace" -c user.name=Test -c user.email=test@example.com commit -qm init
base_branch="$(git -C "$workspace" branch --show-current)"

export XDG_RUNTIME_DIR="$runtime"
export XDG_STATE_HOME="$state"
export ANVIL_SKIP_TERMINAL_QUERY=1
export ANVIL_GATEWAY_LOG="$socket_log"
export TERM="${TERM:-screen-256color}"

last_out=""
last_code=0
ctl() {
	printf '$ anvil-ctl %s\n' "$*" >>"$transcript"
	set +e
	last_out="$("$anvil_ctl" --workspace "$workspace" "$@" 2>"$root/ctl.err")"
	last_code=$?
	set -e
	printf '%s\n' "$last_out" >>"$transcript"
	printf 'exit=%d\n' "$last_code" >>"$transcript"
	if [[ -s "$root/ctl.err" ]]; then
		printf 'stderr: %s\n' "$(cat "$root/ctl.err")" >>"$transcript"
	fi
	return 0
}
expect_ok() {
	ctl "$@"
	if [[ "$last_code" -ne 0 ]]; then
		fail "anvil-ctl $* exited $last_code"
	fi
}
json_field() {
	python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["data"][sys.argv[1]])' "$1" <<<"$last_out"
}
assert_json() {
	local path="$1" expected="$2"
	local actual
	actual="$(json_field "$path")"
	if [[ "$actual" != "$expected" ]]; then
		fail "$path: expected '$expected', got '$actual'"
	fi
}
assert_contains() {
	local haystack="$1" needle="$2" label="$3"
	if [[ "$haystack" != *"$needle"* ]]; then
		fail "$label does not contain '$needle'"
	fi
}

tmux -L "$session" new-session -d -s anvil -x 120 -y 40 \
	"cd '$workspace' && exec '$anvil_bin' -w '$workspace' a.txt"
for _ in $(seq 1 100); do
	ctl --json status
	if [[ "$last_code" -eq 0 ]]; then
		break
	fi
	sleep 0.1
done
if [[ "$last_code" -ne 0 ]]; then
	fail "Anvil control socket did not become ready"
fi

expect_ok --json open a.txt +2:1
expect_ok --json edit set $'alpha\nbeta\n'
expect_ok --json query
assert_json text $'alpha\nbeta'
assert_json modified True
assert_json revision 1
assert_json undo_depth 1
assert_contains "$(json_field buffers)" "'active': True" "query buffers"

expect_ok --json command ':undo'
expect_ok --json query
assert_json text $'one\ntwo'
assert_json undo_depth 0
expect_ok --json command ':redo'
expect_ok --json save
[[ "$(cat "$workspace/a.txt")" == $'alpha\nbeta' ]] || fail "save did not update file"

expect_ok --json edit set $'staged\n'
expect_ok --json query
assert_json text $'staged'
expect_ok --json save
expect_ok --json command ':git-diff'
for _ in $(seq 1 50); do
	ctl --json command ':git-hunk-next'
	[[ "$last_code" -eq 0 ]] && break
	sleep 0.1
done
[[ "$last_code" -eq 0 ]] || fail "git hunk navigation did not become ready"
expect_ok --json command ':git-stage-hunk'
cached=""
for _ in $(seq 1 50); do
	cached="$(git -C "$workspace" diff --cached -- a.txt)"
	[[ "$cached" == *"+staged"* ]] && break
	sleep 0.1
done
assert_contains "$cached" "+staged" "git index"
expect_ok --json command ':q'

git -C "$workspace" checkout -q -b side
printf 'theirs\n' >"$workspace/conflict.txt"
git -C "$workspace" add conflict.txt
git -C "$workspace" -c user.name=Test -c user.email=test@example.com commit -qm side
git -C "$workspace" checkout -q "$base_branch"
printf 'ours\n' >"$workspace/conflict.txt"
git -C "$workspace" add conflict.txt
git -C "$workspace" -c user.name=Test -c user.email=test@example.com commit -qm ours
git -C "$workspace" merge side >/dev/null 2>&1 || true

expect_ok --json open conflict.txt
expect_ok --json command ':git-conflicts'
expect_ok --json command ':q'
expect_ok --json command ':git-resolve-ours'
unmerged=""
for _ in $(seq 1 50); do
	unmerged="$(git -C "$workspace" diff --name-only --diff-filter=U)"
	[[ -z "$unmerged" ]] && break
	sleep 0.1
done
[[ -z "$unmerged" ]] || fail "git conflict was not resolved"
[[ "$(cat "$workspace/conflict.txt")" == "ours" ]] || fail "git resolve did not write ours"

expect_ok --json command ':git-blame'
expect_ok --json command ':q'
expect_ok --json command ':git-conflicts'
expect_ok --json command ':q'
expect_ok --json edit set $'dirty\n'

ctl --json quit
if [[ "$last_code" -ne 5 ]]; then
	fail "quit without force should fail with exit 5, got $last_code"
fi
expect_ok --json quit --force
for _ in $(seq 1 50); do
	[[ -n "$(find "$state/anvil/sessions" -name '*.json' -print -quit 2>/dev/null)" ]] && break
	sleep 0.1
done
[[ -n "$(find "$state/anvil/sessions" -name '*.json' -print -quit 2>/dev/null)" ]] ||
	fail "session file was not written"

printf 'ctl smoke passed\n'
printf 'transcript: %s\n' "$transcript"
