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
model_mut_methods="$(
    python3 - "$src" <<'PY'
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
count = 0
for path in root.rglob("*.rs"):
    text = path.read_text()
    for match in re.finditer(r"(?m)^impl Model\b[^{]*\{", text):
        end = match.end()
        depth = 1
        while end < len(text) and depth:
            if text[end] == "{":
                depth += 1
            elif text[end] == "}":
                depth -= 1
            end += 1
        block = text[match.end() : end - 1]
        count += len(re.findall(r"fn\s+\w+[^\n]*\(&mut self", block))
print(count)
PY
)"
model_mut_non_model_fns="$(
    python3 - "$src" <<'PY'
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
count = 0
for path in root.rglob("*.rs"):
    text = path.read_text()
    cleaned = list(text)
    for match in re.finditer(r"(?m)^impl Model\b[^{]*\{", text):
        end = match.end()
        depth = 1
        while end < len(text) and depth:
            if text[end] == "{":
                depth += 1
            elif text[end] == "}":
                depth -= 1
            end += 1
        for index in range(match.start(), end):
            if cleaned[index] != "\n":
                cleaned[index] = " "
    count += sum(
        "&mut Model" in signature
        for signature in re.findall(r"\bfn\s+\w+\s*\([^)]*\)", "".join(cleaned))
    )
print(count)
PY
)"
model_new="$(grep -R 'Model::new' "$src" --include='*.rs' | wc -l | tr -d ' ')"
page_model_setups="$(
    python3 - "$src" <<'PY'
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
pages = [
    "settings_ui.rs",
    "settings_ui/tests.rs",
    "theme_browser.rs",
    "theme_picker.rs",
    "command_palette.rs",
]
count = 0
for page in pages:
    path = root / page
    if path.exists():
        count += path.read_text().count("Model::new")
print(count)
PY
)"
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
  model_mut_methods=$model_mut_methods
  model_mut_non_model_fns=$model_mut_non_model_fns
  model_new=$model_new
  page_model_setups=$page_model_setups
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

# File/LOC counts are informational: a page split may add files while reducing
# coupling. Gate only structural coupling and oversized-file budgets.
check_max deps "$deps" "$TUI_DEPS"
check_max impl_model "$impl_model" "$TUI_IMPL_MODEL"
check_max model_mut_methods "$model_mut_methods" "$TUI_MODEL_MUT_METHODS"
check_max model_mut_non_model_fns "$model_mut_non_model_fns" "$TUI_MODEL_MUT_NON_MODEL_FNS"
check_max page_model_setups "$page_model_setups" "$TUI_PAGE_MODEL_SETUPS"
check_max files_over_800 "$oversized" "$TUI_FILES_OVER_800"

echo "tui-metrics: ok (loc=$loc; baseline=$TUI_LOC)"
