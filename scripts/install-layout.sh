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

for binary in anvil anvil-ctl anvil-mcp anvil-gateway anvil-exec-runner anvil-plugin-host; do
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
prefix="$(cd "$prefix" && pwd)"
install -m 0755 \
  "$layout/bin/anvil" \
  "$layout/bin/anvil-ctl" \
  "$layout/bin/anvil-mcp" \
  "$layout/bin/anvil-gateway" \
  "$layout/bin/anvil-exec-runner" \
  "$layout/bin/anvil-plugin-host" \
  "$prefix/bin/"

desktop_source="$layout/share/applications/com.github.GBLMX.Anvil.desktop"
if [[ -f "$desktop_source" ]]; then
  install -d \
    "$prefix/share/applications" \
    "$prefix/share/icons/hicolor/scalable/apps" \
    "$prefix/share/icons/hicolor/256x256/apps"
  desktop_stage="$prefix/share/applications/.anvil-desktop-$$"
  sed "s#@BINDIR@#$prefix/bin#g" "$desktop_source" > "$desktop_stage"
  install -m 0644 "$desktop_stage" \
    "$prefix/share/applications/com.github.GBLMX.Anvil.desktop"
  rm -f "$desktop_stage"
  install -m 0644 \
    "$layout/share/icons/hicolor/scalable/apps/com.github.GBLMX.Anvil.svg" \
    "$prefix/share/icons/hicolor/scalable/apps/"
  install -m 0644 \
    "$layout/share/icons/hicolor/256x256/apps/com.github.GBLMX.Anvil.png" \
    "$prefix/share/icons/hicolor/256x256/apps/"
  if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$prefix/share/applications" >/dev/null 2>&1 || true
  fi
  if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t "$prefix/share/icons/hicolor" >/dev/null 2>&1 || true
  fi
fi

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
