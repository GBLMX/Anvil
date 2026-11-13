# ADR-0032: Language Tooling Service Slot

- Status: Accepted for implementation; additive slot and fallback landed
- Date: 2026-11-13
- Related: [ADR-0028](0028-module-contract.md), [ADR-0025](0025-lsp-and-language-breadth.md),
  [ADR-0033](0033-latex-language-provider.md)

## Context

`anvil-tui` discovers language servers from a built-in extension table. A
distribution module may need to add a tooling provider without forking that
table or forcing every distribution to carry the same language set. The first
`0.7.4` desktop slice showed the module pattern; this ADR proposes the next
additive extension point without selecting a product module.

The LSP worker still owns process startup and lifecycle. A provider only
describes a launch profile; it must not open a second process path or bypass the
existing gateway/session boundary.

## Decision

### 1. Contract

`anvil-module` adds a typed, additive service slot:

```rust
pub struct LanguageServerSpec {
    pub tool: String,
    pub args: Vec<String>,
    pub language_id: String,
    pub root_markers: Vec<String>,
    pub label: String,
    pub initialization_options: Option<serde_json::Value>,
}

pub trait LanguageToolingService: Send + Sync {
    fn server_for(&self, path: &Path) -> Option<LanguageServerSpec>;
}

impl ModuleRegistry {
    pub fn set_language_tooling_service(
        &mut self,
        service: Arc<dyn LanguageToolingService>,
    ) -> Result<(), ModuleError>;

    pub fn language_tooling_service(
        &self,
    ) -> Option<Arc<dyn LanguageToolingService>>;
}
```

The spec is plain owned data. It exposes no `anvil-tui` state and no process
handle.

### 2. Resolution order

`lsp_settings` becomes:

1. Ask the installed `LanguageToolingService` for `path`.
2. If it returns a spec, use it.
3. Otherwise use the existing built-in catalog.

The built-in table remains the fallback so a feature-free or module-free build
behaves exactly as before. A provider can override an extension; returning
`None` intentionally defers to the built-in entry.

### 3. Distribution wiring

`DistributionServices` gains an optional `language_tooling` field. The slot
remains empty in the standard edition until a concrete provider module is
accepted. The minimal fixture also leaves it empty.

The service is additive under contract v1. It does not change the existing
`LspWorker` lifecycle, JSON-RPC transport, gateway permissions, or root-marker
selection.

### 4. Validation

The spike must prove:

- a fake provider supplies a new language profile;
- the fake provider can override a built-in extension;
- returning `None` falls back to the built-in catalog;
- the module registry registers and rolls back the service slot;
- no provider is needed for the current workspace tests.

No real language server is required for these tests.

## Consequences

- The next `0.7.4` decision can name a provider module without changing the LSP
  transport.
- Distributions can add language profiles without forking `anvil-tui`.
- The built-in catalog remains the compatibility fallback.
- A provider is trusted in-process code; the LSP worker still owns launch and
  lifecycle.

## Non-goals

- Replacing the built-in language catalog in this slice.
- Moving language-server process ownership into a module.
- Runtime plugin-provided language servers.
- Per-project provider configuration or a second LSP event loop.
- Selecting a concrete official language provider.
