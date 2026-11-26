# ADR-0078: Gateway Authorization Scopes, Trust Sources, and Audit

- Status: Proposed
- Version: 1.0.0
- Date: 2026-11-26
- Related: [ADR-0001](0001-plugin-isolation.md),
  [ADR-0002](0002-go-daemon.md),
  [ADR-0005](0005-exec-policy.md),
  [ADR-0014](0014-plugin-api-v1-freeze.md),
  [ADR-0020](0020-content-addressed-plugin-store.md),
  [ADR-0021](0021-signed-plugin-index.md),
  [ADR-0034](0034-release-signing-authority.md),
  [ADR-0038](0038-project-settings-trust.md),
  [ADR-0049](0049-editor-control-plane.md),
  [ADR-0058](0058-sandbox-capability-profiles-and-trust-boundaries.md),
  [ADR-0064](0064-multi-root-routing-and-ownership.md),
  [ADR-0065](0065-provider-protocols-and-execution-classes.md),
  [ADR-0070](0070-ide-extension-contributions-and-execution-trust.md)
- Supersedes:
- Superseded by:

## Context

The gateway is the policy enforcement point for runtime plugins and external
tools, but not the sandbox. Its current policy is a connection-scoped
permission set: `go/internal/gateway/server.go` checks requested permissions
against the effective connection state and returns a generic `policy denied`
error. Package trust (`pkg.trust`) verifies a digest and signer key, but a
valid signature is not a capability review and does not express a publisher
trust policy.

Developers working on local plugins or workspace policy repeatedly hit their
own gateway. The proposed convenience mode creates three risks if implemented
as a single `Strict`/`Standard`/`Developer` switch:

- local path or a workspace-controlled marker could elevate trust;
- a signed package could be treated as reviewed as a whole;
- “warn” for an undeclared capability could become an implicit allow.

Warnings are not authorization decisions. An undeclared capability must
remain denied or require an explicit user decision; it must never become an
automatic grant.

The current ADRs already establish the relevant boundaries:

- [ADR-0058](0058-sandbox-capability-profiles-and-trust-boundaries.md): the
  gateway is policy enforcement, not a sandbox; no silent fallback;
- [ADR-0070](0070-ide-extension-contributions-and-execution-trust.md):
  descriptors and grants are per root; a declaration is not authority;
- [ADR-0038](0038-project-settings-trust.md): project files are untrusted
  input unless the root is trusted;
- [ADR-0005](0005-exec-policy.md): no shell, argv arrays, explicit cwd, and a
  fixed executable policy.

The failure mode is therefore not “too few features”; it is an authorization
model that cannot state who approved what, for which capability, in which
root, for how long, and why.

## Decision

### 1. Model authorization as independent axes

The implementation MUST NOT encode trust, scope, and enforcement as one linear
mode. The axes are:

**Enforcement mode**

```text
enforce | audit
```

**Trust source**

```text
default
signed-publisher
explicit-developer
```

**Grant scope**

```text
once
plugin
workspace
session
```

A UI label may summarize a combination, but the persisted and evaluated
decision retains all three dimensions.

### 2. Keep the secure default

The default remains enforcing. Unknown or undeclared capabilities are denied
or asked; they are never allowed by a warning. `signed-publisher` requires a
publisher/trust-store policy and a declared capability policy; a signature
alone does not grant execution.

### 3. Developer access is an explicit grant, not path detection

`explicit-developer` MUST be bound to:

- the plugin digest;
- the owning workspace root ID;
- the declared capability set;
- a session TTL;
- the user who approved it.

A local filesystem path, a `.anvil` marker, an unsigned package name, or a
workspace file MUST NOT automatically select developer access.

**Undeclared capabilities remain deny/ask under developer access.**
A warning is not an authorization result.

### 4. Project policy may narrow, never expand

A workspace policy file such as `.anvil/policy.toml` is untrusted repository
input. It may:

- change `allow` to `ask` or `deny`;
- constrain a capability to the owning root;
- add workspace-specific denies.

It MUST NOT:

- grant a capability that the user/global policy did not grant;
- broaden a plugin or root scope;
- enable audit mode;
- select developer trust;
- override a hard deny.

Project policy is loaded only through the ADR-0038 trust gate; an explicit
`ANVIL_SETTINGS`-style complete override does not make project policy trusted.

### 5. Grant scopes are bounded

- `allow-once`: bound to a request/operation digest and nonce, expires after
  the operation, and is consumed atomically.
- `allow-for-plugin`: bound to plugin ID and digest; invalidated on digest or
  manifest change.
- `allow-for-workspace`: bound to the canonical workspace root ID, not a raw
  path string; invalidated when trust or root identity changes.
- `session`: bound to the current editor session and TTL.

A grant for one root MUST NOT apply to another root. Symlink and multi-root
resolution use ADR-0064 root identity.

### 6. Policy precedence

At minimum:

```text
hard deny
> explicit user deny
> project deny/ask
> explicit user grant
> signed-publisher + declared capability policy
> default ask/deny
```

The exact evaluator may be factored differently, but a lower-trust source must
not override a higher-trust deny. The evaluated decision and its source are
recorded.

### 7. Audit is a separate, fail-closed mode

Audit is an enforcement mode, not a trust source. It:

- is available only in an interactive user session;
- is explicitly selected, never enabled by a project policy;
- has a bounded TTL and a visible status-bar indicator;
- records every allow/deny/ask decision;
- does not disable the plugin host, Landlock, seccomp, PTY, or exec policy;
- fails closed when the audit sink cannot be written.

The audit record MUST include at least:

```text
timestamp
session_id
actor
workspace_root_id
plugin_id
plugin_digest
capability
operation/request digest
decision
policy_source
grant_scope
result
```

Secrets, credentials, and sensitive command bodies MUST be redacted. The log is
append-only, user-only (`0600`), rotated, and tamper-evident. The query surface
is `:gateway audit` or equivalent; `:pkg audit` remains package-signature
metadata, not the gateway audit log.

### 8. The prompt is trusted UI

The approval prompt is rendered by the trusted TUI overlay, not by plugin
output. It shows:

- plugin ID and digest;
- owning root;
- capability;
- exact operation/arguments summary;
- why it was denied/asked;
- policy source;
- expiry/scope.

Actions are:

```text
allow once
allow for plugin
allow for workspace
deny
show policy
```

There is no “allow all” action. The prompt cannot be spoofed by terminal escape
sequences from plugin output.

### 9. Headless sessions fail closed

If no trusted interactive prompt is available, an `ask` decision MUST resolve
to `deny` unless an existing matching grant or policy allow already covers the
exact request. A headless client, plugin, or gateway MUST NOT synthesize an
approval, wait indefinitely for a user, or treat a timeout as an allow.

The denial is recorded as `decision=deny` with a reason such as
`no-interactive-ui`. External clients must preconfigure policy or grants before
the operation; otherwise the RPC returns a structured approval-required or
policy-denied error rather than hanging.

### 10. Capabilities stay typed and narrow

The gateway continues to use typed capabilities and argv arrays. No wildcard
capability, shell string, `exec: *`, or implicit capability inheritance is
introduced. `sys.exec` continues to resolve to an explicit executable, cwd,
environment allowlist, timeout, and process-group cleanup under ADR-0005.

### 11. Storage and invalidation

User grants live in the user state directory, not in the repository. Project
policy is read-only repository input. Grants are invalidated by:

- plugin digest or manifest permission change;
- root trust or canonical root change;
- policy schema/version change;
- TTL expiry;
- explicit revocation.

The UI and `doctor` report the effective trust source, grant scope, audit
state, and policy source.

## Consequences

Positive:

- Developer friction is addressed without disabling the gateway.
- Every allowance has an identifiable author, scope, and expiry.
- Signed packages remain distinct from reviewed capabilities.
- Project policy can be versioned and reviewed without becoming a privilege
  escalation channel.
- Audit mode is useful without silently removing the sandbox.

Negative:

- More state and UI than a single mode switch.
- Grant storage, revocation, and audit retention need tests and maintenance.
- Workspace policy changes may require explicit user approval.
- `allow-for-workspace` needs stable root identity across multi-root and
  symlink cases.

## Verification

- Focused tests:
  - default mode denies/asks an undeclared capability;
  - `explicit-developer` does not auto-allow an undeclared capability;
  - `signed-publisher` still requires a capability policy;
  - project policy cannot grant beyond user/global policy;
  - allow-once is single-use and request-digest-bound;
  - allow-for-plugin invalidates on digest change;
  - allow-for-workspace is root-scoped and survives only valid root identity;
  - audit records every decision and blocks on an unwritable sink.
  - headless with no interactive UI turns `ask` into `deny` and does not hang;
  - headless with an existing matching grant or policy allow proceeds;
  - the headless denial records `reason=no-interactive-ui`;
- Failure/cancellation tests:
  - concurrent allow-once requests do not consume the same grant;
  - symlink and multi-root paths cannot escape the approved root;
  - denied operations leave no effective session grant;
  - audit log failure leaves the operation denied.
  - a headless timeout is never converted into an allow.
- Real surface smoke:
  - a local unsigned plugin with an explicit developer grant can run a declared
    capability without repeated prompts;
  - an undeclared capability still prompts or is denied;
  - `:gateway audit` finds the corresponding decision;
  - the status bar shows developer/audit scope;
  - `doctor` reports the same policy source.

## References

- [OWASP ASVS V16: Security Logging and Error Handling](https://cornucopia.owasp.org/taxonomy/asvs-5.0/16-security-logging-and-error-handling)
- [NIST SP 800-207: Zero Trust Architecture](https://csrc.nist.gov/pubs/sp/800/207/final)
- [ADR-0058](0058-sandbox-capability-profiles-and-trust-boundaries.md)
- [ADR-0070](0070-ide-extension-contributions-and-execution-trust.md)
