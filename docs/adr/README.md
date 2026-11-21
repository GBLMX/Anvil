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
| [0005](0005-exec-policy.md) | No shell/pipeline; per-tool argument policy | Accepted; implemented |
| [0006](0006-upstream-dependencies.md) | Upstream dependency baseline | Accepted for Phase 0 |
| [0007](0007-jsonrpc-envelope-and-capability-token.md) | JSON-RPC envelope and capability token | Accepted; implemented |
| [0008](0008-selection-first-interaction.md) | Selection-first interaction model | Accepted |
| [0009](0009-text-model-snapshot-gate.md) | Text model snapshot gate and backend | Accepted |
| [0010](0010-cursor-and-snapshot-ownership.md) | Core owns coordinates; editor owns snapshots | Accepted |
| [0011](0011-workspace-chunked-file-io.md) | Workspace-scoped chunked file I/O | Accepted; implemented |
| [0012](0012-plugin-host-isolation.md) | Plugin-host isolation | Accepted; implemented |
| [0013](0013-kernel-distribution-boundary.md) | Rust kernel and distribution boundary | Accepted; baseline in ADR-0037 |
| [0014](0014-plugin-api-v1-freeze.md) | Plugin API v1 freeze | Accepted |
| [0015](0015-command-line-completion.md) | Command-line completion and popup | Phase 1/2 implemented; Phase 3 plugin completion pending |
| [0016](0016-render-style-layers.md) | Render style layers | Accepted |
| [0017](0017-plugin-activation-and-lifecycle.md) | Plugin activation and lifecycle | Accepted |
| [0018](0018-workspace-tree-watch.md) | Workspace tree watching | Accepted |
| [0019](0019-terminal-keyboard-enhancement.md) | Terminal keyboard enhancement | Accepted |
| [0020](0020-content-addressed-plugin-store.md) | Content-addressed plugin store | Accepted |
| [0021](0021-signed-plugin-index.md) | Signed plugin index, revocations, anti-rollback | Accepted; verifier/cache partial |
| [0022](0022-core-editing-completion.md) | Core editing completion and semantic ownership | Accepted; implemented |
| [0023](0023-internal-capability-closure.md) | Internal capability closure before public distribution | Accepted; closure audit ongoing |
| [0024](0024-tui-state-and-domain-boundaries.md) | TUI state composition, page state, and domain boundaries | Partially implemented; primitives split in ADR-0041 |
| [0025](0025-lsp-and-language-breadth.md) | LSP surface and language breadth boundaries | Accepted; primary-root LSP clause superseded by ADR-0048 |
| [0026](0026-persistent-undo-decision.md) | Persistent undo is deferred and gated | Accepted |
| [0027](0027-distribution-composition-crate.md) | Standard distribution composition crate and two consumers | Accepted |
| [0028](0028-module-contract.md) | Trusted source-level module contract | Accepted |
| [0029](0029-git-module-extension-points.md) | Git module extension points | Accepted for implementation |
| [0030](0030-workspace-index-module.md) | Workspace index module extension points | Accepted for implementation |
| [0031](0031-desktop-url-opener-module.md) | Desktop URL opener module | Accepted for implementation |
| [0032](0032-language-tooling-service.md) | Language tooling service slot | Accepted for implementation |
| [0033](0033-latex-language-provider.md) | LaTeX language provider | Accepted for implementation |
| [0034](0034-release-signing-authority.md) | Hardware-backed public-release signing; offline YubiKey flow | Proposed; public-release direction selected |
| [0035](0035-plugin-wit-seams.md) | Plugin WIT contract from editor seams | Proposed |
| [0036](0036-typst-language-provider.md) | Typst language provider and feature-gated grammar | Accepted for implementation |
| [0037](0037-kernel-api-baseline.md) | Kernel API baseline during internal polish | Accepted |
| [0038](0038-project-settings-trust.md) | Project settings are trust-gated | Accepted |
| [0039](0039-plugin-event-transport.md) | Plugin event transport and interaction boundary | Proposed |
| [0040](0040-helix-keymap-compatibility.md) | Helix-compatible keymap preset and shared command contexts | Accepted |
| [0041](0041-tui-primitive-extraction.md) | TUI primitive extraction | Accepted |
| [0042](0042-selection-first-session-frontend.md) | Selection-first core, headless session, thin frontend | Accepted |
| [0043](0043-plugin-api-1-1.md) | Plugin API 1.1 capability and selection contract | Proposed |
| [0044](0044-watcher-transport.md) | Keep bounded polling watcher; record inotify comparison | Superseded by ADR-0046 |
| [0045](0045-settings-patch-layers.md) | Sparse settings layers, provenance, and a metadata registry | Accepted; implemented 2026-11-16 |
| [0046](0046-full-workspace-watch.md) | Full workspace watch coverage through the gateway | Accepted; implemented 2026-11-17; supersedes ADR-0044 transport decision |
| [0047](0047-file-tree-navigation-and-refresh.md) | File tree navigation and incremental refresh | Accepted; implemented |
| [0048](0048-per-root-language-tooling-and-lsp-workspace.md) | Per-root language tooling and LSP workspace configuration | Accepted; implemented 2026-11-16 |
| [0049](0049-editor-control-plane.md) | Editor control plane and MCP adapter boundary | Accepted; implemented 2026-11-17 |
| [0050](0050-discovery-first-language-packs.md) | Discovery-first defaults and compiled language packs | Accepted; implemented 2026-11-19 |
| [0051](0051-external-editor-reference-models.md) | External editor reference models for UX and language support | Proposed |
| [0052](0052-language-tool-setup-and-source-resolution.md) | Explicit language tool setup and per-tool source resolution | Accepted; implemented native providers 2026-11-19; managed downloads deferred |
| [0053](0053-settings-scopes-and-language-overlays.md) | Explicit settings scopes, language overlays, and registry completeness | Accepted; implemented 2026-11-19 |
| [0054](0054-file-picker-query-and-index-lifecycle.md) | File picker query, ranking, and index lifecycle | Accepted; implemented 2026-11-19 |
| [0055](0055-start-dashboard-actions-and-context.md) | Start dashboard actions, focus, and contextual recents | Accepted; implemented 2026-11-19 |
| [0056](0056-mcp-dual-era-protocol-and-tool-contract.md) | MCP dual-era protocol and structured tool contract | Accepted; implemented 2026-11-20 |
| [0057](0057-tui-surface-model-and-focus-contract.md) | TUI surface model and focus contract | Accepted; core implemented 2026-11-20 |
| [0058](0058-sandbox-capability-profiles-and-trust-boundaries.md) | Sandbox capability profiles and trust boundaries | Accepted; implemented 2026-11-20 |
| [0059](0059-public-docs-boundary-and-deferred-navigation.md) | Public docs boundary, accessibility gates, and deferred navigation/search | Accepted; docs entry and checks implemented 2026-11-20 |
| [0060](0060-release-checksum-signing-authority.md) | Release checksum signing for internal candidates and the public authority boundary | Accepted; internal candidate policy 2026-11-20 |
| [0061](0061-workspace-navigation-and-root-boundary.md) | Distinguish recent session workspaces from current-session roots | Accepted; process handoff implemented 2026-11-20 |
| [0062](0062-workspace-process-handoff.md) | Switch session workspaces by replacing the process after session save | Accepted; implemented 2026-11-20 |
| [0063](0063-command-architecture-and-execution-boundary.md) | Stable command identities, shared capability policy, and bounded effects | Accepted; core implemented |
| [0064](0064-multi-root-routing-and-ownership.md) | Route file, tree, search, and Git work by owning root | Accepted; implemented |
| [0065](0065-provider-protocols-and-execution-classes.md) | UI-neutral providers with data-only, one-shot, LSP, and DAP execution classes | Accepted; implemented |
| [0066](0066-bounded-background-task-lifecycle.md) | Bounded task ownership, cancellation, and deterministic shutdown | Accepted; implemented |
| [0067](0067-settings-file-hot-reload.md) | Poll settings and language files, preserve last-good config, defer while Settings is open | Accepted; implemented 2026-11-20 |

## Numbering Rule

- One immutable number per accepted decision.
- A new ADR must link the ADRs it supersedes or depends on.

## External References Rule

- If an ADR's analysis used external documentation, specifications, source
  code, benchmarks, or other web material, the ADR must contain a
  `## References` section.
- Record the exact source with a full `https://` URL, not only a title, bare
  domain, search result, or local mirror.
- State briefly what each source supports when the connection is not obvious.
- Local Anvil documents may be cited alongside external references, but they do
  not replace the external URL for an external claim.
- Omit the section only when no external material informed the decision.

## Versioning Rule

ADR identity and ADR revision are separate:

- The file name (`0042-selection-first-session-frontend.md`) and number
  (`ADR-0042`) are immutable.
- New and revised ADRs carry `- Version: x.y.z`.
- ADRs without a `Version` field are legacy entries at `1.0.0`.
- A new ADR starts at `1.0.0`.

This policy applies from 2026-11-15. Earlier revisions remain `1.0.0` until
their next substantive update.

Bump rules:

| Change | Version |
|---|---|
| Initial accepted decision | `1.0.0` |
| Implementation/status/boundary update without changing the decision | `1.1.0` |
| Wording, links, or formatting | `1.1.1` |
| In-place re-acceptance of the same decision | `2.0.0`; discouraged |

Supersession never bumps the old ADR's major version. The old ADR keeps its
`1.x.y` version and becomes `Superseded by ADR-NNNN`; the replacement ADR
starts at `1.0.0`.

Version axes are independent:

| Axis | Example | Meaning |
|---|---|---|
| Product version | `0.6.0` | workspace/Cargo version |
| Plan slice | `0.6.0.1` | internal planning label, not semver |
| ADR version | `1.0.0` | one ADR document/decision revision |

`0.6.0.x` labels are not releases and not semver tags. Do not treat a four-part
planning label as a Cargo or ADR version.

## Template

- [ADR template](../templates/adr.md)
