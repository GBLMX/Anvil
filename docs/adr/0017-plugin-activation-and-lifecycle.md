# ADR-0017: Plugin Activation Events and Host Lifecycle

- Status: Proposed; planning only, no runtime implementation in `0.4.0`
- Date: 2026-11-01
- Related: [ADR-0001](0001-plugin-isolation.md),
  [ADR-0007](0007-jsonrpc-envelope-and-capability-token.md),
  [ADR-0012](0012-plugin-host-isolation.md),
  [ADR-0013](0013-kernel-distribution-boundary.md)

## Context

The plugin host is currently an empty sandboxed process. Plugin loading and the
capability table are not implemented, and the host is not yet connected to the
gateway. Starting every installed plugin at editor startup would spend
processes, memory, file descriptors, and handshakes on code that may never run.

Activation events are the scheduling layer above the existing isolation
boundary. They must not become a second permission system or a hidden startup
path for untrusted code.

This ADR is ordered by dependency, not by feature appeal. Each decision assumes
the decisions before it.

## Prerequisites

1. The gateway can register and revoke a per-plugin token and grant set after
   startup. The current gateway accepts only static CLI permissions and one
   `plugin-id`.
2. A versioned plugin manifest exists and is validated without executing plugin
   code.
3. The editor or a future `anvil-session` owns one supervisor for host
   lifecycle. The current TUI `HostSession` is a debug harness, not a public
   lifecycle API.
4. The capability table and real plugin loading path exist. Activation is not
   useful if the host cannot load a plugin after it starts.

## Decision

### 1. Manifest is the static activation index

The manifest carries a versioned list of activation events. Examples:

```json
{
  "id": "uuid-generator",
  "activationEvents": [
    "onCommand:uuid.generate"
  ]
}
```

The wire format is defined explicitly. A supported representation is a
string-tagged form such as:

- `onStartup`
- `onCommand:<command-id>`
- `onLanguage:<language-id>`
- `onWorkspaceContains:<root-entry>`

The manifest also carries a command contribution index:

```json
{
  "commands": [
    { "id": "uuid.generate", "title": "Generate UUID" }
  ]
}
```

`onCommand` alone is a routing hint. The command index is what lets the
editor map an unloaded command to exactly one plugin. Command ids are unique
across installed plugins; a conflicting command is rejected at install time,
not resolved by whichever host starts first.

Validation limits include manifest size, event count, command-id length, and
duplicate events. Unknown event kinds are rejected with a manifest error; they
must not silently become a startup trigger.

### 2. Activation events are not capabilities

An activation event answers only: "may the session start this plugin now?"

It never grants `fs.read`, `fs.write`, `sys.fetch`, `sys.exec`, or any future
capability. Capabilities remain explicit grants registered with the gateway and
subject to the same user consent and token checks as a manually started host.

### 3. The gateway stays a capability enforcement point

The session and editor evaluate activation events. The gateway does not parse
commands, language ids, or workspace markers, and does not decide whether an
activation event is satisfiable.

The gateway's responsibilities remain:

- verify peer credentials;
- verify the per-plugin token;
- check the registered plugin id, workspace, and grants;
- reject missing or revoked capability grants.

This avoids a second manifest policy engine and keeps activation policy above
the trust boundary.

### 4. Register, then spawn

The start sequence is fixed:

1. Resolve the manifest and the requested plugin id.
2. Validate the event and the command index.
3. Register `(plugin_id, token_hash, workspace, grants)` with the gateway.
4. Spawn the host with the token FD.
5. Apply sandbox setup.
6. Handshake with the editor and gateway.
7. Mark the plugin ready and dispatch the queued request.

Registration failure means no spawn. Handshake or sandbox failure means kill,
revoke the token, and remove host state.

The reverse order (`spawn`, then register) is rejected. It creates a live
unauthenticated child and a race between plugin execution and gateway
registration.

### 5. One supervisor with single-flight startup

Each plugin has one lifecycle owner and one host. The supervisor implements:

```text
Unloaded -> Registering -> Starting -> Ready -> Draining -> Stopped
                |             |
                +-----------> Failed -> Disabled
```

Rules:

- Two simultaneous triggers for one plugin produce one start.
- Requests that arrive during `Registering` or `Starting` queue until `Ready`.
- A failed start fails only the waiting requests for that plugin.
- A stopped plugin can be started again by a later event with a new token.
- After repeated crashes, the plugin is disabled until the user re-enables it.
- No supervisor may kill or reload a host while it is dispatching a request;
  drain or cancel first.

### 6. Bounded concurrency and bounded idle cost

Global limits are required:

- maximum concurrently running plugin hosts;
- maximum queued starts;
- start timeout;
- per-plugin request timeout;
- memory and descriptor limits already enforced by the host sandbox.

When the host budget is full, starts wait in a bounded queue. They do not spawn
unbounded sandboxes.

Idle reclaim uses process termination, not cgroup freeze:

- `idle` means no in-flight request, no pending activation, no active event
  subscription, and no supervisor-held timer;
- after the idle timeout, terminate the process group, revoke the token, and
  remove the cgroup and runtime socket;
- a cleanup hook is best-effort and bounded; a hung plugin does not delay
  termination;
- in-memory plugin state is lost unless a future scoped state API provides
  persistence.

`cgroup.freeze` is deferred. It stops scheduling but does not release memory or
file descriptors, so it is not an idle-resource policy by itself.

### 7. Event-specific constraints

`onCommand`
- Routes only through the static command index.
- The command is queued, not executed, until the plugin is ready.
- It is not a license for the plugin to register arbitrary new commands before
  loading.

`onLanguage`
- Fires when a buffer with the matching language is opened, not on every key.
- It deduplicates by language and plugin for the session.
- It does not start trusted local language servers; LSP servers remain outside
  the plugin host per ADR-0012 and ADR-0013.

`onWorkspaceContains`
- Checks workspace root entries only, with a bounded optional depth.
- Does not follow symlinks and does not recurse through a large tree.
- Is evaluated once per workspace open.
- Is user-disableable because common markers such as `.git` would otherwise
  defeat lazy loading.

`onStartup`
- Is never inferred from a vague "core plugin" status.
- Requires explicit user consent or a trusted distribution policy.
- Runs after the activation index is loaded, not before the editor is usable.

ADR-0013 distinguishes compiled distribution Rust from sandboxed runtime
plugins, but it defines no "core runtime plugin" trust class. The standard
edition may select default plugins, but that selection does not by itself make
runtime `OnStartup` trusted. Trust must be explicit.

### 8. No pre-warming in the first implementation

Pre-warming would start sandboxes for plugins that have not activated, which
reintroduces the startup cost this ADR removes. The first implementation starts
only on an activation event.

If pre-warming is added later, it must be opt-in, bounded by the same host
budget, and must not initialize untrusted plugin code before activation.

### 9. Deferred capabilities

The following are not part of this decision:

- scoped plugin-state persistence;
- cgroup freeze/resume;
- plugin-provided language servers;
- hot reload;
- a public Rust API for lifecycle control;
- a VM-based runtime.

A scoped state API would be a new capability family and needs its own ADR.
Using unrestricted `sys.write_file` as "state" is rejected because it grants
workspace write access, not isolated plugin storage.

## Lifecycle

```text
idle
  |
  | command / language / workspace / trusted startup
  v
validate manifest and grants
  |
  v
register token with gateway -----> failure: no process, report error
  |
  v
spawn + token FD + sandbox + handshake
  |
  v
ready -> dispatch request -> response
  |
  +---- more requests ----+
  |
  +---- idle timeout ----> kill + revoke + cleanup -> idle
  |
  +---- crash ------------> bounded restart / disable
```

## Verification

- A fake-host integration test registers 50 plugin manifests and triggers one
  command: exactly one host is spawned; the other 49 remain unloaded.
- Two simultaneous triggers for the same plugin produce one spawn.
- An unloaded plugin's command can be routed from the static index.
- Two plugins declaring the same command are rejected at install time.
- Gateway registration failure produces no child process.
- Handshake failure terminates the process group and revokes the token.
- Idle timeout kills the host and a later event starts a new one with a new
  token.
- A plugin with an active subscription or pending request is not reclaimed.
- Repeated crashes disable the plugin instead of looping.
- `onLanguage` starts only the matching plugin.
- `onWorkspaceContains` does not recurse or follow symlinks.
- `onStartup` without consent does not start a runtime plugin.
- Activation never bypasses a missing capability grant.

## Rejected alternatives

### Load every plugin at startup

Rejected: process, memory, fd, and handshake cost grow with installed plugins,
not with actual use.

### Keep every host alive after first use

Rejected: idle plugins still consume memory, descriptors, and attack surface.
LRU-style termination is simpler and safer than freeze for resource reclaim.

### Use only cgroup freeze for idle

Rejected: freeze is not resource reclamation, and the current cgroup wrapper
does not expose it.

### Let the gateway evaluate activation events

Rejected: activation is session policy. Duplicating command/language/workspace
logic in the gateway creates drift and conflicts with the capability boundary.

### Spawn first, register later

Rejected: a live host can execute before its token and grants are registered.

### Treat standard-edition plugins as implicitly trusted `onStartup` code

Rejected: ADR-0013 defines no such runtime trust class. `onStartup` requires an
explicit consent or trust decision.

### Use `sys.write_file` as plugin state persistence

Rejected: it is workspace write access, not scoped plugin state. A scoped state
API is a separate capability family.

## Consequences

Positive:

- Installed plugins do not cost a process until an event needs them.
- The command index makes cold activation deterministic.
- Capability enforcement stays in one place.
- Host counts and idle cost are bounded.
- Startup behavior is explicit rather than implied by "core".

Negative:

- Cold start adds spawn and handshake latency to the first command.
- The manifest and command index must be maintained and validated.
- Idle termination loses in-memory plugin state.
- A future session extraction must own the supervisor; implementing it in the
  TUI would deepen the current boundary debt.

## Open questions

- Exact default host budget, start timeout, and idle timeout.
- Whether `onWorkspaceContains` is available at all in the first release.
- The precise consent UX for `onStartup`.
- Whether a future scoped state capability is needed before idle termination is
  enabled by default.
