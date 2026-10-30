# Security Policy

Anvil is pre-alpha. There is no supported release yet, but security reports are
still important.

## Reporting a vulnerability

Do **not** open a public issue for a vulnerability.

Use GitHub's private vulnerability reporting for this repository:

<https://github.com/GBLMX/Anvil/security/advisories/new>

Include:

- affected commit or release;
- a minimal reproduction;
- the impacted trust boundary;
- observed impact and any known workaround;
- whether the issue is already public.

Do not include real credentials, private keys, or personal data in a report.

## Scope

Especially relevant:

- plugin-host escape, sandbox bypass, or direct syscall access;
- `openat2` path escape, symlink race, or workspace boundary bypass;
- `sys.fetch` SSRF, DNS rebinding, redirect, or TLS/SNI confusion;
- `sys.exec` argument, environment, executable, or process-tree escape;
- UDS authentication, token replay, or cross-plugin impersonation;
- manifest permission escalation;
- hot-reload use-after-free, zombie, or orphan-process issues;
- memory-safety issues in Lua/QuickJS bindings or native dependencies;
- secret exposure through logs, diagnostics, or crash reports.

Out of scope:

- a compromised editor binary or kernel;
- a malicious terminal emulator;
- social engineering;
- denial of service from a user intentionally exhausting their own machine;
- issues that require the user to grant a permission that already covers the
  behavior.

## Response targets

These are targets, not guarantees:

- acknowledgement: within 3 business days;
- initial triage: within 7 business days;
- remediation plan: within 30 days for confirmed high-severity issues.

We will coordinate disclosure with the reporter. Please allow time for a fix and
a release before publishing details.

## Safe harbor

Good-faith research on this repository is welcome. We will not pursue legal
action for research that avoids privacy violations, data destruction, and
service disruption. Stop and report if you encounter real user data.

## Supported versions

No stable release is supported yet. Security fixes target the current `main`
branch and, once releases exist, the latest stable minor release.
