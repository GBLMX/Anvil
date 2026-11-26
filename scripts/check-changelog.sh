#!/usr/bin/env bash
# Require a CHANGELOG.md entry for user-visible code changes in pull requests.
set -euo pipefail

base="${BASE_SHA:-}"
head="${HEAD_SHA:-${GITHUB_SHA:-}}"
skip="${SKIP_CHANGELOG:-false}"

case "${skip,,}" in
  true|1)
    echo "changelog: skipped by label or PR body marker"
    exit 0
    ;;
esac

if [[ -z "$base" || -z "$head" ]]; then
  echo "changelog: no pull-request range; skipping"
  exit 0
fi

if ! git cat-file -e "${base}^{commit}" 2>/dev/null; then
  echo "changelog: base commit ${base} is not present; fetch full history" >&2
  exit 1
fi

changed="$(git diff --name-only "$base...$head")"

if grep -qx 'CHANGELOG.md' <<<"$changed"; then
  echo "changelog: CHANGELOG.md updated"
  exit 0
fi

user_visible="$(grep -E '^(crates/[^/]+/src/|go/|resources/|plugins/)' <<<"$changed" || true)"
if [[ -z "$user_visible" ]]; then
  echo "changelog: no user-visible paths changed"
  exit 0
fi

echo "changelog: user-visible paths changed without a CHANGELOG.md entry:" >&2
head -10 <<<"$user_visible" >&2
echo "Add an [Unreleased] entry, or apply the skip-changelog label or" >&2
echo "put [skip changelog] in the pull request body." >&2
exit 1
