# ADR-0033: LaTeX Language Provider

- Status: Accepted for implementation; initial slice landed
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

This slice adds LSP support only. It does not add Tree-sitter LaTeX syntax
highlighting, a build command, or a PDF preview. Those require separate slices.

No new process path, gateway permission, or language-server transport is
introduced.

### 4. Tests

- `.tex`/`.bib` mapping and language ids;
- unknown extensions return `None`;
- provider registration;
- standard feature on/off;
- fallback to the built-in catalog when the provider returns `None`;
- module dependency tree contains no `anvil-tui`.

## Consequences

- LaTeX files can use `texlab` without changing the built-in catalog.
- Feature-free builds behave exactly as before.
- Full LaTeX editing still needs a syntax/highlighting slice.
