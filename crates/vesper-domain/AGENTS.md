# Provider-neutral domain

## Purpose

Own stable IDs, messages, content, usage, outcomes, errors, session metadata,
plans, goals, permissions, capabilities, versioned runtime commands/events, and
read/write-free frozen compatibility DTOs.

## Local Contracts

- This crate depends on no workspace crate and performs no I/O.
- No ACP SDK, provider SDK, frontend, transport, or concrete-provider type may
  enter these DTOs.
- Serialized unknown/provider data stays namespaced and opaque.
- Legacy GLM names may appear only in the explicit compatibility module.
- Event sequences are scoped to runtime/session/turn ownership and turn
  terminals are unique.
- Hidden internal chain-of-thought is not a domain content requirement.

## Verification

- Run `cargo test -p vesper-domain`.
- Run `cargo xtask architecture`.

## Child DOX Index

No children.
