# Line Index Stress

- Date: 2026-10-31
- Input: 100 MB synthetic, 80-byte lines
- Seed: 42
- Edit mix: 70% single-character insert, 20% delete, 10% newline insert

## 10k run

```text
cargo test -p anvil-editor --test line_index_stress -- --nocapture
```

All samples from edit 0 through 10,000 used **1,280 chunks**. No chunk growth
occurred. Final document: 1,311,971 lines.

```text
chunk-curve,0,1280
...
chunk-curve,9000,1280
final-chunks,1280,1311971
offset-p99-ns,2017
```

## 100k run

```text
cargo test --release -p anvil-editor --test line_index_stress -- --ignored --nocapture
```

The release run completed in 61.4 s. Samples from 0 through 99,000 remained at
**1,280 chunks**; every sampled chunk invariant check passed.

## Interpretation

The 512/2048 occupancy normalization keeps chunk count bounded at the initial
1,280 chunks under the deterministic random workload. The stress test is a
manual/nightly 100k run; PR CI runs the 10k variant as part of `cargo test`.
