# Release Dry Run — 2026-11-22

Status: passed on the local i7-12700H Linux host with ephemeral Ed25519 keys.
This re-runs the signed release path after reopening the `0.7.0` candidate for
Lua syntax highlighting.

## Keys

- Plugin signing key: ephemeral Ed25519 PEM.
- Release signing key: ephemeral Ed25519 PEM.
- Public release key SHA-256:
  `8919dc64e78d53217f76fe32b82fce7ebddeb92a5860a93dd02051282b0182d6`
- The private keys were generated in a temporary directory and deleted after
  the run. They are not production custody keys.

## Layout and lifecycle

```sh
ANVIL_PLUGIN_SIGNING_KEY="$tmp/plugin-signing.pem" \
  scripts/release-layout.sh "$tmp/layout"
scripts/release-lifecycle-smoke.sh "$tmp/layout"
```

The layout built with the `full` feature set, including Lua highlighting. The
signed official `trim-whitespace` package was installed with
`source: "official"`. The lifecycle smoke completed install, doctor, upgrade,
stale-file cleanup, and uninstall while preserving configuration/runtime state.

Temporary plugin package digest:

```text
594c23658c78d109d968efe7142bebba4d545baad64ebb1393a3ddb13744fc09
```

## Artifacts

```sh
syft layout -o "spdx-json=anvil-0.7.0.sbom.spdx.json"
tar --sort=name --mtime='@0' --owner=0 --group=0 --numeric-owner \
  -czf anvil-0.7.0-linux-x86_64.tar.gz -C layout .
sha256sum anvil-0.7.0-linux-x86_64.tar.gz \
  anvil-0.7.0.sbom.spdx.json release.pub.pem > SHA256SUMS
openssl pkeyutl -sign -inkey release-signing.pem -rawin \
  -in SHA256SUMS -out SHA256SUMS.sig
sha256sum -c SHA256SUMS
openssl pkeyutl -verify -pubin -inkey release.pub.pem -rawin \
  -in SHA256SUMS -sigfile SHA256SUMS.sig
```

Results:

```text
tarball 72b420c7ba4efd36abfba93ae26e390d95847fbc0be1970ca6151fe452b87c3f
sbom    0ff0279f73dffac2b4e8d0db810030f2b918ee638ef9bad266b8f2165640d658
sbom: SPDX-2.3, 7 packages
signature: verified
```

The installed layout passed `anvil doctor --profile installed --json` with
`schema_version: 1`, `profile: installed`, and `ok: true`.

## 0.7.0 release gates

- `cargo test --workspace --all-targets`: 1316 passed, 16 ignored.
- `cargo test -p anvil-api --features bindings`: 58 passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- `cargo deny --offline check`: passed with the existing license/duplicate
  warnings; the online advisory refresh was unavailable from this host.
- `go test -race ./...`, `go vet ./...`, `golangci-lint run ./...`, and
  `govulncheck ./...`: passed.
- `shellcheck scripts/*.sh`, `actionlint`, and YAML parsing: passed.
- Generated bindings export: unchanged; bindings-gated tests passed.

## Remaining release-time steps

- Production key custody and hardware-backed signing ceremony.
- GitHub build-provenance attestation and `gh attestation verify`.
- Repository `ANVIL_PLUGIN_SIGNING_KEY` and `ANVIL_RELEASE_SIGNING_KEY`
  secrets are not present in this checkout's GitHub repository; no public tag
  has been pushed.
