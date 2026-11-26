# Contributing to Anvil

Anvil is preparing the `0.8.0` release candidate. The most useful
contributions are security review, threat-model corrections, benchmark
fixtures, and small vertical slices that match the charter and ADRs.

## Before you start

Read:

1. [PROJECT_CHARTER.md](PROJECT_CHARTER.md)
2. [docs/src/threat-model.md](docs/src/threat-model.md)
3. [docs/src/architecture.md](docs/src/architecture.md)
4. The ADR relevant to your change.

If a change alters a trust boundary, permission, IPC contract, text model, or
security invariant, open an ADR before implementation.

## Licensing

Anvil is dual-licensed under Apache-2.0 or MIT, at the user's option. By
contributing, you agree to license your contribution under the same terms.

Use the Developer Certificate of Origin (DCO) 1.1. Sign off every commit:

```bash
git commit -s -m "your message"
```

There is no separate CLA.

## Development setup

The high-level layout is:

```text
Cargo.toml                  Rust workspace
go.mod                      Go module at the repository root
crates/anvil-api/           public Rust types and schemas
crates/anvil-core/          text model, commands, snapshots
crates/anvil-editor/        selection-first editor state and snapshots
crates/anvil-syntax/        Tree-sitter highlighting and scope extraction
crates/anvil-workspace/     file workers, tree snapshots, watch status
crates/anvil-session/       headless session, providers, plugin lifecycle
crates/anvil-ipc/           UDS transport, JSON-RPC framing
crates/anvil-control/       control socket and shared command manifest
crates/anvil-tui/           terminal frontend
crates/anvil-plugin-host/   sandboxed Luau host
go/cmd/anvil-gateway/       gateway entry point
go/cmd/anvil-exec-runner/   sandboxed rg/fd launcher
go/internal/                gateway, LSP, watch, fetch, policy, fsroot
docs/src/                   charter, threat model, ADRs, plans, performance
```

The pinned development toolchain is:

- Rust **1.100.0** via `rust-toolchain.toml`; install the declared components
  (`rustfmt`, `clippy`, `rust-src`, `rust-analyzer`).
- Go **1.27.4** via `go.mod` and `mise.toml`.
- Node **26.13.1** and TypeScript **5.9.3** for generated API binding type-checks.
  Node is not a runtime dependency.
- A C/C++ compiler for the vendored Luau plugin host.
- Python 3 for repository scripts and smoke checks.
- mdBook **0.5.4** via `mise.toml` for the documentation book.

`mise install` installs the pinned Go and Node versions. Rust is selected by
`rust-toolchain.toml`. `tmux` is additionally required for the real-TUI smoke
scripts. Do not introduce a new native dependency without an ADR.

## Required checks

A pull request must pass:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

For Go:

```bash
gofmt -w .
go vet ./...
go test ./...
go install github.com/golangci/golangci-lint/v2/cmd/golangci-lint@v2.15.0
golangci-lint run ./...
go install golang.org/x/vuln/cmd/govulncheck@v1.9.0
govulncheck ./...
```

For generated API bindings:

```bash
cargo test -p anvil-api --features bindings
npm install --global typescript@5.9.3
tsc --noEmit --skipLibCheck crates/anvil-api/bindings/*.ts
```

`cargo semver-checks` is used by CI; C/C++ language-server smoke tests also
need `clangd`, `jdtls`, `rust-analyzer`, and `gopls` on `PATH` when run.

## Local CI

GitHub Actions can be run locally with act and Docker:

```bash
docker pull ghcr.io/catthehacker/ubuntu:act-latest
act -l
act -j docs
act -j rust
```

See [Local CI](docs/src/local-ci.md) for image mapping, job selection, and
limitations. Act is a fast feedback loop, not a replacement for hosted CI.

### Documentation book

`docs/src/` is an mdBook source tree; `docs/src/SUMMARY.md` is the chapter
list. The `docs` CI job builds it and `scripts/docs-check.py` fails on missing
chapters, broken local links or anchors, plans outside their allowed
directories, or a root Markdown count over budget.

```bash
mdbook serve docs      # live preview on http://localhost:3000
mdbook build docs      # writes dist/book
python3 scripts/docs-check.py
```

Relative links inside the book must stay inside `docs/src/`; link to files
outside it with an absolute `https://github.com/GBLMX/Anvil/...` URL.

Security and performance changes also need the relevant fixtures under
`crates/*/benches` or the crate's integration tests.

## Pull requests

- Keep the change scoped; do not mix refactors with behavior changes.
- Explain the security impact in the PR description.
- Add or update tests. A security invariant without a test is incomplete.
- Update the charter, threat model, ADR, or performance document when behavior
  changes.
- Never claim a sandbox or performance property that the repository does not
  test.
- Avoid committing generated caches, editor state, or local benchmark output.

## Commit style

Use imperative, scoped messages where practical:

```text
docs: clarify plugin host boundary
feat(gateway): add openat2 workspace resolver
fix(plugin-host): reject native module loading
test(exec): cover symlink race in rg
```

## Project process

- [Governance](GOVERNANCE.md)
- [Maintainers and review ownership](MAINTAINERS.md)
- [Release process](docs/src/RELEASE.md)
- [Support policy](SUPPORT.md)
- [Roadmap](docs/src/ROADMAP.md)

## Security reports

Do not put vulnerability details in a public issue or pull request. Follow
[SECURITY.md](SECURITY.md).

## Code of conduct

Participation is governed by [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
