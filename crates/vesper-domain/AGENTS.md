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
- `ToolDefinition.defer_loading` is the visibility axis for the Claude
  Code-style deferred-loading seam: when `true`, the tool stays registered for
  execution but is excluded from the registry's `definitions_for(mode)`
  advertisement. The field carries `#[serde(default)]` so existing serialized
  definitions deserialize unchanged; only an explicit caller opts a tool into
  deferred loading.

## Verification

- Run `cargo test -p vesper-domain`.
- Run `cargo xtask architecture`.

## Child DOX Index

No children.
