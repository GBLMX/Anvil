# Go Gateway Benchmarks

- Date: 2026-10-31
- CPU: 12th Gen Intel(R) Core(TM) i7-12700H
- Kernel: 7.3.0-4-cachyos
- Go: go1.22.3 linux/amd64
- Build: `go test -bench`, default test profile

## Command

```bash
go test -run '^$' -bench . -benchmem \
  ./go/internal/fetch ./go/internal/gateway ./go/internal/execrunner
```

## Results

| Benchmark | ns/op | B/op | allocs/op | Scope |
|---|---:|---:|---:|---|
| `BenchmarkPermissionCheck` | 11.93 | 0 | 0 | direct permission lookup |
| `BenchmarkFrameRoundTrip` | 176.0 | 184 | 5 | in-memory frame encode/decode |
| `BenchmarkHello` | 1796 | 920 | 19 | token decode + hash + constant-time compare |
| `BenchmarkFetchHTTP` | 186,551 | 34,925 | 149 | local HTTP server, 4 KiB body, new transport per request |
| `BenchmarkRunTrue` | 386,622 | 68,268 | 66 | process-group spawn/wait for `/usr/bin/true` |

These are local measurements, not CI gates. `BenchmarkHello` calls the handler
directly, not through a Unix socket; the frame and UDS correctness paths are
covered by tests. `BenchmarkFetchHTTP` deliberately disables connection reuse
through the current per-request transport design.

## Interpretation

- Gateway policy lookup is far below the 1 ms p99 target.
- Frame encoding is sub-microsecond and allocation-light.
- Token verification is dominated by JSON/base64/SHA-256 and stays below 2 µs on
  this host.
- Fetch latency is dominated by transport setup and the local HTTP request path;
  keep-alive reuse is not part of the current policy client.
- Process spawn is roughly 0.34 ms before sandbox setup and tool runtime.
