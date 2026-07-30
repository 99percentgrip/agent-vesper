# Configuration contracts

## Purpose

Own platform-aware Agent Vesper paths, profile validation, typed application
configuration, provider envelopes, secret references, and persistence ports.

## Local Contracts

- Stage 1 resolves and describes paths but never creates, migrates, or writes user state.
- Legacy Native GLM ACP locations are always read-only descriptors.
- Config, data, cache, state, and logs remain distinct.
- Provider values are opaque envelopes; raw secrets are prohibited.

## Verification

- Run `cargo test -p vesper-config`.
- Run `cargo xtask architecture`.

## Child DOX Index

No children.
