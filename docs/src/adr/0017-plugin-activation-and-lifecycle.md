# ADR-0017: Plugin Activation, Concurrent Execution, and Resource Isolation

- Status: Accepted; implementation split across `0.4.4`–`0.4.8`; no runtime
  implementation before `0.4.4`.
- Date: 2026-11-01
- Related: [ADR-0001](0001-plugin-isolation.md),
  [ADR-0007](0007-jsonrpc-envelope-and-capability-token.md),
  [ADR-0012](0012-plugin-host-isolation.md),
  [ADR-0013](0013-kernel-distribution-boundary.md),
  [ADR-0002](0002-go-daemon.md),
  [ADR-0006](0006-upstream-dependencies.md),
  [ADR-0011](0011-workspace-chunked-file-io.md)

## Context

The plugin host is currently an empty sandboxed process. Plugin loading and the
capability table are not implemented, and the host is not yet connected to the
gateway. Starting every installed plugin at editor startup would spend
processes, memory, file descriptors, and handshakes on code that may never run.

Activation events are the scheduling layer above the existing isolation
boundary. They must not become a second permission system or a hidden startup
path for untrusted code.

The broader problem is shared execution. A plugin that spins, leaks memory, or
crashes must not be able to freeze the editor loop or corrupt another plugin.
ADR-0001 and ADR-0012 already select a process boundary; this ADR connects that
boundary to activation, scheduling, resource limits, and state synchronization.

The current implementation has an empty sandboxed host, no plugin loading path,
no gateway connection from the host, and cgroup limits for pids and memory only.
CPU weight, plugin snapshots, and gateway-mediated plugin I/O are proposed here,
not implemented.

The activation decisions are ordered by dependency. Concurrency, resource
isolation, and the I/O boundary apply to every stage after host startup.

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
5. A read-only document snapshot and transaction contract exists with a base
   revision and an explicit conflict error.
6. The delegated cgroup v2 root can set CPU weight, `memory.max`, and
   `pids.max`. Missing cgroup support fails closed rather than running without
   the promised limits.
7. Plugin `sys.*` I/O routes through the gateway with the plugin token and grant
   set; the host has no ambient filesystem/network path.

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

## Concurrent Execution and Resource Isolation

### Process-level actor model

- Every plugin, or an explicitly declared trust group, runs in its own
  plugin-host process. The editor never loads Lua/TS code in-process.
- Hosts communicate with the editor and gateway only through Unix domain
  sockets and the JSON-RPC envelope. There is no shared mutable state.
- A plugin receives read-only document snapshots, not a `PieceTable` handle.
  It submits edits as transactions against a base revision.
- Thread-only isolation is rejected: threads share an address space, so a
  segfault, allocator bug, or process-wide OOM can still take down the editor.

### cgroup v2 resource isolation

- Each host is placed in its own cgroup v2 path before plugin code initializes.
- The supervisor sets `memory.max`, `pids.max`, and `cpu.weight`. Memory and
  process limits are hard containment; CPU weight is scheduler priority, not a
  hard quota.
- OOM kills are attributed by cgroup and surfaced as a plugin failure, not as
  an editor failure.
- If the delegated cgroup cannot be created or written, the plugin fails
  closed. It does not run with a promise of limits that are not active.
- The global host budget from section 6 applies in addition to per-host
  limits.

### Kernel defense in depth

- `no_new_privs`, a native-ABI seccomp allowlist, and Landlock rules are applied
  before VM initialization, following ADR-0012.
- The host receives no editor/gateway descriptors, no ambient workspace path,
  and no network access. `sys.*` calls are the only privileged side effects.
- Sandbox setup failure prevents plugin startup; there is no trusted-mode
  fallback.

### Optimistic concurrency

- The editor core remains the single writer for document state.
- A plugin transaction includes the base revision and the targeted buffer id.
  The core validates the base, applies the edit atomically, and returns a new
  revision.
- A stale base revision is rejected with a conflict error. The plugin may
  re-read the snapshot and retry; the core never merges arbitrary concurrent
  edits silently.
- Plugins cannot call internal mutation methods or share a mutable snapshot.
  High-frequency read-only views may be cached by the host.

### I/O stays outside the UI thread

- Plugin filesystem, network, and exec access goes through gateway `sys.*`
  methods under the plugin's token and grants.
- Plugins cannot use ambient `std::fs`, sockets, or subprocesses from the VM.
  The plugin host's own JSON-RPC socket is the only transport it needs.
- The editor and plugin host may perform CPU work concurrently, but the TUI
  event loop remains the only renderer and command dispatcher.
- JSON-RPC serialization is an accepted cost. A future shared-memory path is
  explicitly deferred and must not become an ambient capability.

### Failure containment

- A crash, OOM kill, sandbox violation, or hung request terminates only the
  affected host and fails its pending requests.
- The supervisor applies bounded restart/backoff and disables repeatedly
  crashing plugins instead of creating a restart loop.
- An infinite loop consumes its cgroup CPU share; it must not block the
  editor's main loop or the gateway.

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
- An OOM-killed or SIGKILLed host leaves the editor and other hosts usable.
- An infinite-loop plugin consumes only its cgroup share and does not delay a
  TUI frame or gateway response.
- A transaction with a stale base revision returns a conflict and does not
  partially apply.
- A plugin cannot read, write, fetch, or exec outside its granted gateway
  surface.
- Missing or unwritable cgroup limits prevent the host from starting.

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

### In-process thread isolation

Rejected: threads share memory and process-wide failure domains. A segfault,
allocator corruption, or OOM can still kill the editor, and cgroups apply to
the process, not an individual plugin thread.

### In-process WebAssembly runtime

Rejected as the v1 isolation boundary: it relies on the runtime's own
memory-safety guarantee and gives less direct control over filesystem,
network, and process limits. A Wasm host in a separate process remains a
possible later implementation.

### Stable Rust ABI plugins

Rejected: Rust has no stable ABI; dynamic loading would couple plugin builds to
compiler versions and disable the process boundary. Rust extensions remain
source-level distribution code per ADR-0013.

### Shared memory for the first implementation

Rejected for now: it needs a separate lifetime, synchronization, and security
design. JSON-RPC snapshots are the baseline; mmap and descriptor passing are
future work.

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
- A plugin crash, OOM, or sandbox violation is contained to its host.
- Plugin CPU work can use other cores without coupling to the render loop.
- The core remains the single writer and rejects stale transactions.
- Concurrent edits have explicit conflict semantics instead of silent merges.

Negative:

- Cold start adds spawn and handshake latency to the first command.
- The manifest and command index must be maintained and validated.
- Idle termination loses in-memory plugin state.
- A future session extraction must own the supervisor; implementing it in the
  TUI would deepen the current boundary debt.
- JSON-RPC adds serialization and copy overhead for snapshots and frequent
  events.
- Out-of-process debugging needs dedicated log/trace tooling.
- Concurrent plugin edits need retry/conflict UX and tests.
- Resource enforcement depends on a delegated cgroup v2 root.
- The core still owns serialized document state; this is not a zero-lock design.

## Future Work

- Evaluate shared memory (`mmap`) with descriptor passing for high-frequency
  snapshots after the JSON-RPC path has measured overhead.
- Consider `cpu.max` in addition to `cpu.weight` if scheduler weight is not a
  strong enough fairness guarantee.
- Provide a `cargo anvil-plugin-dev` workflow for out-of-process logs, traces,
  and a local sandbox harness.
- Revisit a process-hosted Wasm runtime without changing the trust boundary.

## Open questions

- Exact default host budget, start timeout, and idle timeout.
- Whether `onWorkspaceContains` is available at all in the first release.
- The precise consent UX for `onStartup`.
- Whether a future scoped state capability is needed before idle termination is
  enabled by default.
- Default `cpu.weight`, `memory.max`, and `pids.max` values, and the maximum
  number of concurrent hosts.
- Conflict retry policy for plugin transactions: immediate retry, user prompt,
  or plugin-specific backoff.
- Whether JSON-RPC snapshot overhead stays within the latency budget at the
  largest supported document size.
- Whether shared memory needs its own ADR before any implementation.
