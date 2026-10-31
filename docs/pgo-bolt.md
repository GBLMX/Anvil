# PGO and BOLT

This is an opt-in release workflow. It does not change the default
`cargo build --release` output or CI.

## Prerequisites

- Rust 1.88+ and the LLVM tools matching the Rust compiler for PGO
  (`llvm-profdata`). The script prefers the toolchain's copy; install it with
  `rustup component add llvm-tools` if `llvm-profdata` is not found.
- `perf`, `perf2bolt`, and `llvm-bolt` for BOLT. On Arch/CachyOS:
  `pacman -S llvm-bolt`.
- Permission to collect user-space performance events. Use `perf_event_paranoid`
  or a dedicated profile host; do not weaken it in CI.

The training workload is `scripts/pgo-workload.py`. It launches the built
binary in a PTY, edits a generated workspace, opens a second buffer, drives the
vertical split, searches, and quits. It is intentionally short; replace or
extend it when the target workload is known.

## PGO

```sh
./scripts/pgo-bolt.sh pgo
```

Artifacts:

```text
target/pgo/raw/*.profraw
target/pgo/merged.profdata
target/pgo/anvil-pgo
```

The instrumented build and the optimized build use separate
`CARGO_TARGET_DIR`s so profile files are never mixed with normal release
artifacts.

## BOLT

```sh
./scripts/pgo-bolt.sh bolt
```

This builds with `--emit-relocs`, records a `cycles:u` sample, converts it with
`perf2bolt`, and writes:

```text
target/bolt/perf.data
target/bolt/perf.fdata
target/bolt/anvil-bolt
```

BOLT is post-link optimization. The script passes `-strict=0` because Rust PIE
binaries can retain relocations that BOLT cannot classify; review BOLT warnings
before shipping. Keep the unstripped input binary; stripping is a separate
packaging step.

## PGO then BOLT

```sh
./scripts/pgo-bolt.sh all
```

The PGO profile is also passed to the BOLT build so both optimizers train on the
same workload. The default synthetic workload is short: in local validation BOLT
profiled only about 3% of functions and warned that it wanted hundreds of times
more samples. Use a representative workload before drawing conclusions.

## Measurement

Do not claim a speedup from the existence of a profile. Compare the normal
release binary, `anvil-pgo`, and `anvil-bolt` with the same workload, input
files, CPU governor, and page-cache state. The repository's benchmark harnesses
are under `crates/*/benches`; `docs/performance.md` defines the latency gates.
