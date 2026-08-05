# Provider-neutral domain

## Purpose

Own stable IDs, messages, content, usage, outcomes, errors, session metadata,
plans, goals, permissions, capabilities, versioned runtime commands/events,
read/write-free frozen compatibility DTOs, and the Vesper Reasoning
Orchestrator (VRO) Phase VRO-1 domain contracts.

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
- `src/vro.rs` owns the VRO Phase VRO-1 domain contracts per
  `docs/agent-vesper-reasoning-orchestrator-prd.md`: `ReasoningMode` (§8.1),
  `ReasoningStrategy` (§10.3 — the authoritative 10-variant enum),
  `TaskProfile` (§14.2; `ambiguity` is `f32`, so the struct derives `PartialEq`
  but not `Eq`), `ReasoningBudget` (§10.4 — `u16` for
  `max_parallel_branches`/`max_search_depth`/`max_repairs`), and the
  `ReasoningConfig` `[reasoning]` block (§24). Budget preset values pinned by
  §24 are sourced from the PRD; fields §24 does not pin carry documented
  VRO-1 conservative baselines deferred to research phase R3. No orchestration
  logic lives here.

## Verification

- Run `cargo test -p vesper-domain`.
- Run `cargo xtask architecture`.

## Child DOX Index

No children.
