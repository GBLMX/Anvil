# ADR-0020: Content-Addressed Plugin Store and Installed State

- Status: Accepted
- Date: 2026-11-07
- Related: [ADR-0012](0012-plugin-host-isolation.md),
  [ADR-0013](0013-kernel-distribution-boundary.md),
  [ADR-0014](0014-plugin-api-v1-freeze.md),
  [ADR-0017](0017-plugin-activation-and-lifecycle.md)

## Context

The first package installer extracted a verified package into
`PluginDir/<plugin-id>`. That path identifies a mutable directory, not the bytes
the user reviewed. It cannot represent two versions of one plugin, rollback, or
safe garbage collection, and concurrent installs can race on the same
destination.

The 0.6.x distribution model requires a content-addressed store, installed
state that references a digest, and install/enable separation.

## Decision

### Store root

The plugin data root is `$XDG_DATA_HOME/anvil/plugins`, or
`$HOME/.local/share/anvil/plugins` when `XDG_DATA_HOME` is unset or empty.
Failure to resolve a home directory is an install error; the store never falls
back to the current directory.

The data root is `0700`. Store entries, `state.json`, and `state.lock` are
owner-only; committed store trees are read-only.

### Content addressing

The package digest is the SHA-256 of the compressed `.anvil-plugin` archive.
Each digest is stored once:

```text
<data-root>/
  state.json
  state.lock
  store/
    <digest>/
      package.anvil-plugin
      meta.json
      root/
```

`root/` is the verified extracted tree used as the plugin runtime root.
`meta.json` records the digest, plugin id, version, manifest hash, permissions,
and install time.

### Installed state

`state.json` has an explicit schema version and a map keyed by plugin id. Each
entry stores:

- `active_digest`
- source class
- `enabled`
- granted permissions
- retained versions with digest, version, manifest hash, and install time

One plugin id has one active digest. Retained versions are bounded and exist
only for rollback.

### Serialization

`state.lock` is held exclusively across every state read-modify-write
transaction, including install, update, uninstall, enable/disable, and
rollback. Atomic rename prevents torn files but does not prevent lost updates;
the lock is therefore mandatory.

`store.lock` is held exclusively for every store payload mutation, including
commit, uninstall, and garbage collection. Mutations acquire `store.lock`
before `state.lock`; this prevents an editor-side uninstall from deleting a
digest while an installer is committing it. State-only changes such as
enable/disable and rollback take only `state.lock`.

State writes use a private `0600` temp file in the data root, fsync the file,
rename over `state.json`, and fsync the directory.

### Install transaction

1. Verify archive size, digest, signature, trust, manifest, id, and API range.
2. Extract into a private stage under the store.
3. Write `meta.json`, archive copy, and read-only tree.
4. Rename the stage to `store/<digest>`; an existing digest must match.
5. Update `state.json` under `state.lock`.
6. Leave `enabled: false`.

Failure before the store rename leaves no entry. Failure after the store rename
leaves an unreferenced entry suitable for collection.

### Source classes and local plugins

- `official`
- `trusted-distribution`
- `local-file`
- `local-user`

Source class comes from the install origin, never from `plugin.json`.
`ANVIL_PLUGIN_DIR` remains supported as `local-user`; it is not written into
managed state and cannot claim an official source.

### Discovery

Managed discovery resolves `active_digest` to `store/<digest>/root`. Local
discovery still scans `ANVIL_PLUGIN_DIR`. A managed/local duplicate id is a
reported conflict; managed state wins and the local copy is not loaded.

## Consequences

- Updates and rollback can reference immutable package bytes.
- Uninstall can remove state without guessing which directory to delete.
- Concurrent installers serialize instead of losing state updates.
- A corrupt state file fails closed for managed plugins and reports a visible
  error; local-user plugins remain available.
- The CLI and any future installer service must write through the store rather
  than a mutable plugin directory.
