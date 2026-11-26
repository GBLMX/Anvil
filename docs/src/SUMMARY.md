# Summary

- [Documentation index](index.md)

# Getting started

- [Installing Anvil](INSTALL.md)
- [Getting started](onboarding.md)
- [Keymap Configuration](keymap.md)
- [Roadmap](ROADMAP.md)
- [Release Process](RELEASE.md)

# Architecture and reference

- [Anvil Architecture](architecture.md)
- [Anvil Threat Model](threat-model.md)
- [Deployment Model](deployment.md)
- [API Surface and Compatibility](api-surface.md)
- [Modules, Compatibility, and Deprecation Policy](modules.md)
- [Settings sources](settings-sources.md)
- [Anvil Performance Baseline and PGO/BOLT](performance.md)
- [Known Issues and Resolved Bugs](known-issues.md)
- [`anvil doctor`](doctor.md)
- [Recovery and save semantics](recovery.md)
- [Multi-host install and remote workflow](multi-host-install.md)
- [Local CI with act and Docker](local-ci.md)
- [TUI Design System](tui-design.md)
- [TUI Maintainability Budget](tui-maintainability.md)

# SPECs

- [Anvil Plugin API v1](spec/plugin-api-v1.md)
- [Anvil Plugin API](spec/plugin-api.md)

# Current plans and contracts

- [Package Store and Installed State](package-store.md)
- [External Integration](external-integration.md)

# Active plans

- [Architecture Modernization Plan](plans/active/architecture-modernization-plan.md)
- [High-Frequency Command Architecture Plan](plans/active/command-architecture-plan.md)
- [IDE Platform Plan](plans/active/ide-platform-plan.md)
- [Linux Platform Plan](plans/active/linux-platform-plan.md)
- [Task Lifecycle Plan](plans/active/task-lifecycle-plan.md)

# Proposed plans

- [AI Telemetry HUD Plan](plans/proposed/ai-telemetry-plan.md)
- [Analysis Backlog](plans/proposed/analysis-backlog.md)
- [Lua Plugin Tactical Interaction Plan](plans/proposed/plugin-mouse-canvas-plan.md)
- [Small Features Plan](plans/proposed/small-features-plan.md)

# Benchmarks

- [Go Gateway Benchmarks](benchmarks/go-gateway.md)
- [File Operation Benchmarks](benchmarks/file-operations.md)
- [Text Model Benchmark: Phase 0.5 Baseline](benchmarks/text-model.md)
- [Editor Benchmarks](benchmarks/editor.md)
- [Line Index Stress](benchmarks/line-index-stress.md)
- [TUI edit-loop latency](benchmarks/tui.md)
- [Reference Performance Baseline — 2026-11-20](benchmarks/reference-2026-11-20-local-i7.md)
- [Historical Reference Baseline — 2026-11-14](benchmarks/reference-2026-11-14-local-i7.md)
- [Local Lifecycle Re-run — 2026-11-14](benchmarks/local-2026-11-14-lifecycle.md)
- [Local Performance Baseline — 2026-11-13](benchmarks/local-2026-11-13.md)

# Architecture decision records

- [ADR Index](adr/index.md)

- [ADR-0001: Untrusted Plugins Run in a Separate Host Process](adr/0001-plugin-isolation.md)
- [ADR-0002: Go Gateway Is a Per-Session Policy Gateway](adr/0002-go-daemon.md)
- [ADR-0003: Lightweight IDL for the Ten-Method API](adr/0003-idl.md)
- [ADR-0004: Text Model Is Selected by Benchmark](adr/0004-text-model.md)
- [ADR-0005: No Shell, No Pipeline, Per-Tool Argument Policy](adr/0005-exec-policy.md)
- [ADR-0006: Upstream Dependency Baseline](adr/0006-upstream-dependencies.md)
- [ADR-0007: JSON-RPC Envelope and Capability Token Semantics](adr/0007-jsonrpc-envelope-and-capability-token.md)
- [ADR-0008: Selection-First Native Interaction Model](adr/0008-selection-first-interaction.md)
- [ADR-0009: Text Model Snapshot Gate and Production Backend](adr/0009-text-model-snapshot-gate.md)
- [ADR-0010: Core Owns Coordinates; the Editor Layer Owns Snapshots](adr/0010-cursor-and-snapshot-ownership.md)
- [ADR-0011: Workspace-Scoped Chunked File I/O](adr/0011-workspace-chunked-file-io.md)
- [ADR-0012: Plugin-Host Isolation](adr/0012-plugin-host-isolation.md)
- [ADR-0013: Rust Kernel and Distribution Boundary](adr/0013-kernel-distribution-boundary.md)
- [ADR-0014: Plugin API v1 Freeze](adr/0014-plugin-api-v1-freeze.md)
- [ADR-0015: Command-Line Completion and Popup](adr/0015-command-line-completion.md)
- [ADR-0016: Render Style Layers](adr/0016-render-style-layers.md)
- [ADR-0017: Plugin Activation, Concurrent Execution, and Resource Isolation](adr/0017-plugin-activation-and-lifecycle.md)
- [ADR-0018: Workspace Tree Watching](adr/0018-workspace-tree-watch.md)
- [ADR-0019: Terminal Keyboard Enhancement and Key Dispatch](adr/0019-terminal-keyboard-enhancement.md)
- [ADR-0020: Content-Addressed Plugin Store and Installed State](adr/0020-content-addressed-plugin-store.md)
- [ADR-0021: Signed Plugin Index, Revocations, and Anti-Rollback](adr/0021-signed-plugin-index.md)
- [ADR-0022: Core Editing Completion and Semantic Ownership](adr/0022-core-editing-completion.md)
- [ADR-0023: Internal Capability Closure Before Public Distribution](adr/0023-internal-capability-closure.md)
- [ADR-0024: TUI State Composition and Domain Boundaries](adr/0024-tui-state-and-domain-boundaries.md)
- [ADR-0025: LSP Surface and Language Breadth](adr/0025-lsp-and-language-breadth.md)
- [ADR-0026: Persistent Undo Decision](adr/0026-persistent-undo-decision.md)
- [ADR-0027: Distribution Composition Crate and Two Consumers](adr/0027-distribution-composition-crate.md)
- [ADR-0028: Trusted Source-Level Module Contract](adr/0028-module-contract.md)
- [ADR-0029: Git Module Extension Points](adr/0029-git-module-extension-points.md)
- [ADR-0030: Workspace Index Module Extension Points](adr/0030-workspace-index-module.md)
- [ADR-0031: Desktop URL Opener Module](adr/0031-desktop-url-opener-module.md)
- [ADR-0032: Language Tooling Service Slot](adr/0032-language-tooling-service.md)
- [ADR-0033: LaTeX Language Provider](adr/0033-latex-language-provider.md)
- [ADR-0034: Release and Plugin Signing Authority](adr/0034-release-signing-authority.md)
- [ADR-0035: Plugin WIT Contract from Exercised Editor Seams](adr/0035-plugin-wit-seams.md)
- [ADR-0036: Typst Language Provider](adr/0036-typst-language-provider.md)
- [ADR-0037: Kernel API Baseline During Internal Polish](adr/0037-kernel-api-baseline.md)
- [ADR-0038: Project Settings Are Trust-Gated](adr/0038-project-settings-trust.md)
- [ADR-0039: Plugin Event Transport and Interaction Boundary](adr/0039-plugin-event-transport.md)
- [ADR-0040: Helix-Compatible Keymap Preset and Command Contexts](adr/0040-helix-keymap-compatibility.md)
- [ADR-0041: TUI Primitive Extraction](adr/0041-tui-primitive-extraction.md)
- [ADR-0042: Selection-First Core, Headless Session, Thin Frontend](adr/0042-selection-first-session-frontend.md)
- [ADR-0043: Plugin API 1.1 Capability and Selection Contract](adr/0043-plugin-api-1-1.md)
- [ADR-0044: Keep the bounded polling watcher for 0.6.x](adr/0044-watcher-transport.md)
- [ADR-0045: Sparse Settings Layers, Provenance, and a Metadata Registry](adr/0045-settings-patch-layers.md)
- [ADR-0046: Full Workspace Watch Coverage Through the Gateway](adr/0046-full-workspace-watch.md)
- [ADR-0047: File tree navigation and incremental refresh](adr/0047-file-tree-navigation-and-refresh.md)
- [ADR-0048: Per-root language tooling and LSP workspace configuration](adr/0048-per-root-language-tooling-and-lsp-workspace.md)
- [ADR-0049: Editor Control Plane and MCP Adapter Boundary](adr/0049-editor-control-plane.md)
- [ADR-0050: Discovery-First Defaults and Compiled Language Packs](adr/0050-discovery-first-language-packs.md)
- [ADR-0051: External Editor Reference Models for UX and Language Support](adr/0051-external-editor-reference-models.md)
- [ADR-0052: Explicit Language Tool Setup and Source Resolution](adr/0052-language-tool-setup-and-source-resolution.md)
- [ADR-0053: Explicit Settings Scopes, Language Overlays, and Registry Completeness](adr/0053-settings-scopes-and-language-overlays.md)
- [ADR-0054: File Picker Query, Ranking, and Index Lifecycle](adr/0054-file-picker-query-and-index-lifecycle.md)
- [ADR-0055: Start Dashboard Actions, Focus, and Contextual Recents](adr/0055-start-dashboard-actions-and-context.md)
- [ADR-0056: MCP Dual-Era Protocol and Structured Tool Contract](adr/0056-mcp-dual-era-protocol-and-tool-contract.md)
- [ADR-0057: TUI Surface Model and Focus Contract](adr/0057-tui-surface-model-and-focus-contract.md)
- [ADR-0058: Sandbox Capability Profiles and Trust Boundaries](adr/0058-sandbox-capability-profiles-and-trust-boundaries.md)
- [ADR-0059: Public Docs Boundary and Deferred Navigation](adr/0059-public-docs-boundary-and-deferred-navigation.md)
- [ADR-0060: Release Checksum Signing Authority](adr/0060-release-checksum-signing-authority.md)
- [ADR-0061: Workspace Navigation and Root Boundary](adr/0061-workspace-navigation-and-root-boundary.md)
- [ADR-0062: Workspace Process Handoff](adr/0062-workspace-process-handoff.md)
- [ADR-0063: Command Architecture and Execution Boundary](adr/0063-command-architecture-and-execution-boundary.md)
- [ADR-0064: Multi-root Routing and Ownership](adr/0064-multi-root-routing-and-ownership.md)
- [ADR-0065: Provider Protocols and Execution Classes](adr/0065-provider-protocols-and-execution-classes.md)
- [ADR-0066: Bounded Background Task Lifecycle](adr/0066-bounded-background-task-lifecycle.md)
- [ADR-0067: Settings File Hot Reload](adr/0067-settings-file-hot-reload.md)
- [ADR-0068: Unified TUI Visual Language and Markers](adr/0068-tui-visual-language-and-markers.md)
- [ADR-0069: Single-YubiKey Exception for 0.7.0](adr/0069-single-yubikey-0-7-0-exception.md)
- [ADR-0070: IDE Extension Contributions and Execution Trust](adr/0070-ide-extension-contributions-and-execution-trust.md)
- [ADR-0071: TUI Terminal Capability and Frame Delivery Upgrade](adr/0071-tui-terminal-capability-and-frame-delivery.md)
- [ADR-0072: Opt-In OSC 52 Clipboard Write Fallback](adr/0072-opt-in-osc52-clipboard-fallback.md)
- [ADR-0073: TUI Motion and Transition Boundary](adr/0073-tui-motion-and-transition-boundary.md)
- [ADR-0074: Anvil-TUI State Decomposition and Crate Boundary](adr/0074-anvil-tui-state-decomposition-and-crate-boundary.md)
- [ADR-0075: Settings Environment Compatibility Surface](adr/0075-settings-environment-compatibility-surface.md)
- [ADR-0076: Built-in LSP Profile Scope](adr/0076-built-in-lsp-profile-scope.md)
- [ADR-0077: Benchmark-Only Text Backends](adr/0077-benchmark-only-text-backends.md)
- [ADR-0078: Gateway Authorization Scopes, Trust Sources, and Audit](adr/0078-gateway-authorization-scopes-and-audit.md)
- [ADR-0079: Document and View Ownership for TUI Buffers](adr/0079-document-view-ownership-for-tui-buffers.md)

# History

## Historical plans

- [0.4.x Feature Plan](history/plans/0.4.x-plan.md)
- [Phase 0.5 Plan: Core Evidence and Editing Primitives](history/plans/phase-0-5-plan.md)
- [Phase 0.6 Plan: Security and Host Boundaries](history/plans/phase-0-6-plan.md)
- [0.3.0 → 0.4.0 / Phase 0.7 Plan (Polish & Specification)](history/plans/phase-0-7-plan.md)
- [0.3.0 → 0.4.0: Polish & Specification](history/plans/polish-and-specification.md)
- [Ratatui TUI Elm Loop Plan](history/plans/ratatui-elm-loop-plan.md)
- [0.5.x Interaction and UI Plan](history/plans/0.5.x-plan.md)
- [0.6.0.x Provider Protocol Plan](history/plans/0.6.0.x-provider-protocol-plan.md)
- [0.6.x Plugin Distribution, Marketplace, and Download Manager](history/plans/0.6.x-plan.md)
- [0.7.x Official Optional Rust Modules Plan](history/plans/0.7.x-plan.md)
- [Internal Polish Plan](history/plans/internal-polish-plan.md)
- [IDE Capability Plan](history/plans/ide-capability-plan.md)
- [Multi-Root File Tree and Search Routing Plan](history/plans/multi-root-routing-plan.md)
- [Theme and Settings Plan](history/plans/theme-settings-plan.md)

## Release records

- [Release Dry Run — 2026-11-20](history/releases/release-dry-run-2026-11-20.md)
- [Release Dry Run — 2026-11-22](history/releases/release-dry-run-2026-11-22.md)
- [YubiKey Release Rehearsal — 2026-11-22](history/releases/release-yubikey-rehearsal-2026-11-22.md)

## Historical analysis

- [TUI View Boundary and Extraction Decision](history/analysis/tui-view-boundary.md)

# Contributor templates

- [Plan template](templates/plan.md)
- [ADR template](templates/adr.md)

# Appendix

- [Anvil demo](demo/workspace/README.md)