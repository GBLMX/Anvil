# Anvil Plugin API v1

Status: stable wire contract, API version `1.0.0`.

## Envelope

JSON-RPC 2.0 over the length-prefixed Unix socket transport in `anvil-ipc`.
Requests and responses carry an `id`; notifications omit it. The maximum frame
size is 8 MiB.

## Handshake

`session.hello` carries:

- `protocol`: `"anvil-ipc/1"` for the plugin host, or the gateway's negotiated
  editor protocol
- `api_version`: `"1.0.0"`
- `token`: the session token where required

The gateway rejects an unsupported protocol or API version before dispatch.
Plugin manifests declare `api` as `1`, `1.0`, or `1.0.0`; other lines fail
before the Luau entry is loaded.

## Manifest

The frozen manifest path is `plugin.json` in the plugin root. The file is
limited to 64 KiB and contains:

```json
{
  "id": "demo",
  "version": "0.1.0",
  "api": "1.0",
  "entry": "run.luau",
  "activationEvents": ["onStartup"],
  "commands": [{"id": "demo.run", "title": "Run"}]
}
```

`entry` is relative, cannot escape the plugin root, and must exist before
activation. `activationEvents` are limited to `onStartup`, `onCommand:`,
`onLanguage:`, and `onWorkspaceContains:`.

## Method catalog

`anvil_api::METHODS` is the single source of truth.

| Method | Params | Result |
|---|---|---|
| `editor.get_view` | empty | `EditorView` |
| `editor.get_snapshot` | empty | `DocumentSnapshot` |
| `editor.set_mode` | `SetModeRequest` | `SetModeResponse` |
| `editor.apply_edit` | `ApplyEditRequest` | `ApplyEditResponse` |
| `editor.register_command` | `RegisterCommandRequest` | `RegisterCommandResponse` |
| `ui.notify` | `NotifyRequest` | `EmptyResponse` |
| `ui.status` | `StatusRequest` | `EmptyResponse` |
| `event.subscribe` | `EventSubscribeRequest` | `EventSubscribeResponse` |
| `sys.read_file` | `ReadFileRequest` | `ReadFileResponse` |
| `sys.create_dir` | `PathRequest` | `EmptyResponse` |
| `sys.rename` | `RenameRequest` | `EmptyResponse` |
| `sys.remove` | `PathRequest` | `EmptyResponse` |
| `sys.copy` | `CopyRequest` | `EmptyResponse` |
| `sys.remove_all` | `PathRequest` | `EmptyResponse` |
| `sys.fetch` | `FetchRequest` | `FetchResponse` |
| `sys.exec` | `ExecRequest` | `ExecResponse` |

## Command registration

`editor.register_command` adds a command to the running editor's command
palette. The request `id` must be namespaced by the calling plugin id
(`<plugin-id>.<command>`), be at most 128 bytes, and have a non-empty title of
at most 256 bytes.

The editor rejects:

- ids that do not match a loaded, enabled plugin;
- duplicate manifest or dynamic command ids;
- more than 512 dynamic commands across the session.

Dynamic commands do not need a matching `onCommand:` activation event; invoking
one starts the plugin host with that command, exactly like an `onCommand`
manifest command. Registration itself grants no filesystem, exec, clipboard, or
editor capabilities.

## UI notifications

`ui.notify` pushes a transient notification with `info`, `warning`, or `error`
severity. Messages are limited to 4096 bytes and appear with the source
`plugin`; the existing notification queue deduplicates matching entries and
keeps the newest 50.

`ui.status` replaces the status-line text for the current interaction. Messages
are limited to 1024 bytes. Neither method grants focus, overlay, clipboard, or
filesystem authority.

## Document transaction

`ApplyEditRequest` carries `buffer_id`, `base_revision`, and non-overlapping
UTF-8 byte-range replacements. The session commits atomically. A stale base
revision returns `ApiError::Conflict` and leaves the document unchanged; invalid
ranges return `invalid_params`.

## Permissions

The gateway checks the registered plugin token, workspace, and grant set.
Activation never grants capabilities. Host `sys.*` calls use the plugin token;
editor-side document calls remain session-scoped.

## Errors

Errors use the stable `ApiError` codes in `anvil-api`; messages are human text
and not part of the contract. Cancellation, timeout, policy denial, and conflict
are distinct codes.

## Compatibility

Additive fields are minor versions. Removing or changing semantics is a
breaking change. Generated JSON Schema, TypeScript, and Luau bindings are
committed and checked for drift in CI. No stable Rust ABI is promised.

See [ADR-0014](../adr/0014-plugin-api-v1-freeze.md) for the freeze decision.
