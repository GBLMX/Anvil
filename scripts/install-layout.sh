#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ -d "$script_dir/share/anvil" ]]; then
  layout="$script_dir"
  prefix="${1:-"$HOME/.local"}"
else
  [[ $# -ge 1 ]] || {
    echo "usage: $0 <layout-dir> [prefix]" >&2
    exit 2
  }
  layout="$(cd "$1" && pwd)"
  shift
  prefix="${1:-"$HOME/.local"}"
fi

for binary in anvil anvil-gateway anvil-exec-runner anvil-plugin-host; do
  [[ -x "$layout/bin/$binary" ]] || {
    echo "layout is missing bin/$binary" >&2
    exit 2
  }
done

official_src="$layout/share/anvil/official/plugins"
if [[ ! -d "$official_src" ]]; then
  official_src="$layout/share/anvil/plugins"
fi
[[ -d "$official_src" ]] || {
  echo "layout is missing official plugins" >&2
  exit 2
}

install -d "$prefix/bin" "$prefix/share/anvil/official"
install -m 0755 \
  "$layout/bin/anvil" \
  "$layout/bin/anvil-gateway" \
  "$layout/bin/anvil-exec-runner" \
  "$layout/bin/anvil-plugin-host" \
  "$prefix/bin/"

plugins_stage="$prefix/share/anvil/official/.plugins-$$"
rm -rf "$plugins_stage"
mkdir -p "$plugins_stage"
cp -R "$official_src/." "$plugins_stage/"
rm -rf "$prefix/share/anvil/official/plugins"
mv "$plugins_stage" "$prefix/share/anvil/official/plugins"

if [[ -d "$layout/share/anvil/packages" ]]; then
  packages_stage="$prefix/share/anvil/.packages-$$"
  rm -rf "$packages_stage"
  mkdir -p "$packages_stage"
  cp -R "$layout/share/anvil/packages/." "$packages_stage/"
  rm -rf "$prefix/share/anvil/packages"
  mv "$packages_stage" "$prefix/share/anvil/packages"
fi

if compgen -G "$prefix/share/anvil/packages/*.anvil-plugin" >/dev/null; then
  "$prefix/bin/anvil-gateway" \
    -install-bundled "$prefix/share/anvil/packages"
fi

printf 'installed Anvil into %s\n' "$prefix"
