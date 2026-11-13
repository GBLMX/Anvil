# ADR-0033: LaTeX Language Provider

- Status: Accepted for implementation; provider and syntax highlighting landed
- Date: 2026-11-13
- Related: [ADR-0032](0032-language-tooling-service.md)

## Context

`LanguageToolingService` is an additive contract slot with built-in fallback.
The first concrete provider should prove the slot with a language that has a
mature LSP server and no existing built-in entry. LaTeX fits: `texlab` supports
LaTeX and BibTeX, and the current catalog has no `.tex`/`.bib` entry.

## Decision

### 1. Module

- Crate: `anvil-mod-language-latex`.
- Module id: `official.language-latex`.
- Cargo feature: `official-language-latex`.
- Standard edition: opt-in.
- Minimal fixture: absent.

The module contributes only a `LanguageToolingService`; the existing LSP
worker owns process startup, gateway access, and lifecycle.

### 2. Mapping

| Extension | `language_id` | Tool |
|---|---|---|
| `.tex`, `.ltx`, `.sty`, `.cls` | `latex` | `texlab` |
| `.bib` | `bibtex` | `texlab` |

Root markers: `latexmkrc`, `.latexmkrc`, `Tectonic.toml`, `main.tex`, `.git`.

`ANVIL_LSP_LATEX` may override the tool path. Otherwise the provider returns
`texlab`, resolved through the existing LSP startup path.

### 3. Scope

This slice adds the `texlab` launch profile and Tree-sitter syntax
highlighting. It does not add a build command or PDF preview.

No new process path, gateway permission, or language-server transport is
introduced.

### 4. Syntax highlighting

The syntax layer uses `codebook-tree-sitter-latex` (MIT) for `.tex`, `.ltx`,
`.sty`, and `.cls`. The local query highlights comments, sectioning commands,
inline/display math, labels, citations, and include/package/class commands.
BibTeX files remain LSP-only.

The grammar is optional: `anvil-syntax/latex` enables it, and the
`official-language-latex` distribution feature enables both that syntax
feature and the provider. Feature-free builds do not compile the LaTeX grammar.

Highlighting inherits the existing 256 KiB parse cap and background worker
debounce. Parse failure falls back to plain text.

### 5. Tests

- `.tex`/`.bib` mapping and language ids;
- unknown extensions return `None`;
- provider registration;
- standard feature on/off;
- fallback to the built-in catalog when the provider returns `None`;
- module dependency tree contains no `anvil-tui`;
- LaTeX syntax detection and comment/command/label captures.

## Consequences

- LaTeX files can use `texlab` without changing the built-in catalog.
- Feature-free builds behave exactly as before.
- LaTeX documents now get Tree-sitter highlighting in addition to LSP support.
