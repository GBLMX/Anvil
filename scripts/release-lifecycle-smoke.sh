#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
layout="${1:-"$root/dist/anvil"}"
if [[ ! -x "$layout/install.sh" ]]; then
  echo "usage: $0 <release-layout>" >&2
  exit 2
fi
if [[ ! -x "$layout/uninstall.sh" ]]; then
  echo "release layout is missing uninstall.sh: $layout" >&2
  exit 2
fi

tmp="$(mktemp -d)"
cleanup() {
  rm -rf -- "$tmp"
}
trap cleanup EXIT

prefix="$tmp/prefix"
export XDG_DATA_HOME="$tmp/data"
export XDG_CONFIG_HOME="$tmp/config"
export XDG_STATE_HOME="$tmp/state"
export LANG=C.UTF-8

"$layout/install.sh" "$prefix"
for binary in anvil anvil-ctl anvil-mcp anvil-gateway anvil-exec-runner anvil-plugin-host; do
  [[ -x "$prefix/bin/$binary" ]] || {
    echo "install did not provide bin/$binary" >&2
    exit 1
  }
done
[[ -f "$prefix/share/anvil/install-manifest.txt" ]] || {
  echo "install did not write an install manifest" >&2
  exit 1
}
grep -Fxq 'bin/anvil' "$prefix/share/anvil/install-manifest.txt"
grep -q '^share/anvil/official/plugins/' "$prefix/share/anvil/install-manifest.txt"
"$prefix/bin/anvil" --version >/dev/null

doctor="$tmp/doctor.json"
"$prefix/bin/anvil" doctor --profile installed --json >"$doctor"
python3 - "$doctor" <<'PY'
import json
import sys

report = json.load(open(sys.argv[1], encoding="utf-8"))
assert report["schema_version"] == 1, report
assert report["profile"] == "installed", report
assert report["ok"] is True, report
PY

mkdir -p "$XDG_CONFIG_HOME/anvil" "$XDG_STATE_HOME/anvil"
printf 'keep\n' >"$XDG_CONFIG_HOME/anvil/smoke.toml"
printf 'keep\n' >"$XDG_STATE_HOME/anvil/smoke.state"

upgraded="$tmp/upgraded-layout"
cp -a "$layout" "$upgraded"
printf '\n' >>"$upgraded/share/anvil/settings.schema.json"
old_schema_hash="$(sha256sum "$prefix/share/anvil/settings.schema.json" | awk '{print $1}')"
stale="$prefix/share/anvil/old-layout-file.txt"
printf 'stale\n' >"$stale"
printf 'share/anvil/old-layout-file.txt\n' >>"$prefix/share/anvil/install-manifest.txt"
"$upgraded/install.sh" "$prefix"
new_schema_hash="$(sha256sum "$prefix/share/anvil/settings.schema.json" | awk '{print $1}')"
[[ "$old_schema_hash" != "$new_schema_hash" ]] || {
  echo "upgrade did not replace the settings schema" >&2
  exit 1
}
[[ ! -e "$stale" ]] || {
  echo "upgrade did not remove a stale manifest entry" >&2
  exit 1
}
"$prefix/bin/anvil" doctor --profile installed --json >/dev/null

"$upgraded/uninstall.sh" "$prefix"
for binary in anvil anvil-ctl anvil-mcp anvil-gateway anvil-exec-runner anvil-plugin-host; do
  [[ ! -e "$prefix/bin/$binary" ]] || {
    echo "uninstall left bin/$binary" >&2
    exit 1
  }
done
[[ ! -e "$prefix/share/anvil/install-manifest.txt" ]] || {
  echo "uninstall left the install manifest" >&2
  exit 1
}
[[ -f "$XDG_CONFIG_HOME/anvil/smoke.toml" ]] || {
  echo "uninstall removed config state" >&2
  exit 1
}
[[ -f "$XDG_STATE_HOME/anvil/smoke.state" ]] || {
  echo "uninstall removed runtime state" >&2
  exit 1
}

printf 'release lifecycle smoke: install, upgrade, uninstall, doctor ok\n'
