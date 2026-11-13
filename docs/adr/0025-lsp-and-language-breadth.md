# ADR-0025: LSP Surface and Language Breadth

- Status: Proposed
- Date: 2026-11-12
- Related: [ADR-0008](0008-selection-first-interaction.md), [ADR-0010](0010-cursor-and-snapshot-ownership.md), [ADR-0013](0013-kernel-distribution-boundary.md), [ADR-0022](0022-core-editing-completion.md), [ADR-0023](0023-internal-capability-closure.md), [ADR-0024](0024-tui-state-and-domain-boundaries.md)

## Context

LSP supplies most of Anvil's semantic IDE behavior. The current TUI already
handles completion and resolve, semantic tokens, definition, references,
document highlights, call hierarchy, folding ranges, hover, rename, code
actions, formatting, document symbols, sticky symbols, signature help,
workspace symbols, inlay hints, command execution, and code lens.

That surface is broad enough that "LSP support" is no longer a useful claim by
itself. Two separate boundaries need an explicit decision:

- which LSP requests Anvil advertises and tests;
- which languages Anvil highlights without runtime configuration.

The current implementation has real gaps. It has no
`textDocument/declaration`, `textDocument/typeDefinition`, or
`textDocument/implementation` request, so it cannot offer Helix-style
declaration/type-definition/implementation navigation. Language highlighting is
a compile-time Tree-sitter allowlist, not a runtime grammar registry. Files over
256 KiB intentionally fall back to plain text. LSP is also primary-root-only;
multi-root routing exists, but per-root LSP does not.

These are product boundaries, not missing modules. Calling Anvil "Helix-level
LSP" or "multi-language" without stating them would overstate the editor.

## Decision

### 1. Treat the advertised LSP surface as an explicit list

The supported request kinds are:

- completion and completion resolve;
- semantic tokens, inlay hints, and code lens;
- definition, references, document highlights, and call hierarchy;
- hover, signature help, rename, and code actions;
- formatting, document symbols, workspace symbols, and sticky symbols;
- folding ranges and `workspace/executeCommand`.

Code lens is part of this surface. It includes request/parse, bounded
virtual-row rendering, command execution, mouse targeting, and stale-response
handling.

The following are explicitly **not advertised** until implemented:

- `textDocument/declaration`;
- `textDocument/typeDefinition`;
- `textDocument/implementation`;
- document links, document colors, inline values, and linked editing ranges.

Anvil does not claim Helix parity for those requests. Adding one is a bounded
LSP slice, not a documentation change.

### 2. Every LSP addition must preserve the same invariants

A new request is closed only when it has:

- an `LspRequestKind` route and a server capability check;
- canonicalized UTF-16 positions on request and response;
- request-time document revision checks and stale-response rejection;
- a bounded parser for server data;
- an explicit empty, unsupported, and error state;
- tests for success, cancellation, stale revision, and malformed response;
- no second event loop and no blocking call on the TUI thread.

Edits returned by LSP go through the shared `WorkspaceEdit` engine. It checks
document versions and request-time revisions, applies create/rename/delete
operations, keeps unopened files as background buffers, and does not write
files during the edit. LSP results must not write files directly.

LSP remains a consumer of editor semantics, not a second owner. Navigation and
selection stay in the kernel; the TUI renders and routes input. ADR-0008 and
ADR-0010 still apply.

### 3. Language breadth is a curated Tree-sitter allowlist

Syntax highlighting is compiled into `anvil-tui` through explicit
`tree-sitter-*` dependencies and queries. The current families are Rust, Go,
Bash/Zsh/Brush, Fish, Python, TOML, YAML, JSON/JSONC, Markdown, Makefile,
Dockerfile, and `go.mod`.

Adding a language requires:

- an explicit Cargo dependency and query file;
- a language id/path mapping;
- highlighter tests for the new family;
- a binary-size and startup review;
- no execution of grammar-provided code at runtime.

Runtime grammar loading, downloaded grammars, and plugin-provided grammars are
out of scope. They require a separate ADR covering distribution, versioning,
sandboxing, and failure behavior.

Unsupported languages degrade to plain text. They must not receive guessed or
partial highlighting.

### 4. Keep the large-file threshold explicit

Tree-sitter highlighting stops above `256 KiB` (`MAX_HIGHLIGHT_BYTES`). The
buffer still edits as plain text; highlighting is not silently partial.

Raising the threshold requires a latency and memory measurement on the
reference machine. It is not changed merely to match another editor's
behavior.

### 5. Language servers are trusted external tools

Anvil does not bundle language servers. They are resolved from `PATH` and run
without the plugin sandbox. This is a deliberate trust boundary: an LSP server
has the same trust as the user's compiler or language tooling, not the same
trust as an untrusted plugin.

The current server list is a supported integration set, not a guarantee that
every server version works. Server configuration, installation, and upgrades
remain the user's responsibility. Plugin-provided language servers are not part
of this decision.

LSP is primary-root-only for now. Multi-root roots can be opened and saved
through their owning gateway, but the language layer does not fan out per root.
Per-root LSP requires its own ADR and measurements.

### 6. Helix remains a capability reference, not a target

Helix is useful for identifying omissions such as the missing goto variants.
It is not a keymap or behavior target. Selection-first semantics and the
Vim-like TUI adapter remain unchanged.

## Consequences

Positive:

- Documentation and UI can state the exact LSP surface without implying
  unimplemented navigation.
- Language breadth has a reviewable cost model instead of an open-ended
  "supports everything" claim.
- Large-file and multi-root behavior remain predictable.
- New LSP requests reuse the existing revision, UTF-16, bound, and
  `WorkspaceEdit` machinery.

Negative:

- Anvil will look narrower than Helix on declaration/type-definition/
  implementation navigation until those slices land.
- Adding a language is a source change and a binary-size decision, not a user
  configuration change.
- Large files intentionally lose syntax highlighting.
- Multi-root workspaces do not get per-root language servers.

## Alternatives considered

### Add every Helix LSP key and call it parity (rejected)

The request kinds are not the whole capability. Missing routing, bounds, stale
checks, and failure states would produce a surface that looks complete but is
not reliable.

### Load Tree-sitter grammars at runtime (rejected for now)

Dynamic grammars add ABI, version, distribution, and sandbox questions. They
need a separate ADR and a failure model.

### Run language servers inside the plugin sandbox (rejected)

Language servers need normal filesystem, process, and toolchain access. Placing
them in the untrusted-plugin sandbox would either break real servers or weaken
the plugin boundary. They remain trusted external tools.

### Add per-root LSP while adding multi-root routing (rejected)

Per-root servers multiply processes, memory, and trust policy. ADR-0023 keeps
LSP primary-root-only until the routing and measurement work exists.

### Raise the highlighting limit without measurement (rejected)

The 256 KiB ceiling is a latency guard. Changing it without reference-machine
numbers would trade predictable editing for unverified highlighting.

## Verification expectations

- LSP request tests cover every advertised kind's success, empty, error, and
  stale-revision paths.
- Position tests cover UTF-16 conversion at astral-plane and combining
  characters.
- Workspace-edit tests cover version mismatch, create/rename/delete, and
  unopened background buffers.
- Syntax tests cover every allowlisted language and assert unsupported paths
  stay plain text.
- Large-file tests assert `MAX_HIGHLIGHT_BYTES` behavior.
- Multi-root tests assert that LSP stays on the primary root and that the UI
  does not claim per-root language support.
- `cargo test --workspace --no-fail-fast`, clippy, fmt, architecture checks,
  and a TUI smoke test pass for each slice.

## Implementation status (2026-11-13)

- The advertised LSP surface above is implemented, including code lens.
- Goto variants are complete: definition (`gd`), declaration (`gD`), type definition (`gy`), and implementation (`gI`).
- Tree-sitter is a compile-time allowlist of the twelve families listed above.
- Highlighting stops above 256 KiB.
- LSP is primary-root-only.
- Language servers are external trusted tools and are not bundled.
