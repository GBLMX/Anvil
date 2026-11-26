#!/usr/bin/env bash
# Verify version, changelog, lockfile, and documentation sync before a tag.
set -euo pipefail

tag="${1:-}"
version="$(python3 - <<'PY'
import tomllib

with open("Cargo.toml", "rb") as file:
    print(tomllib.load(file)["workspace"]["package"]["version"])
PY
)"

scripts/check-frozen-version.sh

if [[ -n "$tag" && "$tag" != "v$version" ]]; then
  echo "release tag $tag does not match workspace version $version" >&2
  exit 1
fi

if ! grep -q "^## \[$version\]" CHANGELOG.md; then
  echo "CHANGELOG.md has no '## [$version]' section for $version" >&2
  exit 1
fi

cargo metadata --locked --format-version 1 >/dev/null
go mod tidy -diff
python3 scripts/docs-check.py

echo "release checklist: ok (version $version${tag:+, tag $tag})"
