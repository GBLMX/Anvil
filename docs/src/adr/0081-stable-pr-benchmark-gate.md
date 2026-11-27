# ADR-0081: Stable PR Text Benchmark Gate

- Status: Accepted; implemented 2026-11-27
- Version: 1.0.0
- Date: 2026-11-27
- Related: [ADR-0009](0009-text-model-snapshot-gate.md),
  [ADR-0010](0010-cursor-and-snapshot-ownership.md),
  [ADR-0077](0077-benchmark-only-text-backends.md),
  [ADR-0080](0080-release-cadence-and-synchronization.md)
- Supersedes: PR text p99 gate clauses in ADR-0010 and ADR-0080
- Superseded by:

## Context

ADR-0080 and ADR-0010 require the pull-request benchmark job to reject text
p99 regressions above 10%. That gate is not measurable on the shared GitHub
runner used by the job: the measured code is identical between `origin/main`
and the pull request, yet repeated runs reported p99 ratios from 1.153x to
1.825x on `rope`/`gap` first-edit and random-edit workloads. Local repeated
runs of the same harness stayed within 5% at p50.

Raising the p99 threshold or ignoring failures would hide real regressions.
Removing tail reporting would lose the product metric. The gate needs a
statistic that a shared runner can measure repeatedly while p95/p99 remain
available for controlled runs.

## Decision

### 1. Gate PRs on repeated-run p50

The PR benchmark job compares p50, not p99:

- run the PR and main harness three times each;
- take the minimum p50 per workload across those runs;
- fail only when the ratio exceeds 1.10x **and** the absolute increase exceeds
  100 µs.

The absolute floor keeps sub-100 µs microbenchmark noise from failing a PR.

### 2. Keep p95/p99 in the benchmark output

`text-p99` continues to emit p50, p95, and p99 for every workload. PR CI
records the CSV but gates on p50. Tail latency remains a product and release
concern.

### 3. Keep authoritative tail measurements off PR CI

Pinned manual or scheduled runs on a controlled host remain the authority for
p99 and 100 MB behavior. They are not required on every PR. A release claim
that changes text performance must cite those measurements.

### 4. Do not weaken product targets

This ADR changes only the PR regression statistic. It does not change the
10 MB/100 MB p99 targets, the editor frame targets, or the release gates that
use controlled measurements.

## Consequences

Positive:

- The PR gate reflects code changes instead of runner preemption.
- p95/p99 remain visible and usable by release/performance work.
- The 10% regression policy still applies to the median and to absolute deltas.

Negative:

- A tail-only regression can pass PR CI until a controlled run detects it.
- Two benchmark interpretations now exist: fast PR median checks and
  authoritative tail measurements.

## Verification

- `.github/workflows/ci.yml` parses p50 from `text-p99` CSV and still reports
  p50/p95/p99.
- PR #43 reruns demonstrated code-identical p99 failures up to 1.825x; local
  p50 ratios stayed within 0.96x-1.05x.
- PR #46 passed the p50 gate before merge.
- PR #43 passed all checks after the rebase onto the p50 gate.

## References

- [ADR-0080](0080-release-cadence-and-synchronization.md)
- [ADR-0010](0010-cursor-and-snapshot-ownership.md)
- [Text model benchmarks](../benchmarks/text-model.md)
