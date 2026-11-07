#!/usr/bin/env bash
set -euo pipefail

expected="0.6.0"
actual="$(python3 - <<'PY'
import tomllib
with open('Cargo.toml', 'rb') as file:
    print(tomllib.load(file)['workspace']['package']['version'])
PY
)"

if [[ "$actual" != "$expected" ]]; then
  echo "workspace version is $actual; internal polish is frozen at $expected" >&2
  echo "update scripts/check-frozen-version.sh and docs/internal-polish-plan.md" >&2
  echo "in the same commit as the intentional version bump" >&2
  exit 1
fi
