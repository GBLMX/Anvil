# ADR-0051: External Editor Reference Models for UX and Language Support

- Status: Proposed
- Version: 1.0.0
- Date: 2026-11-17
- Related: [ADR-0025](0025-lsp-and-language-breadth.md),
  [ADR-0032](0032-language-tooling-service.md),
  [ADR-0040](0040-helix-keymap-compatibility.md),
  [ADR-0048](0048-per-root-language-tooling-and-lsp-workspace.md),
  [ADR-0050](0050-discovery-first-language-packs.md)
- Supersedes:
- Superseded by:

## Context

Anvil has a strong selection-first engine but is still designing its daily-use
surface: pickers, language readiness, language packs, and discovery defaults.
Several mature editors have already solved adjacent problems. Reading them
without a recorded boundary creates two failure modes:

- copying keymaps or runtime architecture that do not fit the compiled
  distribution and security model;
- collecting links without converting them into Anvil-specific decisions,
  tests, or specs.

ADR-0050 already chooses discovery-first defaults and compiled language packs.
This ADR records which external editors are reference models, what each is used
for, and what compatibility claims Anvil explicitly does not make.

## Decision

### 1. Use each reference for a specific decision, not wholesale adoption

| Reference | Use for | Not a target for |
|---|---|---|
| Kakoune | selection/register/undo semantics for a selection-first core | keymap compatibility or runtime architecture |
| Helix | picker, jumplist, language-config, and selection-first UX patterns | copying all keymaps or becoming a Helix clone |
| Neovim | health/readiness and failure-reporting UX | plugin runtime, Lua config, or Vimscript compatibility |
| LazyVim | curated defaults, extras, and which-key discovery | `lazy.nvim` runtime loading or Mason auto-install |
| Zed | language-extension metadata and language onboarding shape | extension runtime or dependency model |
| LSP 3.17 | language-server capability and configuration contract | assuming every server implements every capability |
| Tree-sitter ABI docs | grammar compatibility and upgrade policy | runtime grammar download or plugin-provided parsers |

Anvil may deliberately diverge from a reference when its trust boundary,
compiled distribution, or selection-first model requires it. A divergence must
be documented in the relevant ADR or spec.

### 2. Selection and editing semantics

Kakoune is the primary semantic reference for:

- a selection always has an anchor, a cursor, and a direction;
- multiple selections are the default primitive, not a special mode;
- registers and undo boundaries are part of the editing model, not a keymap
  afterthought.

Helix is the primary UX reference for:

- motions and actions operating on selections;
- picker and jumplist interaction patterns;
- language configuration structure.

Anvil does not promise Kakoune or Helix keymap compatibility. Keymaps remain
Anvil-owned and conflict-audited.

### 3. Discovery and pickers

Helix's picker and jumplist documentation is the reference for the first
diagnostic, jumplist, and changed-file pickers. LazyVim is the reference for
leader-group discoverability and which-key presentation.

The following must be produced before implementation is called complete:

- a picker matrix: trigger, data source, preview, result bound, cancellation,
  stale-response policy, and read-only/mutating classification;
- a conflict audit against existing Anvil keymaps;
- explicit "not planned" entries for pickers Anvil intentionally omits.

### 4. Language support and readiness

Helix `languages.toml` and Zed language extensions are the reference for the
shape of language-pack metadata:

- grammar / file extensions;
- language server profile and root markers;
- formatter and linter;
- optional test/debug provider;
- readiness and missing-tool reporting.

Neovim `:checkhealth` is the reference for presenting a missing dependency as
an actionable status rather than a generic startup error.

Anvil language packs remain compiled-in feature compositions per ADR-0050.
No external editor's runtime package manager becomes an Anvil dependency.

### 5. Normative contracts

When Anvil implements or debugs an LSP or grammar behavior, the normative
sources are:

- the LSP 3.17 specification for protocol fields, capabilities, and
  workspace/configuration behavior;
- Tree-sitter ABI documentation for grammar/library compatibility.

Local implementation convenience does not override a protocol requirement
without an explicit Anvil decision. Unknown or unimplemented server
capabilities must be reported as unavailable, not guessed.

### 6. Every reference must produce an Anvil artifact

A reference is accepted into the design process only when it produces at least
one concrete artifact:

```text
docs/picker-matrix.md
docs/language-pack-spec.md
docs/languages-schema.md
docs/grammar-compatibility.md
ADR update
focused tests
```

Links alone are not a design decision. A new external dependency still needs
its own ADR under ADR-0006/ADR-0023.

## Consequences

Positive:

- Design discussions have named references and explicit non-goals.
- Pickers, language packs, and readiness checks can be compared consistently.
- Anvil avoids cargo-culting keymaps or runtime plugin architecture.
- LSP and Tree-sitter behavior has a normative source instead of folklore.

Negative:

- Reference projects change; links and behavior need periodic review.
- The artifact requirement adds documentation work before implementation.
- Users may still expect keymap compatibility with Helix, Kakoune, or Vim;
  documentation must state the boundary.

## Verification

- Picker matrix exists and covers each Helix picker as present, missing, or
  explicitly not planned.
- Language-pack spec maps Helix/Zed metadata to Anvil feature names and
  readiness fields.
- `:languages` reports missing LSP, grammar, formatter, linter, and provider
  separately, with actionable remediation.
- Grammar feature CI records the Tree-sitter ABI and disabled-build behavior.
- No new runtime dependency is added solely because a reference project uses
  it.
- Docs review checks that each accepted reference points to a concrete Anvil
  artifact.

## References

- [Kakoune design](https://github.com/mawww/kakoune/blob/master/doc/design.asciidoc)
  — selection-first interaction and multiple-selection rationale.
- [Helix keymap](https://docs.helix-editor.com/keymap.html) — selection/action
  and textobject UX.
- [Helix pickers](https://docs.helix-editor.com/pickers.html) — picker types,
  filtering, and interaction.
- [Helix jumplist](https://docs.helix-editor.com/master/jumplist.html) — jump
  semantics and picker behavior.
- [Helix languages](https://docs.helix-editor.com/languages.html) — language
  configuration shape.
- [Neovim checkhealth](https://neovim.io/doc/user/health/) — readiness and
  troubleshooting framework.
- [Zed language extensions](https://zed.dev/docs/extensions/languages) —
  grammar, language server, and query metadata.
- [LSP 3.17 specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)
  — protocol and capability contract.
- [Tree-sitter ABI versions](https://tree-sitter.github.io/tree-sitter/using-parsers/7-abi-versions.html)
  — grammar compatibility boundaries.
- [LazyVim configuration](https://www.lazyvim.org/configuration) — layered
  defaults and user overrides.
- [LazyVim keymaps](https://www.lazyvim.org/keymaps) — leader-group
  discoverability.
- [LazyVim extras](https://www.lazyvim.org/extras) — language/scenario pack
  presentation.
- [lazy.nvim](https://github.com/folke/lazy.nvim) — runtime plugin model
  explicitly not adopted.
