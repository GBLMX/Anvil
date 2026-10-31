## What changed

<!-- One or two sentences. Link the issue or ADR when applicable. -->

## Security impact

<!--
Describe changes to trust boundaries, permissions, IPC, sandboxing, file/network
access, or dependency surface. Write "none" only when that is true.
-->

## Verification

<!-- Exact commands run and the observed result. Do not report plausible output. -->

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace --all-targets`
- [ ] `cargo deny check`
- [ ] Go formatting, vet, tests, lint, and vulnerability scan
- [ ] Documentation/help updated for user-visible behavior

## Compatibility

<!--
Mention changes to kernel API, IPC, plugin API, configuration, keymaps, or
supported versions. If an interface was frozen, state the version.
-->

## Checklist

- [ ] The change is scoped and has no unrelated refactors.
- [ ] New security invariants have regression tests.
- [ ] No credentials, local caches, profile data, or benchmark output are committed.
- [ ] A sandbox or performance claim is backed by a repository check.
