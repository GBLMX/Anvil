# Recovery and save semantics

This page is also available in the editor through `:help recover`.

## What is durable

- Named buffers are written with temporary-file + `fsync` + rename.
- Session/tab state and crash recovery are bounded to the session format.
- Dirty-buffer swaps can survive a source conflict and can be recovered.

## What is intentionally not durable

- The undo stack is reset after session restore. Persistent undo is not
  supported in this phase; do not rely on undo across restarts.
- The atomic rename replaces the inode. Owner, group, ACLs, xattrs, and hard
  links are not preserved. Check the target permissions before saving when
  those properties matter.
- Project replace previews and applies files one at a time. A failed write
  rolls back in reverse order, but cross-file atomicity is not guaranteed.

## External changes

- A clean open buffer reloads from disk.
- A dirty open buffer prompts `File changed on disk: <path> [y/N]`; it never
  discards local edits without confirmation.
- `:conflict-recover` loads a conflicted swap into the current buffer.
- `:conflict-discard` discards the swap and keeps the current buffer.
- External deletion marks the buffer missing and closes its LSP document.

## Safe sequence

1. Save or inspect dirty buffers before switching branches or running
   formatters/builds.
2. If an external-change prompt appears, confirm only after comparing the
   buffer with disk.
3. After a crash or restore, use `:conflict-recover` before overwriting a
   suspicious buffer.
4. Do not publish a save that relies on owner/ACL/xattr/hard-link continuity
   without validating the resulting inode.
