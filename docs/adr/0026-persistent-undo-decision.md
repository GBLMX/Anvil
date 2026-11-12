# ADR-0026: Persistent Undo Decision

- Status: Accepted
- Date: 2026-11-13
- Related: [ADR-0009](0009-text-model-snapshot-gate.md), [ADR-0022](0022-core-editing-completion.md), [ADR-0023](0023-internal-capability-closure.md)

## Context

Anvil keeps undo/redo in memory. Session restore deliberately restores the
exact buffer or swap text and starts a fresh undo stack. That preserves user
text, but it does not preserve undo history across restarts.

Persistent undo is not a small serialization feature. The current edit records
are offset-based and are valid only against one exact text state. Restoring a
different text state, merging an external change, or replaying records against
a reconstructed buffer can silently undo unrelated text. The swap-file and
crash-recovery paths already carry user data; adding a second persisted history
beside them creates new corruption and write-ordering risks.

ADR-0023 therefore rejects persistent undo for the internal-polish phase. This
ADR records that decision and the conditions for revisiting it, so the absence
is explicit rather than an undocumented gap.

## Decision

### 1. Do not persist undo history in the current phase

The following remain the supported behavior:

- session restore keeps the exact buffer or swap text;
- restore starts a fresh undo stack;
- `:w`, `:wa`, atomic saves, swap files, and crash recovery are unchanged;
- no hidden edit journal is written beside the document;
- no undo state is stored in session metadata, the swap file, or the plugin
  store.

Persistent undo is out of scope for `0.6.x`.

### 2. Re-entry requires a new ADR and all of these gates

A future implementation must satisfy every gate below before it can replace
this decision:

**Text-state identity**

- Undo records must be anchored to a verifiable text state, not merely to byte
  offsets from an earlier revision.
- Restore must detect path moves, external edits, and content changes; a stale
  history must be discarded as a whole, not partially replayed.
- The mapping from restored text to undo records must be tested at grapheme,
  line-ending, and snapshot boundaries.

**Persistence format**

- A versioned schema with explicit forward/backward compatibility behavior.
- Checksums or equivalent corruption detection.
- Bounded per-buffer and global history size, with an explicit eviction policy.
- No unbounded growth from repeated sessions or large edits.

**Crash consistency**

- Atomic history writes, or an append format with a verified recovery point.
- A truncated or partially written history must never block opening the
  document or restoring the buffer.
- Failure to write history must not make the document save fail.
- Save, swap, and history ordering must be documented and tested.

**File identity and privacy**

- History files must be scoped to the owning workspace root and document.
- Permissions must avoid exposing document text to other users.
- Removing a workspace root, closing a document, or discarding a conflict must
  have a defined cleanup rule.
- History must not be written for unnamed, stdin, read-only, missing, or
  conflicted buffers unless a later decision explicitly allows it.

**UI semantics**

- It must be clear whether undo after restore applies to the restored text or
  only to edits made after restore.
- Dirty state, save points, and external-change conflicts must not be inferred
  from stale undo state.
- The behavior after `:w`, `:wa`, `:q`, crash recovery, and conflict recovery
  must be documented before implementation.

**Verification**

- Tests for crash, truncation, checksum failure, schema mismatch, path moves,
  external edits, permission failure, large buffers, and concurrent sessions.
- A destructive-failure test proving that corrupt history never discards the
  document or marks it clean.
- Reference-machine measurements for history size, load time, and save latency.

### 3. Do not substitute a partial implementation

The following are not acceptable as an interim solution:

- persisting offset-only edit records;
- silently replaying a history against a changed file;
- storing full text snapshots without a size and retention policy;
- making undo persistence mandatory for restore;
- treating swap files as undo history.

## Consequences

Positive:

- The current restore path stays simple and data-loss focused.
- No second persisted copy of document text is introduced without privacy,
  cleanup, and corruption rules.
- The product does not claim undo persistence it cannot guarantee.

Negative:

- Users lose undo history after restart or crash recovery.
- The editor remains behind editors that persist undo.
- A future implementation carries a substantial schema, storage, and migration
  cost.

## Alternatives considered

### Persist offset-based records now (rejected)

Offsets are valid only against one exact text state. Restore, external edits,
and line-ending changes make them unsafe.

### Persist full text snapshots per edit (rejected)

This multiplies storage and leaks more document text to disk. It also needs
retention and privacy rules before it can be considered.

### Replay edits onto a restored buffer (rejected)

Replay can apply an edit to the wrong range after any external change or path
move. There is no safe merge rule in the current model.

### Keep undo only when the document is unchanged since the last session (rejected for now)

This narrows the problem but still needs format, identity, cleanup, and
corruption handling. A narrow special case is not a substitute for the gates
above.

### Disable restore to preserve undo semantics (rejected)

Restore and crash recovery protect user text. Giving them up to avoid a fresh
undo stack is a worse trade.

## Implementation status (2026-11-13)

- Undo/redo is in memory only.
- Session restore and crash recovery restart the undo stack.
- No undo journal, undo file, or undo entry exists in session metadata.
- The decision is recorded in `ROADMAP.md`; revisiting it requires a new ADR
  that supersedes this one.
