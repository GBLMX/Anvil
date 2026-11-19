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

old_manifest=""
cleanup() {
  [[ -z "$old_manifest" ]] || rm -f -- "$old_manifest"
}
trap cleanup EXIT
if [[ -f "$prefix/share/anvil/install-manifest.txt" ]]; then
  old_manifest="$(mktemp)"
  cp "$prefix/share/anvil/install-manifest.txt" "$old_manifest"
fi
for schema in settings.schema.json languages.schema.json; do
  if [[ -f "$layout/share/anvil/$schema" ]]; then
    install -m 0644 "$layout/share/anvil/$schema" "$prefix/share/anvil/"
  fi
done
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

manifest="$prefix/share/anvil/install-manifest.txt"
manifest_stage="$prefix/share/anvil/.install-manifest-$$"
{
  for binary in anvil anvil-ctl anvil-mcp anvil-gateway anvil-exec-runner anvil-plugin-host; do
    printf 'bin/%s\n' "$binary"
  done
  for schema in settings.schema.json languages.schema.json; do
    if [[ -f "$prefix/share/anvil/$schema" ]]; then
      printf 'share/anvil/%s\n' "$schema"
    fi
  done
  if [[ -f "$prefix/share/applications/com.github.GBLMX.Anvil.desktop" ]]; then
    printf 'share/applications/com.github.GBLMX.Anvil.desktop\n'
  fi
  if [[ -f "$prefix/share/icons/hicolor/scalable/apps/com.github.GBLMX.Anvil.svg" ]]; then
    printf 'share/icons/hicolor/scalable/apps/com.github.GBLMX.Anvil.svg\n'
  fi
  if [[ -f "$prefix/share/icons/hicolor/256x256/apps/com.github.GBLMX.Anvil.png" ]]; then
    printf 'share/icons/hicolor/256x256/apps/com.github.GBLMX.Anvil.png\n'
  fi
  if [[ -d "$prefix/share/anvil/official/plugins" ]]; then
    find "$prefix/share/anvil/official/plugins" -type f -printf 'share/anvil/official/plugins/%P\n'
  fi
  if [[ -d "$prefix/share/anvil/packages" ]]; then
    find "$prefix/share/anvil/packages" -type f -printf 'share/anvil/packages/%P\n'
  fi
} | LC_ALL=C sort -u >"$manifest_stage"
mv "$manifest_stage" "$manifest"

if [[ -n "$old_manifest" ]]; then
  while IFS= read -r relative; do
    [[ -n "$relative" ]] || continue
    case "$relative" in
      /*|..|../*|*/..|*/../*)
        echo "refusing unsafe manifest path: $relative" >&2
        exit 2
        ;;
    esac
    if ! grep -Fxq "$relative" "$manifest"; then
      rm -f -- "$prefix/$relative"
    fi
  done <"$old_manifest"
fi

printf 'installed Anvil into %s\n' "$prefix"
