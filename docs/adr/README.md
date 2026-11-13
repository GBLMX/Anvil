# ADR Index

Status: canonical index for architecture decision records. Accepted ADRs are
records; a later decision must supersede them rather than silently rewriting
their decision sections.

| ADR | Decision | Status |
|---|---|---|
| [0001](0001-plugin-isolation.md) | Untrusted plugins run in a separate host process | Accepted |
| [0002](0002-go-daemon.md) | Go gateway is a per-session policy gateway | Accepted |
| [0003](0003-idl.md) | Lightweight IDL for the ten-method API | Accepted for v1 |
| [0004](0004-text-model.md) | Text model selected by benchmark | Superseded by ADR-0009 |
| [0005](0005-exec-policy.md) | No shell/pipeline; per-tool argument policy | Accepted; gateway integration follow-up |
| [0006](0006-upstream-dependencies.md) | Upstream dependency baseline | Accepted for Phase 0 |
| [0007](0007-jsonrpc-envelope-and-capability-token.md) | JSON-RPC envelope and capability token | Proposed |
| [0008](0008-selection-first-interaction.md) | Selection-first interaction model | Accepted |
| [0009](0009-text-model-snapshot-gate.md) | Text model snapshot gate and backend | Accepted |
| [0010](0010-cursor-and-snapshot-ownership.md) | Core owns coordinates; editor owns snapshots | Accepted |
| [0011](0011-workspace-chunked-file-io.md) | Workspace-scoped chunked file I/O | Proposed |
| [0012](0012-plugin-host-isolation.md) | Plugin-host isolation | Proposed |
| [0013](0013-kernel-distribution-boundary.md) | Rust kernel and distribution boundary | Proposed |
| [0014](0014-plugin-api-v1-freeze.md) | Plugin API v1 freeze | Accepted |
| [0014b](0014-plugin-wit-seams.md) | Plugin WIT contract from editor seams | Proposed; number collision, renumber before acceptance |
| [0015](0015-command-line-completion.md) | Command-line completion and popup | Proposed |
| [0016](0016-render-style-layers.md) | Render style layers | Accepted |
| [0017](0017-plugin-activation-and-lifecycle.md) | Plugin activation and lifecycle | Accepted |
| [0018](0018-workspace-tree-watch.md) | Workspace tree watching | Accepted |
| [0019](0019-terminal-keyboard-enhancement.md) | Terminal keyboard enhancement | Accepted |
| [0020](0020-content-addressed-plugin-store.md) | Content-addressed plugin store | Accepted |
| [0021](0021-signed-plugin-index.md) | Signed plugin index, revocations, anti-rollback | Accepted; verifier/cache partial |
| [0022](0022-core-editing-completion.md) | Core editing completion and semantic ownership | Proposed |
| [0023](0023-internal-capability-closure.md) | Internal capability closure before public distribution | Proposed |
| [0024](0024-tui-state-and-domain-boundaries.md) | TUI state composition, page state, and domain boundaries | Proposed |
| [0025](0025-lsp-and-language-breadth.md) | LSP surface and language breadth boundaries | Proposed |
| [0026](0026-persistent-undo-decision.md) | Persistent undo is deferred and gated | Accepted |
| [0027](0027-distribution-composition-crate.md) | Standard distribution composition crate and two consumers | Accepted |
| [0028](0028-module-contract.md) | Trusted source-level module contract | Accepted |
| [0029](0029-git-module-extension-points.md) | Git module extension points | Accepted for implementation |
| [0030](0030-workspace-index-module.md) | Workspace index module extension points | Accepted for implementation |
| [0031](0031-desktop-url-opener-module.md) | Desktop URL opener module | Proposed |

## Numbering Rule

- One immutable number per accepted decision.
- The `0014b` WIT ADR is a known collision: it was written while the v1 freeze
  ADR took `0014`. It remains proposed. Renumber it to the next free ADR number
  when accepted or superseded.
- A new ADR must link the ADRs it supersedes or depends on.
