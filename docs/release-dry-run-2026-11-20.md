# Release Dry Run — 2026-11-20

Status: passed on the local i7-12700H Linux host with ephemeral Ed25519 keys.
This exercises the release layout, installer, signed official plugin, package
verification, artifact signing, SBOM shape, and installed `doctor` profile.

## Keys

- Plugin signing key: ephemeral Ed25519 PEM.
- Release signing key: ephemeral Ed25519 PEM.
- Public release key SHA-256:
  `c31727efe20bec3f6e7195ce7367c72f4402f2923c3227806f65d1091f0fe28f`
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
syft layout -o "spdx-json=anvil-0.6.0-dryrun.sbom.spdx.json"
tar --sort=name --mtime='@0' --owner=0 --group=0 --numeric-owner \
  -czf anvil-0.6.0-dryrun-linux-x86_64.tar.gz -C layout .
sha256sum anvil-0.6.0-dryrun-linux-x86_64.tar.gz \
  anvil-0.6.0-dryrun.sbom.spdx.json release.pub.pem > SHA256SUMS
openssl pkeyutl -sign -inkey release-signing.pem -rawin \
  -in SHA256SUMS -out SHA256SUMS.sig
sha256sum -c SHA256SUMS
openssl pkeyutl -verify -pubin -inkey release.pub.pem -rawin \
  -in SHA256SUMS -sigfile SHA256SUMS.sig
```

Results:

```text
tarball 3b872cfca9ded1899e1a1b4f4756055b0f668c3d61a6f471787953de9741b489
sbom    17a7891c8de5923660d306e908e56a256aeef09e1c16213c7f6e96975cbe111b
signature: verified
sbom: SPDX-2.3, 7 packages
```

The archive contained `./install.sh`, `./uninstall.sh`, `./bin/anvil`, and
`./bin/anvil-gateway`. Installing from the tarball produced an installed
`doctor` report with `ok: true`, and the installed plugin state retained
`source: "official"`.

## Remaining release-time steps

- Production key custody and hardware-backed signing ceremony.
- GitHub build-provenance attestation and `gh attestation verify`.
- Explicit choice of the next public version and the matching freeze/version
  update.

`0.6.0` remains frozen for internal polish; this dry run used the non-release
label `0.6.0-dryrun`.
