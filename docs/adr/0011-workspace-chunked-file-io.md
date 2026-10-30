# ADR-0011: Workspace-Scoped Chunked File I/O

- Status: Proposed; chunked Go `sys.read_file` implemented; TUI client, `sys.write_file`, and TUI `--workspace` are not implemented yet
- Date: 2026-10-31
- Related: [ADR-0002](0002-go-daemon.md), [ADR-0007](0007-jsonrpc-envelope-and-capability-token.md)

## Context

The TUI currently opens and saves with `std::fs` directly. That bypasses the
per-session gateway and the `fsroot` `openat2` boundary. Moving file I/O behind
the gateway requires more than routing: the gateway has no `sys.read_file` or
`sys.write_file` handler, the editor has no gateway client, and
`MAX_FRAME_LEN` is 8 MiB, so a 100 MB file cannot be read in one frame.

The editor also has no workspace root. `fsroot` accepts only paths relative to
an already opened directory with `RESOLVE_BENEATH`; it does not accept absolute
paths. Multi-buffer `:e` currently accepts arbitrary paths.

## Decision

### 1. Workspace is explicit and session-scoped

- The default workspace is the TUI's startup working directory.
- `anvil --workspace <path>` sets an explicit workspace. The path must exist and
  resolve to a directory before the gateway starts.
- All file paths in `sys.read_file` and `sys.write_file` are workspace-relative.
  Absolute paths, empty paths, NUL bytes, and any path that escapes the root are
  rejected. `..` is resolved by the gateway, not by caller string cleaning.
- This applies to `:e` and all buffers. Opening a file outside the workspace is
  refused with a status message naming the workspace, not silently clamped or
  retried through `std::fs`.
- The status line always shows the workspace basename so the boundary is visible.

The default is deliberately narrow. A user who starts in a subdirectory can
widen the workspace explicitly with `--workspace`; there is no ambient expansion
based on the first file opened.

### 2. Fail closed

- If the gateway is unavailable, misconfigured, or fails its handshake, file
  operations are disabled.
- The TUI shows a sandbox warning and refuses open, save, `:w`, `:w file`, and
  `:wq` until the gateway is healthy again.
- **There is no `std::fs` fallback for file I/O.** Keeping a direct path would
  make the sandbox claim false for the exact users who need it.
- Already loaded buffers may remain visible, but saving them fails closed.
  In-memory edits are not discarded by a gateway failure.

### 3. Transport envelope

`MAX_FRAME_LEN` remains 8 MiB. File data is base64-encoded bytes. Base64 keeps
arbitrary binary chunks inside the JSON frame and bounds worst-case escaping;
the editor assembles all chunks and rejects invalid UTF-8 before constructing a
document.

All offsets are absolute byte offsets from the start of the file. Chunks may
split a UTF-8 sequence. `next_offset` is always the offset immediately after the
decoded data in the response.

### 4. `sys.read_file`

Request:

```json
{
  "jsonrpc": "2.0",
  "id": "read-1",
  "method": "sys.read_file",
  "params": {
    "path": "src/main.rs",
    "offset": 0,
    "max_bytes": 4194304
  }
}
```

Response:

```json
{
  "jsonrpc": "2.0",
  "id": "read-1",
  "result": {
    "data": "<base64>",
    "next_offset": 4194304,
    "eof": false
  }
}
```

- One request returns at most one chunk. The client loops until `eof` is true.
- `max_bytes` is optional; default and maximum are 4 MiB of raw file bytes. The
  server may return less than requested.
- `data` is standard base64 of the returned raw bytes.
- `eof` is explicit. A short chunk is not an end signal; a file whose size is an
  exact multiple of the chunk size still requires a final request with
  `"eof": true`.
- Each request reopens the file through `fsroot` with `openat2` and
  `RESOLVE_BENEATH`. The gateway does not hold a long-lived file descriptor
  across chunks.
- An empty file returns `data: ""`, `next_offset: 0`, `eof: true`.
- A non-regular file, a path escape, an offset beyond EOF, or invalid base64 is
  rejected.
  boundary is rejected.

### 5. `sys.write_file`

Request:

```json
{
  "jsonrpc": "2.0",
  "id": "write-1",
  "method": "sys.write_file",
  "params": {
    "path": "src/main.rs",
    "offset": 0,
    "data": "<base64>"
}
}
```

Response:

```json
{
  "jsonrpc": "2.0",
  "id": "write-1",
  "result": {
    "next_offset": 4194304
  }
}
```

- `data` is standard base64 of the raw chunk bytes.
- `offset` must equal the accumulated length for that path; otherwise the
  request fails with `conflict`.
- The gateway creates a temporary file in the destination directory, writes
  chunks as they arrive, flushes and `fsync`s at `eof`, then `rename`s it over
  the destination. This preserves the existing atomic-replacement caveat: owner,
  ACLs, xattrs, and hard links are not preserved.
- The temporary file is removed on error, cancellation, timeout, or connection
  close. Stale writes are removed by the gateway when the session ends.
- Only one active write per path is allowed. A second writer fails with
  `conflict` until the first commits or aborts.
- A write session must end with `eof: true`; abandoning it does not modify the
  destination.
- `fs.write` permission is separate from `fs.read`.

### 6. Permissions

- `fs.read:<glob>` grants read access to matching workspace-relative paths.
- `fs.write:<glob>` grants write access to matching workspace-relative paths.
- `sys.read_file` requires `fs.read`; `sys.write_file` requires `fs.write`.
- The gateway checks the permission before opening the path and again after
  resolving it beneath the root. `openat2` is defense in depth, not a substitute
  for path policy.
- The exact glob syntax is intentionally not expanded in this ADR; the first
  implementation should support a small, documented subset (for example `*`
  and `**`) rather than two competing pattern languages.

### 7. Errors

| Code | Meaning |
|---|---|
| `-32602` | invalid params, invalid base64, offset beyond EOF |
| `-32002` | path policy denied |
| `-32003` | session required |
| `-32007` | file not found or not a regular file |
| `-32008` | gateway or file service unavailable |
| `-32009` | write offset conflict or concurrent writer |

### 8. Performance gate

The direct `std::fs` 100 MB open baseline is 185.9 ms (Rust side) and 193.7 ms
(PTY first output). After the chunked gateway path exists, rerun the same
fixture at 4 MiB chunks and report:

- Rust-side open + first frame.
- PTY first output.
- Per-chunk UDS round-trip latency.
- Total chunk count and total bytes.

The initial acceptable increment is **at most 50 ms** over the direct-read
baseline. A 20 ms target may be physically unrealistic with 25 round trips; do
not shrink chunking to meet a target. If the increment exceeds 50 ms, evaluate
a larger frame-safe chunk (up to 7 MiB) or shared-memory transfer before
changing the security boundary.

## Consequences

Positive:

- File access has one enforceable boundary: gateway policy plus `openat2`.
- Large files work within the existing 8 MiB frame limit.
- Writes are atomic at the destination and fail closed.
- The UI can honestly state when the sandbox is unavailable.

Negative:

- Opening a workspace-outside file becomes an explicit error.
- 100 MB reads require about 25 round trips.
- The editor and gateway both need new session/write state.
- Multi-buffer paths are constrained by one workspace per session.
