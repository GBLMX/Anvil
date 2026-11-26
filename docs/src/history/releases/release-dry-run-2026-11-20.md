# Release Dry Run — 2026-11-20

Status: passed on the local i7-12700H Linux host with ephemeral Ed25519 keys.
This exercises the release layout, installer, signed official plugin, package
verification, artifact signing, SBOM shape, and installed `doctor` profile.

## Keys

- Plugin signing key: ephemeral Ed25519 PEM.
- Release signing key: ephemeral Ed25519 PEM.
- Public release key SHA-256:
  `6edfec66a048b3afc07ed226abd84883a8f7dda54aae796a3bdf24796855b5b1`
- The private keys were generated in a temporary directory and deleted after
  the run. They are not production custody keys.

## Layout and lifecycle

```sh
ANVIL_PLUGIN_SIGNING_KEY="$tmp/plugin-signing.pem" \
  scripts/release-layout.sh "$tmp/layout"
scripts/release-lifecycle-smoke.sh "$tmp/layout"
```

The layout built with the `full` feature set. The signed official
`trim-whitespace` package was installed with `source: "official"`. The
lifecycle smoke completed install, doctor, upgrade, stale-file cleanup, and
uninstall while preserving configuration/runtime state.

Temporary plugin package digest:

```text
594c23658c78d109d968efe7142bebba4d545baad64ebb1393a3ddb13744fc09
```

## Artifacts

```sh
syft layout -o "spdx-json=anvil-0.7.0-dryrun.sbom.spdx.json"
tar --sort=name --mtime='@0' --owner=0 --group=0 --numeric-owner \
  -czf anvil-0.7.0-dryrun-linux-x86_64.tar.gz -C layout .
sha256sum anvil-0.7.0-dryrun-linux-x86_64.tar.gz \
  anvil-0.7.0-dryrun.sbom.spdx.json release.pub.pem > SHA256SUMS
openssl pkeyutl -sign -inkey release-signing.pem -rawin \
  -in SHA256SUMS -out SHA256SUMS.sig
sha256sum -c SHA256SUMS
openssl pkeyutl -verify -pubin -inkey release.pub.pem -rawin \
  -in SHA256SUMS -sigfile SHA256SUMS.sig
```

Results:

```text
tarball 8d8fcf2c6987e5aa25b8517e8e0492ce6550511c3c4f489f852d018501767d99
sbom    4413cbea16ecfe2745db2ecb75620a57712ce4ca82208b76b75c59a697c8aa98
signature: verified
sbom: SPDX-2.3, 7 packages
```

The archive contained `./install.sh`, `./uninstall.sh`, `./bin/anvil`, and
`./bin/anvil-gateway`. Installing from the tarball produced an installed
`doctor` report with `ok: true`, and the installed plugin state retained
`source: "official"`.

## 0.7.0 release gates

- `cargo test --workspace --all-targets`: 1294 passed, 16 ignored.
- `cargo test -p anvil-api --features bindings`: 53 passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- `cargo deny check`: passed (`advisories`, `bans`, `licenses`, `sources`);
  upstream duplicate-`syn` and unused license-allowance warnings remain.
- `go test -race ./...`, `go vet ./...`, `golangci-lint run ./...`, and
  `govulncheck ./...`: passed with `umask 022`.
- `shellcheck scripts/*.sh`, `actionlint`, and YAML parsing: passed.
- Generated bindings and API surface checks: unchanged.

The first Go race run inherited `umask 077` from the signing rehearsal and
failed the copy-mode assertion; rerunning with `umask 022` passed. The failure
was test-environment state, not a repository change.

## Remaining release-time steps

- Production key custody and hardware-backed signing ceremony.
- GitHub build-provenance attestation and `gh attestation verify`.
- The next public version is `0.7.0`; the workspace and release-version guard
  are updated, but no public tag is authorized yet.

The dry run used the non-release label `0.7.0-dryrun`. The current release
candidate is `0.7.0`; public publication remains gated on production signing
and provenance.
