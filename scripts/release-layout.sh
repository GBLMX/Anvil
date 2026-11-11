#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="${1:-"$root/dist/anvil"}"
go_toolchain="${GOTOOLCHAIN:-go1.26.10}"

cd "$root"

install -d "$out/bin" "$out/share/anvil/official/plugins"

cargo build --release -p anvil-tui -p anvil-ctl -p anvil-plugin-host
GOTOOLCHAIN="$go_toolchain" go build -o "$out/bin/anvil-gateway" ./go/cmd/anvil-gateway
GOTOOLCHAIN="$go_toolchain" go build -o "$out/bin/anvil-exec-runner" ./go/cmd/anvil-exec-runner

install -m 0755 \
  target/release/anvil \
  target/release/anvil-ctl \
  target/release/anvil-plugin-host \
  "$out/bin/"
cp -R plugins/official/. "$out/share/anvil/official/plugins/"
install -m 0755 scripts/install-layout.sh "$out/install.sh"

if [[ -n "${ANVIL_PLUGIN_SIGNING_KEY:-}" ]]; then
  for plugin in "$root"/plugins/official/*; do
    [[ -d "$plugin" ]] || continue
    scripts/package-official-plugin.sh \
      "$plugin" \
      "$ANVIL_PLUGIN_SIGNING_KEY" \
      "$out/share/anvil/packages"
  done
fi

printf 'release layout: %s\n' "$out"
