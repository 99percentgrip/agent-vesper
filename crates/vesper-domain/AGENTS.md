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
  from `Failed`). VRO-3.1 added `ModelCapabilities` (PRD §10.2). VRO-10
  (PRD §10.5 + §14.3 + §10.4) closes three final PARTIAL/DEFERRED gaps:
  (1) `WorkflowPlanStep` now carries the five previously-missing §10.5
  fields (`expected_output_schema`, `failure_policy: StepFailurePolicy`,
  `max_attempts`, `parallel_allowed`, `requires_user_approval`) with
  conservative serde defaults so legacy plans deserialize unchanged;
  (2) the VRO-2.1 free-form `String` aliases `Assumption`, `EvidenceRef`,
  `ContextRef` are promoted to **strict newtypes** (`Assumption` carries
  `statement`/`confidence: Option<f32>`/`status: AssumptionStatus`;
  `EvidenceRef` carries `kind: EvidenceKind` + `locator`; `ContextRef` carries
  `kind: ContextKind` + `locator`) — `From<&str>`/`From<String>`/`AsRef<str>`
  impls keep every existing call site compiling, so the type-level strictness
  is gained without a use-site rewrite. `DeliberationArtifact` now derives
  `PartialEq` only (not `Eq`) because `Assumption` carries `Option<f32>`;
  (3) `OutcomeStatus` gains the `RateLimitExceeded` variant (PRD §10.4
  "account for provider rate limits") so the orchestrator halts cleanly on an
  HTTP 429 instead of crashing. Provider names are FORBIDDEN in this file
  (xtask architecture guard scans for them). Budget preset values pinned by
  §24 are sourced from the PRD; fields §24 does not pin carry documented
  Phase R3 calibrated baselines. No orchestration logic lives here.

## Verification

- Run `cargo test -p vesper-domain`.
- Run `cargo xtask architecture`.

## Child DOX Index

No children.
