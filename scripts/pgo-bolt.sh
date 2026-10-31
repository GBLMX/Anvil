#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

MODE="${1:-pgo}"
shift || true

PGO_DIR="${PGO_DIR:-$ROOT/target/pgo}"
BOLT_DIR="${BOLT_DIR:-$ROOT/target/bolt}"
TARGET="${TARGET:-}"
WORKLOAD_SCRIPT="$ROOT/scripts/pgo-workload.py"
WORKLOAD_WORKSPACE="${WORKLOAD_WORKSPACE:-$ROOT/target/pgo-workload}"
BASE_RUSTFLAGS="${BASE_RUSTFLAGS:-}"

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

need_tool() {
    command -v "$1" >/dev/null 2>&1 || die "missing $1; install the LLVM tools and perf first"
}

target_args() {
    if [[ -n "$TARGET" ]]; then
        printf '%s\n' "--target" "$TARGET"
    fi
}

binary_path() {
    local dir="$1"
    if [[ -n "$TARGET" ]]; then
        printf '%s/%s/release/anvil\n' "$dir" "$TARGET"
    else
        printf '%s/release/anvil\n' "$dir"
    fi
}

cargo_build() {
    local dir="$1"
    local flags="$2"
    mapfile -t target < <(target_args)
    CARGO_TARGET_DIR="$dir" RUSTFLAGS="${BASE_RUSTFLAGS} ${flags}" \
        cargo build --release "${target[@]}" -p anvil-tui
}

run_workload() {
    local binary="$1"
    python3 "$WORKLOAD_SCRIPT" "$binary" "$WORKLOAD_WORKSPACE"
}

find_profdata() {
    local host sysroot
    host="$(rustc -vV | sed -n 's/^host: //p')"
    sysroot="$(rustc --print sysroot)"
    if [[ -n "$host" && -x "$sysroot/lib/rustlib/$host/bin/llvm-profdata" ]]; then
        printf '%s\n' "$sysroot/lib/rustlib/$host/bin/llvm-profdata"
        return
    fi
    command -v llvm-profdata || die "missing llvm-profdata; run: rustup component add llvm-tools"
}

pgo_build() {
    local profdata
    profdata="$(find_profdata)"
    rm -rf "$PGO_DIR/raw" "$PGO_DIR/build"
    mkdir -p "$PGO_DIR/raw"

    local instrumented_flags="-Cprofile-generate=$PGO_DIR/raw -Ccodegen-units=1"
    cargo_build "$PGO_DIR/build" "$instrumented_flags"
    local instrumented
    instrumented="$(binary_path "$PGO_DIR/build")"
    [[ -f "$instrumented" ]] || die "instrumented binary not found: $instrumented"

    LLVM_PROFILE_FILE="$PGO_DIR/raw/anvil-%p-%m.profraw" run_workload "$instrumented"
    local profiles=("$PGO_DIR"/raw/*.profraw)
    [[ -f "${profiles[0]}" ]] || die "no PGO profiles collected in $PGO_DIR/raw"

    "$profdata" merge -o "$PGO_DIR/merged.profdata" "${profiles[@]}"
    cargo_build "$PGO_DIR/build" "-Cprofile-use=$PGO_DIR/merged.profdata -Ccodegen-units=1"
    cp "$(binary_path "$PGO_DIR/build")" "$PGO_DIR/anvil-pgo"
    printf 'PGO binary: %s\n' "$PGO_DIR/anvil-pgo"
}

find_llvm_tool() {
    local tool="$1"
    if command -v "$tool" >/dev/null 2>&1; then
        command -v "$tool"
        return
    fi
    if command -v llvm-config >/dev/null 2>&1; then
        local bindir
        bindir="$(llvm-config --bindir 2>/dev/null || true)"
        if [[ -n "$bindir" && -x "$bindir/$tool" ]]; then
            printf '%s\n' "$bindir/$tool"
            return
        fi
    fi
    die "missing $tool; install llvm-bolt for the system LLVM toolchain"
}

bolt_build() {
    need_tool perf
    local perf2bolt bolt
    perf2bolt="$(find_llvm_tool perf2bolt)"
    bolt="$(find_llvm_tool llvm-bolt)"
    local source_bin="${BOLT_BIN:-}"
    local profile_flags=""
    if [[ -n "$source_bin" ]]; then
        source_bin="$(realpath "$source_bin")"
    else
        if [[ -n "${PGO_PROFILE_USE:-}" ]]; then
            profile_flags="-Cprofile-use=$(realpath "$PGO_PROFILE_USE") -Ccodegen-units=1"
        fi
        cargo_build "$BOLT_DIR/build" "-C link-arg=-Wl,--emit-relocs $profile_flags"
        source_bin="$(binary_path "$BOLT_DIR/build")"
    fi
    [[ -f "$source_bin" ]] || die "BOLT source binary not found: $source_bin"

    local perf_data="$BOLT_DIR/perf.data"
    local fdata="$BOLT_DIR/perf.fdata"
    local output="$BOLT_DIR/anvil-bolt"
    mkdir -p "$BOLT_DIR"
    rm -f "$perf_data" "$fdata" "$output"

    if ! perf record -e cycles:u -j any,u -o "$perf_data" -- \
        python3 "$WORKLOAD_SCRIPT" "$source_bin" "$WORKLOAD_WORKSPACE"; then
        die "perf record failed; check perf_event_paranoid or run on the target host"
    fi

    "$perf2bolt" -strict=0 -p "$perf_data" -o "$fdata" "$source_bin"
    "$bolt" "$source_bin" \
        -data="$fdata" \
        -o="$output" \
        -reorder-blocks=ext-tsp \
        -reorder-functions=hfsort \
        -split-functions \
        -icf \
        -strict=0
    printf 'BOLT binary: %s\n' "$output"
}

case "$MODE" in
    pgo)
        pgo_build
        ;;
    bolt)
        bolt_build
        ;;
    all)
        pgo_build
        PGO_PROFILE_USE="$PGO_DIR/merged.profdata" bolt_build
        ;;
    *)
        printf 'usage: %s {pgo|bolt|all}\n' "$0" >&2
        exit 2
        ;;
esac
