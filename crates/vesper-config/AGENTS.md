# Configuration contracts

## Purpose

Own platform-aware Agent Vesper paths, profile validation, typed application
configuration, provider envelopes, secret references, and persistence ports.

## Local Contracts

- Stage 1 resolves and describes paths but never creates, migrates, or writes user state.
- Legacy Native GLM ACP locations are always read-only descriptors.
- Config, data, cache, state, and logs remain distinct.
- Provider values are opaque envelopes; raw secrets are prohibited.
- `sandbox_config.rs` owns the `[sandbox]` scope demand reader and
  `web_config.rs` the `[web]` opt-in scope reader (VRO-14 PR-5:
  `enabled`/`respect_robots`/`output_budget_bytes`/`allowlist`, plus the
  default-off `interact.enabled` or `[web.interact] enabled` gate); both are
  deliberately dependency-free single-table TOML readers (no `toml` crate —
  see their module docs), missing files yield the inactive default, and
  unknown keys are ignored for forward compatibility.

## Verification

- Run `cargo test -p vesper-config`.
- Run `cargo xtask architecture`.

## Child DOX Index

No children.
