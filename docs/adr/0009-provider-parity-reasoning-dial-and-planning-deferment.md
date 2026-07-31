# ADR 0009: Provider Parity — Reasoning Dial Reconciliation and Planning Deferment

Status: ACCEPTED — **Decision 1 stands; Decision 2 SUPERSEDED by ADR 0010 (Tier C authorization)**

> **Reassessment (Tier C authorization).** Decision 2 deferred model-driven
> plan generation and the agentic tool-execution loop to a future stage.
> That deferment is **overturned by the lead architect** — see
> [ADR 0010](0010-tier-c-agent-loop-and-tool-execution.md). The single-turn,
> tool-free restriction is lifted **at the workspace level**: a new
> `vesper-agent` crate composes `vesper-runtime` into a multi-turn,
> tool-executing agent loop. `vesper-runtime` itself remains the pure,
> provider-neutral single-turn engine (its contract is preserved, not
> violated). The TUI's `/review <body>` placeholder is retired once the
> `update_plan` tool flows through the agent loop into the REVIEW phase.

## Context

A parity audit of `agent-vesper-tui` + `vesper-runtime` + `vesper-provider-glm`
against the frozen Python oracle (`/home/alex/Projects/Native GLM-5.2 Provider`
@ `bf4d4287e2e3320aa3f09015f678e6169d520045`) found two architectural
divergences that would have broken the workspace contract if carried into
production.

**Divergence 1 — The reasoning dial.** The Rust GLM factory advertised two
separate controls — `zai:effort` (`low/medium/high/max`, `scope: Request`) and
`zai:interleaved-thinking` (`scope: Both`) — in
`crates/vesper-provider-glm/src/factory.rs`. The oracle has **one** control:
`session.thought_level` ∈ `{disabled, enabled, high, max}`
(`config.py:591`), mutated live by `/thinking` (`agent.py:2231-2248`) and
applied to the next prompt because `GlmClient` is rebuilt per turn
(`agent.py:711-730`). The oracle has **no `/effort` command** and never sends
`low`/`medium` as `reasoning_effort` (only `high`/`max` are emitted,
`config.py:610,617`). The Rust `low`/`medium` values were therefore invented,
not oracle-derived.

**Divergence 2 — Plan Mode.** The oracle's Plan Mode is **model-driven via an
agentic tool-execution loop**: `PLAN_MODE_PROMPT` is injected into the system
prompt (`agent.py:236`, `457`), and the model emits `update_plan` tool calls
(`agent.py:3537`, `_handle_update_plan` at `4618`) that write `.agent/plan.md`
and push ACP `update_plan` updates (`_send_plan` at `4748`). The Rust runtime
is, by binding contract, **tool-free and single-turn**
(`crates/vesper-runtime/AGENTS.md`: *"Do not depend on tool execution"*;
`supervisor.rs` terminates the turn on `FinishOutcome::ToolCalls`). The TUI's
`/review <body>` placeholder, where the human authors the plan, is therefore a
*different product*, not a stepping stone to oracle parity.

## Decision

### Decision 1 — Reconcile the reasoning dial with the oracle

Collapse `zai:effort` and `zai:interleaved-thinking` into a **single,
session-scoped** reasoning dial matching the oracle's
`{disabled, enabled, high, max}` scale.

- The single superpower is `zai:reasoning` with `command_alias: "thinking"`
  and `allowed_values: {disabled, enabled, high, max}`.
- `low` and `medium` are **explicitly invalid**. They are removed from the
  descriptor and the TUI command surface.
- The dial is **session-scoped**: a `/thinking <level>` command mutates the
  session's reasoning mode and takes effect on the next prompt, mirroring the
  oracle's `session.thought_level`. This is faithful to the source of truth
  and a smaller change than a per-request override envelope.
- The `zai:model` selector (session-scoped) is retained unchanged.

### Decision 2 — ~~Defer model-driven plan generation to a future "Agent Loop" stage~~ SUPERSEDED

> **Overturned by [ADR 0010](0010-tier-c-agent-loop-and-tool-execution.md).**
> The original text below is retained for the record.

~~Model-driven plan generation **cannot** be added to `vesper-runtime` without
violating its DOX contract.~~ ~~It is explicitly deferred to a future stage that
owns:~~

- An agentic tool-execution loop (read tools, `update_plan`, continuation).
- A tool registry with mode-aware permission gating.
- A live ACP `update_plan` emit channel (today `vesper-acp` delivers a plan
  only from persisted replay, `adapter.rs:563-572`, not from a live turn).
- `PLAN_MODE_PROMPT` system injection keyed off `SessionOperatingMode::Plan`.

Until that stage exists, `SessionOperatingMode::Plan` remains a stored label
with no model-driven behavior, and the TUI's `/review <body>` stays labeled as
a non-oracle, human-authored placeholder (per `apps/agent-vesper-tui/AGENTS.md`
Work Guidance).

## Alternatives considered

- **Per-request override envelope on `ProviderRequest`** (carrying
  `SuperpowerOverrides` per turn): rejected as the primary path because the
  oracle is session-scoped and a per-request envelope diverges from the source
  of truth. The existing `ProviderRequest.reasoning` field is still used to
  carry the session's current mode into each turn.
- **Keep `low`/`medium` effort values**: rejected; they are not in the oracle
  (`THOUGHT_LEVELS` has only `disabled/enabled/high/max`).
- **Add tool execution to `vesper-runtime`** for Plan Mode: rejected; it
  breaks the runtime's binding DOX contract and belongs to a dedicated stage.
- **Human-authored plan via `/review` as the parity target**: rejected; the
  oracle never asks the human to author the plan body.

## Consequences

- `vesper-provider-glm` advertises one reasoning dial; the TUI command surface
  drops `/effort` and keeps `/thinking`.
- `vesper-runtime` gains a session-scoped reasoning mode on the session actor,
  mutable via a new `UpdateSessionReasoning` runtime command, threaded into
  every turn's `ProviderRequest.reasoning`.
- Model-driven planning is recorded as **known and owned** (deferred to a
  future stage), not implied. Stage 11b TUI planning remains a UI state
  machine only.

## Compatibility implications

- The `zai:effort` and `zai:interleaved-thinking` descriptor IDs are removed.
  Frontends that matched on those IDs must match `zai:reasoning`. This is
  acceptable because no production frontend outside `agent-vesper-tui` consumes
  the superpower surface yet.
- `ProviderRequest.reasoning` and the GLM serializer's
  `parse_request_reasoning` (`request.rs:262`) are unchanged — they already
  accept `{disabled, enabled/standard, high, max}`.
- Loaded/persisted sessions initialize their reasoning mode to the runtime
  default via `snapshot.reasoning.or(defaults.reasoning)`, preserving today's
  behavior for sessions that predate this field.

## Security implications

- The runtime command carries only an opaque, bounded reasoning-mode label. It
  grants no new filesystem, process, secret, or policy authority. The runtime
  remains tool-free and provider-neutral.

## Migration implications

- `vesper-provider-glm` is the first adapter reconciled. The
  `SuperpowerValue → ReasoningIntent` mapping lives in the GLM crate; a
  provider-neutral `SuperpowerApplier` trait is deferred until a second
  concrete provider needs it.

## Verification requirements

- `cargo test --workspace --all-features` passes.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean.
- `cargo run --package xtask --quiet -- architecture` validates the dependency
  graph (no layering regression).
- An integration test proves `/thinking max` mutates the runtime session
  state and the subsequent `ProviderRequest` carries `reasoning.mode = "max"`
  through to the GLM serializer's `reasoning_effort: "max"`.

## Evidence

- Oracle baseline: `/home/alex/Projects/Native GLM-5.2 Provider` @ `bf4d4287`.
- `THOUGHT_LEVELS` mapping: `glm_acp/config.py:591-620` (the
  `{disabled, enabled, high, max}` scale).
- Wire mapping: `glm_client.py:540-548` (`reasoning_effort` only ever
  `high`/`max`).
- Rust serializer parity: `crates/vesper-provider-glm/src/request.rs:153-168`.
- Runtime no-tool contract: `crates/vesper-runtime/AGENTS.md`.
- Parity audit report (this session).
