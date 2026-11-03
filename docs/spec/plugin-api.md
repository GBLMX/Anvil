# Anvil Plugin API (draft)

Status: draft for `0.4.9`; not stable. The v1 contract freezes at `0.6.0`.

## Envelope

JSON-RPC 2.0 over the length-prefixed Unix socket transport in `anvil-ipc`.
Requests and responses carry an `id`; notifications omit it. The maximum frame
size is 8 MiB.

## Handshake

`session.hello` carries:
- `protocol`: `"anvil-ipc/1"`
- `api_version`: the caller's API version
- `token`: the session token

The gateway rejects an unsupported protocol or API version before dispatch.
The plugin host sends `api_version: "0.1.0"` in its editor hello. Plugin
manifests declare `api`; manifests outside the supported `0.1` line fail before
the Luau entry is loaded.

## Method catalog

`anvil_api::METHODS` is the single source of truth. Each entry distinguishes:

- `Empty`: no parameter object
- `Pending`: DTO not defined yet
- `Named`: a generated DTO

Typed methods currently include:

- `editor.get_view` -> `EditorView`
- `editor.get_snapshot` -> `DocumentSnapshot`
- `editor.set_mode` -> `SetModeRequest` / `SetModeResponse`
- `editor.apply_edit` -> `ApplyEditRequest` / `ApplyEditResponse`

`ui.*`, `event.*`, and `sys.*` remain pending in the generated DTO catalog even
where the runtime forwards them through the gateway.

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
