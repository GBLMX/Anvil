# ADR-0043: Plugin API 1.1 Capability and Selection Contract

- Status: Proposed
- Version: 1.0.0
- Date: 2026-11-15
- Related: [ADR-0003](0003-idl.md), [ADR-0007](0007-jsonrpc-envelope-and-capability-token.md),
  [ADR-0014](0014-plugin-api-v1-freeze.md), [ADR-0037](0037-kernel-api-baseline.md)

## Context

Plugin API v1 is frozen at `1.0.0`. It is deliberately narrow:

- `InteractionMode` only exposes `select`;
- `TextPosition` is editor output and there is no `set_selection` method;
- `editor.register_command` carries an id and title, but no argument schema,
  capability list, or declarative effect descriptor;
- the handshake negotiates one API version, not per-feature capabilities.

These constraints are correct for v1. They must not be changed in place:
adding a public enum variant/field or retrofitting `#[non_exhaustive]` is a
breaking change under the Cargo semver rules. The next plugin capability slice
therefore needs a new explicit API line.

## Decision

### 1. New API line

Add `API_VERSION = 1.1.0` as a separate negotiated line. Hosts and plugins that
speak `1.0.0` remain supported. A `1.1.0` plugin must not assume a `1.0.0` host
implements any 1.1 method.

### 2. Capability negotiation

`session.hello` gains an optional capability list for the 1.1 line. The
advertised capabilities are closed, versioned strings owned by `anvil-api`.
Unknown capabilities are ignored by the peer, not treated as an error.

Initial capability set:

- `selection.write` for validated selection replacement;
- `command.descriptor` for argument/effect metadata;
- `snapshot.viewport` if a bounded viewport-only snapshot is accepted later.

The base v1 DTOs remain unchanged.

### 3. Selection contract

Add a validated `editor.set_selection` request only under
`selection.write`. The request carries:

- the editor-owned buffer id;
- the expected document revision;
- one or more text positions with all coordinate fields populated.

The editor validates UTF-8 boundaries and coordinate consistency before
committing. A stale buffer/revision is rejected; no partial selection update is
allowed. A plugin must not construct positions for editor input unless the
method is advertised.

### 4. Command descriptor

`editor.register_command` remains id/title-only in v1. In 1.1 it may carry an
optional descriptor:

- argument schema identifier;
- required capabilities;
- effect kinds the handler may submit;
- a title/detail string.

Registration remains metadata-only. The editor still authorizes every actual
effect; a descriptor is not a capability grant.

### 5. Compatibility

- New 1.1 types start with `#[non_exhaustive]` where a field/variant may grow.
- Existing v1 types are not silently extended.
- The generated JSON Schema and TypeScript/Lua bindings include a versioned
  catalog for both lines.
- Contract consumers exercise v1-only, 1.1-capable, and mixed-version peers.
- An API line is removed only by a later major ADR.

## Non-goals

- Stable Rust ABI or dynamic Rust plugin loading.
- Changing plugin lifecycle, sandbox policy, or gateway permissions.
- Runtime plugin definition of motions, undo grouping, or editor semantics.
- Command descriptors granting authority.

## Open questions

- Whether `selection.write` should accept text positions or UTF-16 ranges only.
- Whether command effect descriptors are declarative metadata or executable
  schemas.
- Whether `snapshot.viewport` is useful enough to justify another payload DTO.

## Acceptance gates

- v1 compatibility tests continue unchanged.
- 1.1 hello/capability tests cover unknown capabilities, missing host support,
  and stale revisions.
- Selection tests cover invalid UTF-8 boundaries, inconsistent coordinate
  fields, multiple selections, and no partial mutation.
- Command descriptor tests prove metadata cannot bypass capability checks.
