#!/usr/bin/env bash
# End-to-end MCP stdio smoke test over the real Anvil control socket.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
root="$(mktemp -d "${TMPDIR:-/tmp}/anvil-mcp-smoke.XXXXXX")"
workspace="$root/workspace"
runtime="$root/runtime"
state="$root/state"
transcript="$root/transcript.log"
session="anvil-mcp-smoke-$$"
keep=0

anvil_bin="${ANVIL_BIN:-}"
anvil_ctl="${ANVIL_CTL:-}"
anvil_mcp="${ANVIL_MCP:-}"
if [[ -z "$anvil_bin" ]]; then
	if [[ -x "$repo_root/dist/anvil/bin/anvil" ]]; then
		anvil_bin="$repo_root/dist/anvil/bin/anvil"
	else
		anvil_bin="$(command -v anvil || true)"
	fi
fi
if [[ -z "$anvil_ctl" ]]; then
	if [[ -x "$repo_root/dist/anvil/bin/anvil-ctl" ]]; then
		anvil_ctl="$repo_root/dist/anvil/bin/anvil-ctl"
	else
		anvil_ctl="$(command -v anvil-ctl || true)"
	fi
fi
if [[ -z "$anvil_mcp" ]]; then
	if [[ -x "$repo_root/target/release/anvil-mcp" ]]; then
		anvil_mcp="$repo_root/target/release/anvil-mcp"
	else
		anvil_mcp="$(command -v anvil-mcp || true)"
	fi
fi

fail() {
	keep=1
	{
		printf '\nMCP SMOKE FAILURE: %s\n' "$1"
		printf 'root=%s\n' "$root"
		if command -v tmux >/dev/null 2>&1; then
			tmux -L "$session" capture-pane -p -t anvil -S -200 2>/dev/null || true
		fi
	} >>"$transcript"
	printf 'mcp smoke failed: %s\n' "$1" >&2
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

for command in python3 tmux; do
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
if [[ -z "$anvil_mcp" || ! -x "$anvil_mcp" ]]; then
	printf 'skip: anvil-mcp binary not found; set ANVIL_MCP\n' >&2
	keep=1
	exit 0
fi

mkdir -p "$workspace" "$runtime" "$state"
printf 'one\ntwo\n' >"$workspace/a.txt"
export XDG_RUNTIME_DIR="$runtime"
export XDG_STATE_HOME="$state"
export ANVIL_SKIP_TERMINAL_QUERY=1
export ANVIL_GATEWAY_LOG="$root/gateway.log"
export TERM="${TERM:-screen-256color}"

tmux -L "$session" new-session -d -s anvil -x 120 -y 40 \
	"cd '$workspace' && exec '$anvil_bin' -w '$workspace' a.txt"
for _ in $(seq 1 100); do
	if "$anvil_ctl" --json status >/dev/null 2>&1; then
		break
	fi
	sleep 0.1
done
if ! "$anvil_ctl" --json status >/dev/null 2>&1; then
	fail "Anvil control socket did not become ready"
fi

if ! python3 - "$anvil_mcp" >>"$transcript" 2>&1 <<'PY'
import json
import os
import selectors
import subprocess
import sys
import time

mcp = sys.argv[1]
process = subprocess.Popen(
    [mcp],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.STDOUT,
    text=True,
    bufsize=1,
    env=os.environ,
)
selector = selectors.DefaultSelector()
selector.register(process.stdout, selectors.EVENT_READ)
next_id = 1


def request(method, params=None):
    global next_id
    request_id = next_id
    next_id += 1
    message = {"jsonrpc": "2.0", "id": request_id, "method": method}
    if params is not None:
        message["params"] = params
    process.stdin.write(json.dumps(message) + "\n")
    process.stdin.flush()
    deadline = time.monotonic() + 10
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise RuntimeError(f"{method}: timed out")
        if not selector.select(remaining):
            raise RuntimeError(f"{method}: timed out")
        line = process.stdout.readline()
        if not line:
            raise RuntimeError(f"{method}: MCP exited")
        response = json.loads(line)
        if response.get("id") == request_id:
            if "error" in response:
                raise RuntimeError(f"{method}: {response['error']}")
            return response["result"]


def tool(name, arguments):
    result = request("tools/call", {"name": name, "arguments": arguments})
    if result.get("isError"):
        raise RuntimeError(f"{name}: {result['content'][0]['text']}")
    return json.loads(result["content"][0]["text"])


try:
    initialized = request("initialize", {})
    assert initialized["protocolVersion"] == "2024-11-05", initialized
    tools = request("tools/list")["tools"]
    names = {entry["name"] for entry in tools}
    assert {"anvil_status", "anvil_query", "anvil_edit", "anvil_command"} <= names, names

    status = tool("anvil_status", {})
    assert status["protocol_version"] == "3", status

    query = tool("anvil_query", {"start_line": 1, "end_line": 1})
    assert query["text"] == "one\n", query

    edited = tool("anvil_edit", {"mode": "set", "text": "alpha\nbeta\n"})
    assert edited["accepted"] is True, edited
    query = tool("anvil_query", {"start_line": 1, "end_line": 2})
    assert query["text"] == "alpha\nbeta\n", query

    undone = tool("anvil_command", {"command": ":undo"})
    assert undone["accepted"] is True, undone
    query = tool("anvil_query", {"start_line": 1, "end_line": 1})
    assert query["text"] == "one\n", query

    denied = request(
        "tools/call",
        {"name": "anvil_command", "arguments": {"command": "pkg install x"}},
    )
    assert denied["isError"] is True, denied
    assert "policy_denied" in denied["content"][0]["text"], denied

    quit_result = tool("anvil_quit", {"force": True})
    assert quit_result["accepted"] is True, quit_result
finally:
    process.stdin.close()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()
PY
then
	fail "MCP stdio transcript failed"
fi

printf 'mcp smoke passed\n'
printf 'transcript: %s\n' "$transcript"
