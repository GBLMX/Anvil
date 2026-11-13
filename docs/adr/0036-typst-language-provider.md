# ADR-0036: Typst Language Provider

- Status: Accepted for implementation
- Date: 2026-11-13
- Related: [ADR-0032](0032-language-tooling-service.md),
  [ADR-0033](0033-latex-language-provider.md)

## Context

`LanguageToolingService` is an additive contract slot with a built-in fallback.
The built-in catalog covers Rust, Go, Python, TypeScript/JavaScript, C/C++,
Java, Lua, JSON, YAML, Shell, Markdown, TOML, HTML, CSS, and Zig. It does not
cover Typst.

Typst has a mature language server, `tinymist`, and a Tree-sitter grammar in
`codebook-tree-sitter-typst`. LaTeX proved the provider pattern, but its grammar
was initially compiled into every `anvil-syntax` build. Optional modules must
not silently add grammar code to feature-free compositions.

## Decision

### 1. Module

- Crate: `anvil-mod-language-typst`.
- Module id: `official.language-typst`.
- Cargo feature: `official-language-typst`.
- Standard edition: opt-in.
- Minimal fixture: absent.

The module contributes only a `LanguageToolingService`; the existing LSP
worker owns process startup, gateway access, and lifecycle.

### 2. Mapping

| Extension | `language_id` | Tool | Arguments |
|---|---|---|---|
| `.typ` | `typst` | `tinymist` | `lsp` |

Root markers: `typst.toml`, `.git`.

`ANVIL_LSP_TYPST` may override the tool path. Otherwise the provider returns
`tinymist`, resolved through the existing LSP startup path.

### 3. Syntax highlighting

`anvil-syntax` gains a `typst` Cargo feature that enables the optional
`codebook-tree-sitter-typst` dependency and the `.typ` detection. The
`official-language-typst` distribution feature enables both the provider and
`anvil-syntax/typst`.

Typst highlighting covers comments, strings, numbers, headings, list items,
emphasis, quotes, and call names. Files without the feature remain plain text.
The existing parse-size cap and background highlighting debounce apply.

### 4. Composition

`anvil-distro-standard` composes each selected provider in its own module
registry and combines the resulting services with a small ordered composite.
This preserves the contract-v1 single `LanguageToolingService` slot while
allowing more than one optional provider in one distribution.

### 5. Tests

- `.typ` mapping, tool, arguments, and root markers;
- unknown extensions return `None`;
- provider registration;
- syntax detection and core captures with `anvil-syntax/typst`;
- feature-free syntax builds contain neither LaTeX nor Typst grammars;
- `official-language-typst` on/off build and startup error;
- combined LaTeX + Typst composition;
- module dependency tree contains no `anvil-tui`.

## Consequences

- Typst files get LSP support and highlighting only in compositions that opt in.
- Feature-free builds stay smaller and do not compile optional grammars.
- No new process path, gateway permission, or LSP transport is introduced.

## Non-goals

- Typst compilation, PDF export, or preview.
- Taking over LSP process startup or lifecycle.
- Changing the built-in language catalog.
- Marketplace-installed Rust modules.
