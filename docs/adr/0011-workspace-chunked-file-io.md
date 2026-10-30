# ADR-0011: Workspace-Scoped Chunked File I/O

- Status: Proposed; Go `sys.read_file`/`sys.write_file`, `anvil-ipc::GatewayClient`, TUI gateway wiring, and TUI `--workspace` are implemented
- Date: 2026-10-31
- Related: [ADR-0002](0002-go-daemon.md), [ADR-0007](0007-jsonrpc-envelope-and-capability-token.md)

## Context

The TUI uses the per-session gateway for open/save and refuses file operations
when the gateway cannot start. The gateway and `anvil-ipc::GatewayClient`
implement chunked `sys.read_file`/`sys.write_file`; `MAX_FRAME_LEN` is 8 MiB,
so a 100 MB file is transferred in 5.5 MiB chunks.

The TUI workspace root defaults to the startup directory and can be set with
`--workspace`. `fsroot` accepts only paths relative to the opened directory with
`RESOLVE_BENEATH`; absolute paths and outside paths are rejected for every
buffer.

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
    "max_bytes": 5767168
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
    "next_offset": 5767168,
    "eof": false
  }
}
```

- One request returns at most one chunk. The client loops until `eof` is true.
- `max_bytes` is optional; default and maximum are 5.5 MiB of raw file bytes.
  5.5 MiB encodes to about 7.3 MiB of base64 and fits the 8 MiB frame with
  JSON overhead; the theoretical ceiling is about 5.9 MiB. The server may
  return less than requested.
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
    "data": "<base64>",
    "eof": false
  }
}
```

The final chunk uses `"eof": true`; its offset must equal the accumulated
length, and its `data` may be empty when the last non-final chunk already
contained the end of the file:

```json
{
  "path": "src/main.rs",
  "offset": 5767168,
  "data": "<base64>",
  "eof": true
}
```

Response:

```json
{
  "jsonrpc": "2.0",
  "id": "write-1",
  "result": {
    "next_offset": 5767168
  }
}
```

- `data` is standard base64 of the raw chunk bytes.
- `offset` must equal the accumulated length for that path; otherwise the
  request fails with `conflict`.
- The gateway streams chunks into a temporary file in the destination
  directory. It does **not** accumulate the whole document in memory: a 100 MB
  write would otherwise add a 100 MB gateway allocation on top of the chunk
  buffers.
- There is no per-chunk `fsync`. The gateway flushes and `fsync`s once at
  `eof`, then `rename`s the temporary file over the destination. This preserves
  the existing atomic-replacement caveat: owner, ACLs, xattrs, and hard links
  are not preserved.
- The destination parent is opened with `openat2` and `RESOLVE_BENEATH`; the
  temporary file and final rename both use that directory fd. The final name is
  a single path component. Rename target validation is not skipped: a target
  parent that escapes the workspace is rejected before the temporary file is
  created. Rename replaces the target entry itself and never follows a target
  symlink.
- The temporary file is removed on error, cancellation, timeout, or connection
  close. Stale writes are removed when the connection ends.
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

The direct `std::fs` 100 MB open baseline is **96.1 ms** (Rust side) after
two-pass parallel line scanning and **209.6 ms** (PTY first output, measured
before parallel indexing). Use this baseline after
`71ea604`; the older 185.9 ms / 193.7 ms numbers were measured before the
line-index copy fix and must not be used to attribute gateway overhead. After
the chunked gateway path exists, rerun the same fixture at 5.5 MiB chunks and
report:

- Rust-side open + first frame.
- PTY first output.
- Per-chunk UDS round-trip latency.
- Total chunk count and total bytes.

Measured 2026-10-31 on the same 100 MB fixture: gateway-ready 3.38 ms,
19 chunks, 104,857,596 bytes, 27.7 ms average per chunk, 637.4 ms total UDS
read loop, and **563.1 ms PTY first output** after parallel indexing. That is
**+353 ms** over the older direct PTY baseline; the gate **fails**. The 4 MiB run was 732.0 ms, so 5.5
MiB is a real but insufficient improvement; parallel indexing cut Rust open from
174 ms to 96 ms and gateway PTY from 640 ms to 563 ms. A 7 MiB raw chunk cannot fit after
base64; the practical ceiling is about 5.9 MiB. Server-side stage benchmarks
show per chunk: read **0.84 ms (~6.5 GB/s)**, base64 encode **4.98 ms**,
JSON encode **5.33 ms**, full response encode **11.8 ms**; the file is
page-cache warm. Client-side per chunk: JSON decode **1.17 ms**, base64 decode
**2.09 ms**, append **0.31 ms**, full decode **5.88 ms**, UDS frame read
**1.46 ms**. Pipelining can overlap round trips, but it cannot remove the
encode CPU. Binary framing removes about 10.3 ms server + 3.3 ms client per
chunk (~258 ms total), projecting to roughly **305 ms PTY**, not 240 ms.
fd passing removes the whole transfer path and is the only option that can
approach the direct baseline. Pipelining was measured separately on the real
gateway: 1 in-flight 591.8 ms, 2 372.0 ms, 4 389.0 ms, 8 **335.3 ms**, 19
376.6 ms. Within the same run, window 8 saves **256 ms** against window 1 and
projects to roughly **307 ms PTY**; it is not a 28 ms socket-only optimization.
The worker-count check (16/8/4) showed gateway PTY 563.1/560.7/597.3 ms, so
CPU contention does not explain a 30–50 ms gap; 16 remains the default.

The single 50 ms gate was too coarse for a 100 MB document. The Phase 0.6 gates
are:

- **Open 100 MB PTY < 200 ms.** Current gateway is 563 ms; direct Rust open is
  96 ms. Parallel line-index construction is implemented; binary framing and
  pipelining are still needed. Not met.
- **Daily edit key-to-screen < 1 ms.** Current Rust-side is ~0.08 ms and local
  PTY is ~0.16–0.6 ms. Met.
- **50 ms open remains a long-term target**, not a Phase 0.6 blocker; the
  line-index construction is now parallel but still part of the direct baseline.

Do not reduce chunk size to chase a latency number, and do not raise the frame
limit without re-evaluating the memory and parsing boundary.

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
