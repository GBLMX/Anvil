# ADR-0004: Text Model Is Selected by Benchmark, Rope Is the Default Candidate

- Status: Superseded by ADR-0009 for backend selection; benchmark method remains
- Date: 2026-10-30

## Context

Anvil targets 100 MB files with editing and scrolling, not only reading. The
original whitepaper selected Gap Buffer first and made Rope a later option.

Gap Buffer is simple and fast near the gap, but insertion/deletion far from the
gap moves the tail of the buffer. At 100 MB, random edits can move tens of
megabytes per keystroke. Snapshots, undo, and rendering can multiply that cost.

Ferriso already has a `GapBuffer` and a segmented line index, but its
`Document::open` default limit is 64 MiB. Reuse is possible, but it must be
retested at Anvil's target size.

Ropey is explicitly designed for large texts and memory-incoherent edits. A
piece table is also viable; the choice is not "Gap Buffer or nothing".

## Decision

1. The editor depends on a `TextBuffer` trait, not a concrete storage type.
2. Phase 0 benchmarks Gap Buffer, Rope, and a piece table on identical workloads.
3. **Rope or piece table is the default production candidate.** Gap Buffer ships
   only if it meets the random-edit gates below.
4. Only one backend ships in v1. A second backend is not a parallel maintenance
   path.
5. The snapshot contains `Arc<Document>` plus revision and viewport; it must not
   copy the full text per frame.
6. Rendering reads only visible lines. Syntax highlighting is incremental and
   viewport-scoped.
7. Ferriso's gap buffer and line index are reference implementations, not an
   automatic dependency.

## Current harness

`crates/anvil-core` defines the `TextBuffer` trait and three candidates:
`RopeBuffer`, `GapBuffer`, and `PieceTable`. The initial CLI benchmark runs:

```text
cargo run --release -p anvil-core --bin text-bench -- 1 10 100
```

It measures construction, first viewport extraction, and repeated edits at
1/50/99% offsets. It is not yet the final decision harness: p99 sampling,
undo/redo, snapshots, memory accounting, and the one-line 100 MB case remain
open gates.

## Required measurements

For 1 MB, 10 MB, and 100 MB:

- open + line index + first viewport;
- insert/delete at 1%, 50%, and 99% offsets;
- undo/redo;
- line lookup and viewport extraction;
- one-line 100 MB file;
- CRLF/BOM/invalid UTF-8 behavior;
- byte/char/grapheme conversions;
- peak RSS;
- snapshot cost when the document is shared.

Report p50/p95/p99 and max. A benchmark that only measures sequential append is
not sufficient.

## Gates

The chosen backend must:

- keep p99 edit latency within the Phase 0 performance budget;
- avoid a full-document copy per snapshot;
- render a 100 MB viewport at the documented frame budget;
- survive the Unicode/long-line corpus without hangs;
- keep undo memory bounded.

If Gap Buffer fails, replace it. Do not add a second backend "for small files"
unless a later ADR proves the maintenance cost is worth it.

## Consequences

Positive:

- Performance is selected by evidence.
- The trait boundary allows a later change without rewriting the UI.
- Rope/piece table handles random edits better.

Negative:

- Rope char/byte conversions and line indexing are more complex.
- The Phase 0 benchmark is on the critical path.
- Ferriso's existing gap-buffer code cannot simply be copied without revalidation.

## Alternatives considered

### Gap Buffer only (rejected as default)

Good for small/medium files and local edits; high risk for 100 MB random edits.

### Rope only without benchmark

Likely the right choice, but the acceptance criteria still need measured evidence
and a comparison against the actual editor workload.

### Dual backend

Rejected for v1: two implementations of cursor movement, undo, snapshots,
search, and line indexing is a maintenance trap.

## References

- [Ropey](https://docs.rs/ropey/latest/ropey/)
- Ferriso `crates/ferriso-editor/src/document.rs` and `gap.rs`
