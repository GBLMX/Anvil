# Contributing to Anvil

Anvil is in Phase 0. The most useful contributions are security review, threat
model corrections, benchmark fixtures, and small vertical slices that match the
charter.

## Before you start

Read:

1. [PROJECT_CHARTER.md](PROJECT_CHARTER.md)
2. [docs/threat-model.md](docs/threat-model.md)
3. [docs/architecture.md](docs/architecture.md)
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

The repository is still a skeleton. The intended layout is:

```text
Cargo.toml                  Rust workspace
crates/anvil-core/          text model, commands, snapshots
crates/anvil-tui/           terminal frontend
crates/anvil-ipc/           UDS transport, JSON-RPC framing
crates/anvil-api/           public Rust types, schema, generated stubs
crates/anvil-test/          security and performance harnesses
crates/anvil-exec-runner/   sandboxed rg/fd launcher
plugin-host/                Lua/QuickJS host and sandbox
go/                         Go module: gateway
docs/                       charter, threat model, ADRs, performance
```

Toolchains are pinned by `rust-toolchain.toml` and `go/go.mod` once they exist.
Do not introduce a new native dependency without an ADR.

## Required checks

Once code exists, a pull request must pass:

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
```

Security and performance changes also need the relevant fixtures under
`crates/anvil-test/`.

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

## Security reports

Do not put vulnerability details in a public issue or pull request. Follow
[SECURITY.md](SECURITY.md).

## Code of conduct

Participation is governed by [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
