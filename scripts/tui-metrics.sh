#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

src="crates/anvil-tui/src"
baseline="docs/tui-maintainability.baseline"
check=false
if [ "${1:-}" = "--check" ]; then
    check=true
fi

files="$(find "$src" -name '*.rs' | wc -l | tr -d ' ')"
loc="$(find "$src" -name '*.rs' -print0 | xargs -0 cat | wc -l | tr -d ' ')"
impl_model_files="$(grep -Rl '^impl Model' "$src" --include='*.rs' | wc -l | tr -d ' ')"
impl_model="$(grep -R '^impl Model' "$src" --include='*.rs' | wc -l | tr -d ' ')"
mut_methods="$(grep -R -E 'fn [A-Za-z0-9_]+.*\(&mut self' "$src" --include='*.rs' | wc -l | tr -d ' ')"
model_new="$(grep -R 'Model::new' "$src" --include='*.rs' | wc -l | tr -d ' ')"
oversized="$(find "$src" -name '*.rs' -print0 | xargs -0 wc -l | awk '$1 > 800 && $2 != "total" { count++ } END { print count + 0 }')"
deps="$(
    cargo metadata --no-deps --format-version 1 \
        | python3 -c 'import json,sys; print(len(next(p for p in json.load(sys.stdin)["packages"] if p["name"] == "anvil-tui")["dependencies"]))'
)"

cat <<EOF
tui-metrics:
  files=$files
  loc=$loc
  deps=$deps
  impl_model=$impl_model
  impl_model_files=$impl_model_files
  mut_self_methods=$mut_methods
  model_new=$model_new
  files_over_800=$oversized
EOF

if [ "$check" = false ]; then
    exit 0
fi

# shellcheck disable=SC1090
source "$baseline"

check_max() {
    local name="$1"
    local actual="$2"
    local expected="$3"
    if [ "$actual" -gt "$expected" ]; then
        echo "tui-metrics: $name grew: baseline $expected, found $actual" >&2
        exit 1
    fi
}

check_max files "$files" "$TUI_FILES"
check_max deps "$deps" "$TUI_DEPS"
check_max impl_model "$impl_model" "$TUI_IMPL_MODEL"
check_max impl_model_files "$impl_model_files" "$TUI_IMPL_MODEL_FILES"
check_max mut_self_methods "$mut_methods" "$TUI_MUT_SELF_METHODS"
check_max model_new "$model_new" "$TUI_MODEL_NEW"
check_max files_over_800 "$oversized" "$TUI_FILES_OVER_800"

echo "tui-metrics: ok (loc=$loc; baseline=$TUI_LOC)"
