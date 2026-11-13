# ADR-0021: Signed Plugin Index, Revocations, and Anti-Rollback

- Status: Accepted; gates 1-4 and expiry policy landed in `go/internal/pluginindex`; gates 5-7 remain pending
- Date: 2026-11-07
- Related: [ADR-0017](0017-plugin-activation-and-lifecycle.md),
  [ADR-0020](0020-content-addressed-plugin-store.md),
  [0.6.x plan](../0.6.x-plan.md), [threat model](../threat-model.md)

## Context

Anvil can verify a local `.anvil-plugin` package and store it by digest, but it
has no signed catalog, no publisher identity for indexed packages, and no
rollback or revocation policy. A marketplace cannot trust an unsigned index or
a cached index that a mirror can replay with older content.

Package signatures already use detached Ed25519 over the SHA-256 digest of the
archive bytes. The index design should reuse that primitive instead of adding
a second signing stack.

This ADR freezes the index/revocation wire format and trust rules. It does not
implement the marketplace UI or the download manager.

## Decision

### Trust roots

A release build embeds:

- one or more root Ed25519 public keys;
- the HTTPS base URL for the signed index;
- the maximum index age and fetch limits.

Root keys are not learned by trust-on-first-use and are not configurable by an
index. A key identifier is:

```text
key_id = "sha256:" + lowercase_hex(SHA-256(raw 32-byte public key))
```

Root-key rotation requires a new Anvil release containing both the old and new
keys during the overlap. An index-driven root rotation is not supported in v1.
A build-time development override may be used only by test/dev builds; it is
not accepted by an official release build and cannot authorize `official`
source class.

`ANVIL_INDEX_TRUST_STORE` is reserved for tests and local development. Indexes
verified through it are displayed as development data and cannot make a package
`Official` or `TrustedDistribution`.

### Signed documents

Two documents share one envelope:

| File | Purpose | Maximum size |
|---|---|---|
| `index.json` + `index.sig` | catalog and publisher metadata | 4 MiB |
| `revocations.json` + `revocations.sig` | append-only revocation set | 1 MiB |

The JSON file is signed as exact bytes. The signature envelope is:

```json
{
  "schema_version": 1,
  "key_id": "sha256:...",
  "signature": "lowercase_hex_64_bytes"
}
```

- Publisher IDs and key IDs are unique within one document. Plugin IDs are
  unique, and version strings are unique within one plugin.

The Ed25519 signature covers `SHA-256(document_bytes)`, matching the existing
package-signature convention. The payload is parsed only after the signature
and size checks pass. Index signatures use lowercase hex only; the flexible
raw/hex/base64 package-signature decoder is not reused for index documents.

A document is rejected when any of these is true:

- it is not valid UTF-8;
- it exceeds its byte cap;
- the signature envelope has an unknown schema, unknown key ID, or wrong
  signature length;
- the signature does not verify;
- after signature verification, the JSON has an unknown top-level schema or a
  trailing non-whitespace value.

The canonical bytes are the downloaded bytes. Verifiers must not re-encode
JSON before checking the signature.

### Index payload

```json
{
  "schema": "anvil.plugin-index",
  "schema_version": 1,
  "sequence": 42,
  "issued_at": "2026-11-07T00:00:00Z",
  "expires_at": "2026-11-14T00:00:00Z",
  "publishers": [
    {
      "id": "anvil",
      "name": "Anvil",
      "key_id": "sha256:...",
      "public_key": "lowercase_hex_32_bytes",
      "official": true
    }
  ],
  "plugins": [
    {
      "id": "anvil.official.trim-whitespace",
      "name": "Trim Whitespace",
      "summary": "Removes trailing whitespace on save.",
      "publisher_id": "anvil",
      "homepage": "https://example.invalid/trim-whitespace",
      "license": "MIT",
      "versions": [
        {
          "version": "0.1.0",
          "digest": "lowercase_hex_32_bytes",
          "manifest_hash": "lowercase_hex_32_bytes",
          "package_url": "https://packages.example.invalid/trim-whitespace-0.1.0.anvil-plugin",
          "signature_url": "https://packages.example.invalid/trim-whitespace-0.1.0.anvil-plugin.sig",
          "package_size": 12345,
          "api_min": "1.0.0",
          "api_max": "2.0.0",
          "platforms": ["linux-x86_64", "linux-aarch64"],
          "permissions": [],
          "published_at": "2026-11-07T00:00:00Z",
          "yanked": false
        }
      ]
    }
  ]
}
```

Rules:

- `schema_version` is the index schema version, not the plugin API version.
- `sequence` is a monotonically increasing unsigned integer per index URL.
- `expires_at` must be after `issued_at` and at most seven days after it.
- `publishers[].key_id` must match `SHA-256(public_key)`.
- `plugins[].publisher_id` must name one publisher in the same document.
- Plugin IDs use the existing plugin-ID validator. Version strings use the
  existing semver rules and are unique within one plugin entry.
- `api_min` is inclusive and `api_max` is exclusive. The running
  `API_VERSION` must satisfy `api_min <= API_VERSION < api_max`.
- `platforms` must contain at least one entry. Unknown platform names are
  ignored for matching; a version with no matching current platform is
  incompatible.
- `permissions` is the publisher-declared permission set used for the detail
  view. After download it must exactly match the manifest's permission set;
  mismatch rejects the package.
- `manifest_hash` is `SHA-256` of the exact root `plugin.json` bytes. The
  installed-state manifest hash must use this definition before indexed
  installs are enabled.
- `digest` is `SHA-256` of the exact compressed package bytes.
- `package_size` is the exact compressed size and must not exceed
  `MAX_PACKAGE_BYTES`.
- `package_url` and `signature_url` are absolute HTTPS URLs with no userinfo
  or fragment. Their hosts must be on the installer's exact host allowlist.
  Redirects remain forbidden.
- `yanked` blocks new installs but does not disable an installed version.
  Revocation is the only mechanism that disables installed code.
- No field may contain HTML or terminal control sequences. The UI treats all
  text as untrusted plain text.

Bounds for schema version 1:

| Name | Limit |
|---|---|
| `MAX_INDEX_BYTES` | 4 MiB |
| `MAX_INDEX_PUBLISHERS` | 1024 |
| `MAX_INDEX_PLUGINS` | 4096 |
| `MAX_VERSIONS_PER_PLUGIN` | 64 |
| `MAX_INDEX_TEXT` | 4096 bytes per text field |
| `MAX_INDEX_URL` | 2048 bytes per URL |
| `MAX_PACKAGE_BYTES` | 64 MiB (existing package cap) |
| `MAX_REVOCATION_BYTES` | 1 MiB |
| `MAX_REVOCATIONS` | 8192 |
| `MAX_SIGNATURE_BYTES` | 4 KiB |

An index that exceeds any cap is rejected as a whole; there is no partial
acceptance.

### Publisher and source class

The index signature authenticates the publisher key, but does not by itself
make every package `Official`.

Source class is derived from install origin:

| Origin | Source class |
|---|---|
| build-embedded index, publisher marked `official: true` | `official` |
| build-embedded index, any other publisher | `trusted-distribution` |
| explicit local package file | `local-file` |
| `ANVIL_PLUGIN_DIR` directory | `local-user` |
| development trust override | development only; never official |

The installer must verify the package's detached signature against the
publisher's `public_key` from the signed index. A local trust-store pin for the
plugin ID, when present, must match that key. A key change for an already
pinned plugin ID is rejected unless the user explicitly removes the pin; a
signed index alone does not silently rotate publisher keys.

On the first indexed install, the reviewed publisher key is pinned only after
explicit user consent. If the trust store cannot be written, the install fails
rather than proceeding without a pin.

The index-declared `source` does not exist. A package or manifest cannot
self-declare a source class.

### Revocation payload

```json
{
  "schema": "anvil.plugin-revocations",
  "schema_version": 1,
  "sequence": 7,
  "issued_at": "2026-11-07T00:00:00Z",
  "expires_at": "2026-11-14T00:00:00Z",
  "entries": [
    {
      "kind": "package",
      "plugin_id": "example.plugin",
      "digest": "lowercase_hex_32_bytes",
      "reason": "malware",
      "revoked_at": "2026-11-07T00:00:00Z"
    },
    {
      "kind": "publisher",
      "publisher_key_id": "sha256:...",
      "reason": "key compromise",
      "revoked_at": "2026-11-07T00:00:00Z"
    }
  ]
}
```

Rules:

- Revocations are append-only for schema version 1. A client keeps the union
  of previously accepted entries and the new document; omission from a newer
  document never un-revokes an entry.
- A package revocation is keyed by digest. `plugin_id` is display/audit data
  and must match the installed package when checked.
- The same digest with a conflicting `plugin_id` makes the revocation
  document invalid. The cached union keeps the first accepted mapping.
- A publisher revocation blocks every package signed by that key.
- Revoked packages cannot be installed, enabled, or activated. An installed
  package that becomes revoked is stopped and disabled on the next check.
- Revocation checks use the local cached union and do not require network
  access. Emergency local disable remains available.
- An expired revocation document does not clear cached revocations.

### Cache, state, and anti-rollback

Cache and state live under the plugin data root:

```text
$XDG_DATA_HOME/anvil/plugins/index/
  index.json
  index.sig
  revocations.json
  revocations.sig
  state.json
```

The directory is `0700`; files are `0600`. Writes use a private temp file,
fsync, and rename. A failed download or verification leaves the previous cache
intact.

`state.json` schema:

```json
{
  "version": 1,
  "index_url": "https://.../index.json",
  "index": {
    "sequence": 42,
    "digest": "lowercase_hex_32_bytes",
    "issued_at": "2026-11-07T00:00:00Z"
  },
  "revocations": {
    "sequence": 7,
    "digest": "lowercase_hex_32_bytes"
  },
  "revoked_packages": [
    {"plugin_id": "example.plugin", "digest": "lowercase_hex_32_bytes"}
  ],
  "revoked_publishers": ["sha256:..."]
}
```

The cache is valid only when its document digest matches the digest recorded in
`state.json`. A cache written before a failed state update is discarded and
refetched.

Anti-rollback rules:

1. First use accepts a correctly signed document and records its sequence.
2. A document with a lower sequence than the recorded sequence is rejected and
   the previous cache remains active.
3. An equal sequence is accepted only when the document digest is identical.
   Equal sequence with different bytes is an equivocation and is rejected.
4. A higher sequence is accepted only after full schema, bounds, signature, and
   expiry validation.
5. Index and revocation sequences are independent.
6. The local state is not tamper-proof against a local privileged attacker.
   It protects against network replay and mirror rollback, not against
   deliberate deletion by the same user.

`issued_at` may be at most five minutes in the future. A document whose
`expires_at` has passed remains usable for cached browsing, but new installs
are disabled and the UI must mark it stale. `expires_at` must not be more than
seven days after `issued_at`.

### Offline behavior

- Network failure keeps the last verified cache and reports offline/stale
  status.
- Cached browsing is allowed while offline.
- Indexed installs require a non-expired index and a non-expired revocation
  document. If the revocation document cannot be refreshed or is expired, new
  indexed installs are blocked; existing local files remain installable with a
  visible warning.
- Existing installed packages are checked against the cached revocation union
  at startup and after any refresh, without requiring network access.

### Install flow for indexed packages

1. Fetch and verify the index, then compare anti-rollback state.
2. Fetch and verify the revocation document; update the local union.
3. Resolve plugin, version, publisher, compatibility, and revocation status.
4. Reject before enqueue when `package_size > MAX_PACKAGE_BYTES`, the version
   is yanked, revoked, incompatible, or its publisher key is revoked.
5. Start `pkg.download` with the expected digest, size, signature URL, and
   publisher key ID. The download queue enforces the caps and digest before
   handing the package to the installer.
6. Verify the package signature against the signed publisher key, parse the
   manifest, verify `manifest_hash` and `permissions`, then install through the
   content-addressed store.
7. Leave the plugin disabled and do not activate it.

`sys.fetch` carries only the index and revocation documents. Package archives
continue through the bounded `pkg.download` queue; they are not placed in a
JSON fetch response.

## Consequences

Positive:

- Mirrors cannot replay an older signed index or revocation list.
- Publisher keys are authenticated by the index and pinned locally.
- Revocation works offline and cannot be undone by a stale document.
- Package bytes remain on the existing digest/signature/store path.
- The schema has explicit caps and compatibility checks before download.

Negative:

- Release key provisioning and rotation are release-engineering work.
- The manifest hash definition must be aligned with the installed-state store
  before indexed installs are enabled.
- A local privileged attacker can delete anti-rollback state; this is outside
  the network-replay threat model.
- Schema version 1 does not support sharded indexes. If 4 MiB is exhausted,
  a later ADR must define shards rather than raising the cap silently.

## Alternatives considered

### Unsigned index with package signatures only

Rejected. Package signatures authenticate a package, but not the catalog,
publisher list, compatibility metadata, or revocation state.

### Trust-on-first-use for the index root

Rejected. A first-use interception could become the root of trust. The index
root must ship with the release.

### Embed package metadata in each package

Rejected. It forces one fetch per plugin, prevents bounded catalog validation,
and makes revocation and search expensive.

### Store package bytes in the index

Rejected. `sys.fetch` is a bounded JSON response; a 64 MiB package cannot pass
through it safely. Use `pkg.download`.

### Make the index a Git repository

Rejected for v1. It adds a second transport and does not provide expiry,
sequence-based rollback rejection, or a detached-signature contract.

### Allow an index to rotate the root key

Rejected for v1. Root rotation is an application update. Accepting a new root
from a document signed by the old root would make root compromise unrecoverable
without a release anyway.

## Implementation gates

Status: the detached verifier, schema/bounds checks, cache/state atomicity,
anti-rollback sequence checks, revocation union, and issued/expiry window
checks are implemented in `go/internal/pluginindex`. Manifest-hash alignment
(gate 5), indexed install integration (gate 6), and release provenance
(gate 7) remain pending.

1. Root-key fixture and detached index-signature verifier with malformed,
   wrong-key, truncated, and trailing-data tests.
2. Schema parser with every cap and compatibility check.
3. Cache/state atomicity and anti-rollback tests: lower sequence, equal
   sequence with different bytes, crash between cache and state writes.
4. Revocation union tests: append-only, expired document, offline enforcement,
   revoked installed package, revoked publisher.
5. Exact `plugin.json` manifest hash alignment with the store.
6. Indexed install integration: digest/size passed to `pkg.download`, package
   signature checked against the indexed publisher key, no auto-enable.
7. Release signing/provenance gate before an index is published.

## References

- [0.6.x distribution plan](../0.6.x-plan.md)
- [ADR-0020 content-addressed store](0020-content-addressed-plugin-store.md)
- [Threat model](../threat-model.md)
- [Plugin API v1 spec](../spec/plugin-api-v1.md)
