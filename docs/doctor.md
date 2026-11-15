# `anvil doctor`

`anvil doctor [--json] [--profile auto|dev|installed]` checks the installation
and terminal environment before the TUI starts.

## Profiles

- `auto` (default): `installed` when the executable is in a layout with
  `share/anvil` beside `bin`; otherwise `dev`.
- `dev`: missing sidecar binaries are warnings. Use this for `cargo run` and
  development builds.
- `installed`: missing required sidecar binaries are errors. Release installers
  and release CI must use this profile explicitly.

## Required / optional matrix

| Check id | Required | Missing behavior |
|---|---|---|
| `workspace` | yes | error |
| `gateway` | installed profile | error in `installed`; warn in `dev` |
| `anvil-exec-runner` | installed profile | error in `installed`; warn in `dev` |
| `anvil-plugin-host` | no | warn |
| `anvil-ctl` | no | warn |
| `anvil-mcp` | no | warn |
| `locale` | yes, UTF-8 | error |
| `truecolor` | no | warn |
| `runtime-dir` | no, HOME fallback exists | warn |
| `config` | no | warn; explicitly set but missing is error |
| `landlock` | Landlock or seccomp | error only when neither is available |
| `lsp` | no | warn when no supported server is on `PATH` |

`TERM=dumb` is a warning even when `ANVIL_TRUECOLOR` is forced.

## Exit codes

- `0`: no required errors; warnings are allowed.
- `1`: at least one required error.

## JSON schema

Schema version `1` has stable top-level fields and stable check ids:

```json
{
  "schema_version": 1,
  "anvil_version": "0.6.0",
  "profile": "installed",
  "distribution": "anvil",
  "ok": true,
  "checks": [
    {
      "id": "gateway",
      "status": "ok",
      "message": "/home/user/.local/bin/anvil-gateway",
      "remediation": ""
    }
  ]
}
```

`status` is `ok`, `warn`, or `error`. `remediation` is always present and is
empty when no action is needed. Consumers should key on `id`, not message text.
