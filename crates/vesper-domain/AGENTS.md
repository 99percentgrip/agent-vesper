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
- `src/vro.rs` owns the VRO domain contracts per
  `docs/agent-vesper-reasoning-orchestrator-prd.md`: `ReasoningMode` (§8.1),
  `ReasoningStrategy` (§10.3 — the authoritative 10-variant enum),
  `TaskProfile` (§14.2; `ambiguity` is `f32`, so the struct derives `PartialEq`
  but not `Eq`), `ReasoningBudget` (§10.4 — `u16` for
  `max_parallel_branches`/`max_search_depth`/`max_repairs`), and the
  `ReasoningConfig` `[reasoning]` block (§24). VRO-2.1 added the remaining §14
  data contracts — `ReasoningRequest` (§14.1), `DeliberationArtifact` (§14.3),
  `Candidate` (§14.4), `ReasoningOutcome` (§14.5) — plus `VerificationResult`
  (§10.8), `OutcomeStatus`, `VerificationStatus`, `PrivacyMode`, and the
  `InferenceCost`/`VerificationSummary`/`WorkflowPlanStep` placeholders. VRO-2.2
  upgraded `VerificationFinding` from a `String` placeholder to a real struct
  (`message`/`severity`/`location`) with a `VerificationSeverity` enum, and
  added `VerificationStatus::Error` (the verifier itself could not run, distinct
  from `Failed`). Fields whose types are not yet defined use documented
  placeholders (`String` aliases or `serde_json::Value`); real domain IDs
  (`RequestId`/`CandidateId`/`SessionId`) are reused. `Candidate` and
  `ReasoningOutcome` derive `PartialEq` only (they carry
  `serde_json::Value`/`f32`). Budget preset values pinned by §24 are sourced
  from the PRD; fields §24 does not pin carry documented VRO-1 conservative
  baselines deferred to research phase R3. No orchestration logic lives here.

## Verification

- Run `cargo test -p vesper-domain`.
- Run `cargo xtask architecture`.

## Child DOX Index

No children.
