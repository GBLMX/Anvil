# File Operation Benchmarks

- Date: 2026-11-01
- CPU: 12th Gen Intel(R) Core(TM) i7-12700H
- Go: 1.26.10 linux/amd64
- Scope: direct `fsroot` mutations used by BUG-005

## Command

```bash
go test ./go/internal/fsroot -run '^$' \
  -bench 'Benchmark(CopyFile|RenameEntry|RemoveAllTree)' \
  -benchtime=1s -benchmem
```

## Results

| Benchmark | ns/op | MB/s | B/op | allocs/op |
|---|---:|---:|---:|---:|
| `CopyFile/1MiB` | 117,614 | 8,915 | 280 | 9 |
| `CopyFile/16MiB` | 3,308,848 | 5,070 | 276 | 9 |
| `RenameEntry` | 2,024 | — | 16 | 4 |
| `RemoveAllTree` | 229,218 | — | 20,623 | 627 |

`RemoveAllTree` deletes 8 directories containing 16 files each. Its fixture is
rebuilt outside the timed region for every sample.

## Interpretation

- Copy allocates a fixed small amount regardless of file size; `io.Copy` is
  streaming through the kernel copy path, not buffering the file in Go.
- The 16 MiB copy drops from about 8.9 GB/s to 5.1 GB/s, consistent with page
  cache and copy-path overhead; neither result includes `fsync`.
- Rename is metadata-only and sub-microsecond-class on this host.
- Recursive removal cost is dominated by per-entry directory/unlink syscalls;
  allocation growth tracks directory traversal, not file contents.

## Not Covered

- Gateway JSON-RPC framing and Unix-socket latency.
- The TUI `FileWorker` queue and status-spinner behavior.
- Cold-cache reads, `fsync`, cross-filesystem copies, and concurrent operations.
- Permission checks and symlink-policy checks performed by the gateway handler.

These are local measurements and evidence for the file-operation design, not
release gates. The end-to-end gateway path should be measured separately before
using these numbers for a performance claim.
