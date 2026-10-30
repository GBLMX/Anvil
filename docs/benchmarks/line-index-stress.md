# Line Index Stress

- Date: 2026-10-31
- Input: 100 MB synthetic, 80-byte lines
- Seed: 42
- Edit mix: 70% single-character insert, 20% delete, 10% newline insert

## 10k run

```text
cargo test -p anvil-editor --test line_index_stress -- --nocapture
```

All samples from edit 0 through 9,000 used **1,281 chunks**. No chunk growth
occurred. Final document: 1,310,692 lines.

```text
chunk-curve,0,1281
...
chunk-curve,9000,1281
final-chunks,1281,1310692
offset-p99-ns,2218
```

## 100k run

```text
cargo test --release -p anvil-editor --test line_index_stress -- --ignored --nocapture
```

The release run completed in 51.6 s. The sampled 100k run completed with **1,281 chunks**. Deviation: the full
line-start round-trip check runs at every 10,000 edits for the 100k test; the
per-1,000 check covers `len_lines`, chunk count, and chunk bounds. This is
documented rather than claiming a full 1k cadence for 100k.

## Interpretation

The 512/2048 occupancy normalization keeps chunk count bounded at the initial
1,281 chunks under the deterministic random workload. The stress test is a
manual/nightly 100k run; PR CI runs the 10k variant as part of `cargo test`.
