# Settings sources

Anvil resolves typed settings from these sources, in order:

1. built-in defaults;
2. the user settings file (`ANVIL_SETTINGS`, or
   `$XDG_CONFIG_HOME/anvil/settings.toml` / `$HOME/.config/anvil/settings.toml`);
3. a trusted workspace `.anvil.toml`;
4. environment variables.

CLI flags currently control process behavior (`--workspace`, `--editor`,
`--read-only`, `--truecolor`), not typed settings fields. They are therefore not
shown as a settings source. If a typed CLI override is added later, it must get
its own stable `cli` origin.

## Effective and ignored sources

`:settings` shows the source of the selected field:

- `default`
- `environment`
- the user settings path
- the trusted project settings path

If a project `.anvil.toml` exists but cannot be used, the same line also shows
the ignored path and reason, for example:

```text
project file ignored: /repo/.anvil.toml (workspace root is untrusted)
```

An explicit `ANVIL_SETTINGS` file replaces the project layer; an existing
project file is then reported as ignored with that reason.

`:config-reload` reports the settings files that actually loaded and appends
ignored project files with their reason. A parse failure is reported as an
error containing the file path and parser location; it does not silently
replace the failure with defaults.
