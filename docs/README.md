# Anvil documentation

The documentation is an mdBook project.

- `book.toml` — book configuration (`src = "src"`, output in `../dist/book`).
- `src/` — chapter sources; `src/SUMMARY.md` is the chapter list.
- `src/index.md` — documentation index, rendered as the book introduction.

```bash
mdbook serve docs      # live preview on http://localhost:3000
mdbook build docs      # writes dist/book
python3 scripts/docs-check.py
```

Relative links inside `src/` must stay inside `src/`; link to files outside the
book (repository `README.md`, `SECURITY.md`, source files) with an absolute
`https://github.com/GBLMX/Anvil/...` URL.
