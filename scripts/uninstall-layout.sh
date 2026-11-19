#!/usr/bin/env bash
set -euo pipefail

prefix="${1:-"$HOME/.local"}"
if [[ ! -d "$prefix" ]]; then
  echo "prefix does not exist: $prefix" >&2
  exit 2
fi
prefix="$(cd "$prefix" && pwd)"
manifest="$prefix/share/anvil/install-manifest.txt"

if [[ ! -f "$manifest" ]]; then
  echo "install manifest not found: $manifest" >&2
  echo "refusing to remove files without a manifest" >&2
  exit 2
fi

while IFS= read -r relative; do
  [[ -n "$relative" ]] || continue
  case "$relative" in
    /*|..|../*|*/..|*/../*)
      echo "refusing unsafe manifest path: $relative" >&2
      exit 2
      ;;
  esac
done <"$manifest"

while IFS= read -r relative; do
  [[ -n "$relative" ]] || continue
  rm -f -- "$prefix/$relative"
done <"$manifest"

rm -f -- "$manifest"
for directory in \
  "$prefix/share/anvil/official/plugins" \
  "$prefix/share/anvil/packages" \
  "$prefix/share/anvil/official" \
  "$prefix/share/anvil" \
  "$prefix/share/applications" \
  "$prefix/share/icons/hicolor/scalable/apps" \
  "$prefix/share/icons/hicolor/256x256/apps" \
  "$prefix/share/icons/hicolor" \
  "$prefix/share/icons" \
  "$prefix/share" \
  "$prefix/bin"; do
  rmdir --ignore-fail-on-non-empty "$directory" 2>/dev/null || true
done

printf 'uninstalled Anvil from %s\n' "$prefix"
