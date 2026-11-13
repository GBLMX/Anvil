#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work="$(mktemp -d -t anvil-provider-smoke.XXXXXX)"
session="anvil-provider-smoke-$$"
keep=0

cleanup() {
  tmux -L "$session" kill-server 2>/dev/null || true
  if [[ "$keep" == 1 ]]; then
    printf 'preserved smoke state at %s\n' "$work"
  else
    rm -rf "$work"
  fi
}
trap cleanup EXIT

bin="${ANVIL_BIN:-$root/target/release/anvil}"
ctl="${ANVIL_CTL:-$root/target/release/anvil-ctl}"
[[ -x "$bin" && -x "$ctl" ]] || {
  printf 'build anvil and anvil-ctl first\n' >&2
  exit 2
}
command -v tmux >/dev/null || {
  printf 'tmux is required\n' >&2
  exit 2
}

mkdir -p "$work/workspace" "$work/runtime" "$work/state"
printf 'hello\n' >"$work/workspace/a.txt"
export XDG_RUNTIME_DIR="$work/runtime"
export XDG_STATE_HOME="$work/state"
export ANVIL_SETTINGS="$work/settings.toml"
export ANVIL_PLUGIN_DIR="$root/tests/fixtures"
export ANVIL_PLUGIN_ALLOW_STARTUP=1
unset ANVIL_BUNDLED_PLUGIN_DIR

"$bin" --version >/dev/null
tmux -L "$session" new-session -d -s smoke -x 120 -y 40 \
  "cd '$work/workspace' && exec '$bin' -w '$work/workspace' a.txt"

status() {
  "$ctl" --workspace "$work/workspace" --json status 2>/dev/null
}

for _ in $(seq 1 100); do
  if status >/dev/null 2>&1; then
    break
  fi
  sleep 0.05
done
status >/dev/null 2>&1 || {
  keep=1
  printf 'TUI control socket did not become ready\n' >&2
  exit 1
}

tmux -L "$session" send-keys -t smoke ':provider-query fixture.completion alpha' Enter
for _ in $(seq 1 100); do
  if tmux -L "$session" capture-pane -pt smoke | grep -q 'plugin fixture: 1 provider items'; then
    break
  fi
  sleep 0.05
done
tmux -L "$session" capture-pane -pt smoke | grep -q 'plugin fixture: 1 provider items' || {
  keep=1
  printf 'completion provider did not return one item\n' >&2
  exit 1
}

tmux -L "$session" send-keys -t smoke Escape
tmux -L "$session" send-keys -t smoke ':provider-query fixture.picker alpha' Enter
for _ in $(seq 1 100); do
  if tmux -L "$session" capture-pane -pt smoke | grep -q 'Provider · Enter apply'; then
    break
  fi
  sleep 0.05
done
tmux -L "$session" capture-pane -pt smoke | grep -q 'Provider · Enter apply' || {
  keep=1
  printf 'picker provider overlay did not open\n' >&2
  exit 1
}
tmux -L "$session" send-keys -t smoke Enter
for _ in $(seq 1 100); do
  if tmux -L "$session" capture-pane -pt smoke | grep -q 'provider action applied'; then
    break
  fi
  sleep 0.05
done
tmux -L "$session" capture-pane -pt smoke | grep -q 'provider action applied' || {
  keep=1
  printf 'picker action did not reach the plugin\n' >&2
  exit 1
}

printf 'provider smoke passed\n'
