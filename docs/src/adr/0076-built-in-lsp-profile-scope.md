# ADR-0076: Built-in LSP Profile Scope

- Status: Proposed
- Version: 1.0.0
- Date: 2026-11-24
- Related: [ADR-0025](0025-lsp-and-language-breadth.md),
  [ADR-0032](0032-language-tooling-service.md),
  [ADR-0038](0038-project-settings-trust.md),
  [ADR-0050](0050-discovery-first-language-packs.md),
  [ADR-0052](0052-language-tool-setup-and-source-resolution.md),
  [ADR-0075](0075-settings-environment-compatibility-surface.md)
- Supersedes: the built-in catalog scope clauses of ADR-0050 and ADR-0052
- Superseded by:

## Context

`anvil-lsp-client` currently has 15 built-in server groups across 22 extension
match arms: Rust, Go, Python, TypeScript/JavaScript, C/C++, Java, Lua, JSON,
YAML, Shell, Markdown, TOML, HTML/CSS, and Zig.

There is no "default trust set" in the current code. The built-in catalog is
lazy: a server is resolved only when a matching file is opened. Servers are
external trusted tools under ADR-0025, not plugins in the sandbox. Reducing the
catalog therefore reduces the built-in discovery/doctor/documentation surface,
not a set of always-running processes.

The current setup path limits how aggressively profiles can be removed.
`anvil setup --profile` is documented in ADR-0052 but remains a deferred flag.
Removing a built-in profile without a first-class explicit configuration path
would turn a supported language into an undocumented manual setup problem.

## Decision

### 1. Keep five core built-in profiles

The default built-in catalog keeps:

- `rust-analyzer`;
- `gopls`;
- Python (`pyright-langserver`, `basedpyright-langserver`, or `pylsp`);
- TypeScript/JavaScript (`typescript-language-server` or `vtsls`);
- C/C++ (`clangd`).

These five are the curated default surface.

### 2. Move other profiles behind explicit configuration

The other ten profiles -- Java, Lua, JSON, YAML, Shell, Markdown, TOML,
HTML, CSS, and Zig -- become explicit user or root configuration through
`languages.toml` or an explicit setup profile. They remain supported
capabilities; they are no longer auto-discovered as part of the default
catalog.

`anvil setup --profile` (or an equivalent first-class explicit setup path)
must be implemented and tested before a built-in profile is removed. Removal
without that path is not accepted by this ADR.

### 3. Preserve lazy launch and the trust boundary

- A configured server still starts only for a matching file or explicit
  request.
- LSP servers remain external trusted tools under ADR-0025.
- No server process starts merely because it appears in the doctor or
  `:languages` catalog.
- Module-provided language servers remain resolved through ADR-0032 and
  ADR-0028.

### 4. Report the reason for non-discovery

`:languages`, doctor, and `settings check` must distinguish:

- a built-in core profile;
- an explicitly configured profile;
- a module-provided profile;
- an unsupported extension;
- a configured server whose executable is missing.

A missing profile must not be reported as a broken core feature.

## Consequences

Positive:

- The default language catalog and doctor surface become easier to explain.
- Core profiles receive the default test and documentation attention.
- Non-core language support remains available through explicit configuration.
- No trust-boundary or lazy-launch semantics change.

Negative:

- Opening a Java, Lua, JSON, YAML, Shell, Markdown, TOML, HTML/CSS, or Zig
  file no longer finds a built-in profile by default.
- Users must configure `languages.toml` or run an explicit setup profile.
- The setup `--profile` flag and documentation become release prerequisites.
- The change reduces default capability, not merely implementation size.

## Verification

- Focused tests:
  - the default catalog contains exactly the five core profiles;
  - a reverse assertion confirms the ten moved profiles are absent from the
    default catalog;
  - `languages.toml` and per-root configuration can re-add a moved profile;
  - `anvil setup --profile` (when implemented) resolves the same explicit
    catalog;
  - no LSP process starts before a matching file or request.
- Failure/cancellation tests:
  - a missing non-core server reports "not configured", not "core profile
    missing";
  - an untrusted project overlay cannot select an LSP command.
- Real surface smoke:
  - `:languages`, doctor, and setup report the same core/default and
    explicitly configured sets.

## References

- [ADR-0025](0025-lsp-and-language-breadth.md)
- [ADR-0052](0052-language-tool-setup-and-source-resolution.md)
- [ADR-0050](0050-discovery-first-language-packs.md)
- [Language setup](../doctor.md)
