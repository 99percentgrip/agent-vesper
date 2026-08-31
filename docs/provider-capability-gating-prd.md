# PRD — Dynamic Capability-Driven Feature Gating (Provider- & Model-Routed UI)

Status: Accepted, implementation in progress
Target release: v0.20.59
Owner: Agent Vesper TUI (composition) + `vesper-provider-glm` (adapter surface)
Related: ADR 0009 (reasoning dial), ADR 0010 (Tier C agent loop),
`docs/agent-vesper-reasoning-orchestrator-prd.md` (PRD precedent),
`crates/vesper-provider/src/superpowers.rs` (`SuperpowerPolicy`),
`crates/vesper-provider/src/capability.rs` (`SupportLevel`).

## 1. Problem

The TUI's provider feature surface is **static**. Feature controls and feature
gating are decided by a hardcoded provider-name check (`is_glm`) and by direct
calls into `vesper_provider_glm::GlmCatalog` from the frontend. A feature that
the active provider or active model cannot honor is never *disabled* — it is
either rendered as if available (and fails later, sometimes with another
provider's branded error text) or hidden behind a name check that ignores what
the provider actually advertises.

The user-facing symptom (reported against the provider panel): *“you added
providers into the panel but it's not dynamic — if a feature is not available
for a specific model or provider, that feature should be disabled.”*

### Current-state evidence

| # | Site | Defect |
|---|------|--------|
| E1 | `apps/agent-vesper-tui/src/main.rs:2421` | `/settings` splices `/plan`, `/thinking`, `/model`, `/generation`, `/auxiliary`, `/mixture` behind `if is_glm` — a provider **name** check, not an advertisement check. LM Studio advertises `model` + `thinking` (`apps/agent-vesper-tui/src/lmstudio_provider.rs:389-434`) but they never appear. |
| E2 | `main.rs:2460-2525` | `/plan`, `/generation`, `/mixture` value choices are hardcoded `vec![]`s behind `if is_glm`; not derived from any descriptor or policy. |
| E3 | `main.rs:2516-2520` | `/auxiliary` choices filter through `GlmCatalog::supports_plan` / `GlmCatalog::is_vision_model` — GLM business logic executing inside the TUI frontend. |
| E4 | `main.rs:4095-4101` | Image-paste gate calls `GlmCatalog::is_vision_model` on the **active** model whatever the provider, and emits GLM-branded guidance (“select Standard/BigModel and GLM-5V-Turbo…”) even when LM Studio is active. |
| E5 | `main.rs:4131-4132` | Mixture adviser selection iterates the GLM static catalog with GLM predicates; a non-GLM provider can never field advisers, and the failure mode is a GLM error, not a disabled control. |
| E6 | `lmstudio_provider.rs:363` | LM Studio descriptors carry `ProviderCapabilities::default()` (every capability `Unknown`); nothing downstream consumes `ModelDescriptor.capabilities` at all. |
| E7 | `crates/vesper-provider-glm/src/factory.rs` (`glm_superpowers()`) | Only `zai:reasoning` + `zai:model` are advertised; `plan` / `generation` / `auxiliary` have no descriptors, so no descriptor-driven panel can exist for them. |
| E8 | `crates/vesper-provider-glm/src/policy.rs:68` | `GlmSuperpowerPolicy` already implements exactly the desired dynamic rules (`/model` filtered by plan, `/thinking high|max` only on deep-reasoning models, model-change cascades) and is registered (`main.rs:504`) — but the static paths above bypass it. |
| E9 | `crates/vesper-runtime/src/registry.rs` | The registry exposes `superpowers()`, `descriptor()`, `superpower_policy()` but no `ModelCatalog` accessor; per-model capabilities have no runtime route today. |
| E10 | `crates/vesper-provider/src/capability.rs` (`resolve_support`) | Fail-closed precedent already codified: `Unknown` + `Require` → `Reject`. The UI must adopt the same stance. |

## 2. Goals

1. **G1 — Panel is advertised-driven.** Every provider feature entry in
   `/settings` and every value palette derives from the active provider's
   advertised `SuperpowerDescriptor`s (by `command_alias`) and the active
   model's advertised `ProviderCapabilities`. Zero provider-name checks
   (`is_glm` and equivalents) in frontend logic.
2. **G2 — Unsupported features are disabled, not silent.** A feature the
   active provider does not advertise is **hidden**; a feature that is
   advertised but not available for the active model is rendered **disabled
   with a truthful, provider-neutral reason**; a value that is not valid for
   the current session state is **not offered**.
3. **G3 — Provider-routed, fail-closed gating.** Vision/image input, mixture
   advisers, auxiliary-model selection, and thinking levels consult
   capability/policy surfaces owned by the active provider's adapter.
   Missing (`Unknown`) or absent capability data denies the feature
   (mirrors E10). Nothing is guessed.
4. **G4 — GLM logic lives only in the GLM adapter.** Every
   `vesper_provider_glm::` reference disappears from TUI frontend logic;
   concrete adapters appear only at the composition boundary (registration,
   configuration, catalog-source construction), per `apps/AGENTS.md`.

## 3. Non-goals

- Adding new providers, models, plans, or controls (project contract: no
  invented provider surface).
- Registry/runtime API redesign: `ProviderRegistry` gains no new accessor in
  this PRD (rationale in §6).
- LM Studio capability advertisement beyond **verified** API fields
  (see P5 evidence gate; unverified fields stay `Unknown`, which disables
  the feature honestly).
- ACP-side (`agent-vesper-acp`) UI changes; the ACP `/model` picker already
  routes through descriptors.
- VRO reasoning-override surface (`/reasoning set mode=…`) — orthogonal and
  untouched.

## 4. Terminology

- **Advertisement** — what a provider's factory publishes:
  `SuperpowerDescriptor`s, `ProviderDescriptor`, `SuperpowerPolicy`,
  `ModelCatalog` snapshots with per-model `ProviderCapabilities`.
- **Frontend** — `agent-vesper-tui` library modules (`lib.rs`, `commands.rs`,
  `dispatch.rs`, `superpowers.rs`, `ui.rs`, `capabilities.rs`) plus the
  binary's palette/gating functions. Provider-neutral by contract.
- **Composition boundary** — `main.rs` startup/registration wiring where
  concrete adapters may be named (`apps/AGENTS.md`).
- **Fail-closed** — `SupportLevel::Unknown`, missing model entry, or absent
  descriptor ⇒ feature denied/disabled with a truthful reason.

## 5. Requirements

### Functional

- **FR-1 Settings panel.** The `/settings` list is assembled from the active
  surface's descriptors: for each advertised alias in
  `{plan, thinking, model, generation, auxiliary, mixture}` a row appears;
  aliases the provider does not advertise produce no row; aliases advertised
  but ineligible for the active model render disabled with reason text.
- **FR-2 GLM descriptor completion.** `glm_superpowers()` additionally
  advertises session-scoped choice descriptors for `plan`
  (`zai:endpoint-plan`: coding/standard/bigmodel), `generation`
  (`zai:generation-profile`: balanced/precise/exploratory), and `auxiliary`
  (`main` + eligible catalog models). Choice validity rules (plan support,
  vision exclusion for auxiliary) move into `GlmSuperpowerPolicy`
  (`valid_choices`/`validate`), out of the TUI.
- **FR-3 Policy-filtered choices.** Every value palette for an advertised
  alias passes `SuperpowerPolicy::valid_choices(alias, advertised,
  active_plan, active_model)` using the session's registered policy
  (`TuiSession.policy`), and dispatch rejects invalid values through
  `SuperpowerPolicy::validate`.
- **FR-4 Capability index.** A pure, provider-neutral
  `ModelCapabilityIndex` (new `capabilities.rs` in the TUI lib) holds the
  active provider's `ModelDescriptor` list and answers fail-closed queries:
  `accepts_image(model, media_type)`, `tools(model)`,
  `reasoning_effort_levels(model)`, `adviser_candidates(active_model)`,
  `is_known(model)`.
- **FR-5 Vision gating.** Queued images dispatch only when
  `accepts_image(active_model, media_type)` succeeds for every queued image;
  otherwise the turn is rejected before the provider call with the denial
  reason (e.g. “model `qwen3-8b` does not report image support”). No
  provider-branded strings.
- **FR-6 Mixture gating.** Advisers come from `adviser_candidates(active_model)`
  narrowed by the active provider's policy (`valid_choices("mixture", …)`,
  e.g. GLM excludes vision models from text-adviser duty and applies plan
  availability); when fewer than one eligible adviser remains, `/mixture`
  offers/accepts only `off` and enabling it is rejected with a truthful
  reason (e.g. single-model provider).
- **FR-7 Auxiliary gating.** `/auxiliary` values come from the `auxiliary`
  descriptor filtered by policy; when no auxiliary model is eligible the
  control renders disabled with `main` only.
- **FR-8 Thinking gating.** `/thinking` values come from the descriptor
  filtered by policy against the active model (GLM: `high`/`max` only on
  deep-reasoning models; LM Studio: exactly its advertised values).
- **FR-9 Provider switch.** Switching providers rebuilds surface, policy,
  and capability index together, so every panel/palette reflects the new
  provider without restart.
- **FR-10 LM Studio truthfulness.** LM Studio descriptors fill capability
  fields **only** from API-verified data (P5); otherwise fields stay
  `Unknown`/`Unsupported` with honest reasons. The hardcoded
  disabled/enabled/high thinking list is replaced by capability-derived
  values only when evidence supports each value.

### Contracts

- **C-1 Fail-closed.** `Unknown`, missing entry, or absent descriptor never
  enables a feature (E10 precedent).
- **C-2 Provider-neutral text.** No user-visible string names another
  provider's models/plans when the active provider differs.
- **C-3 Neutrality of the lib.** No `vesper_provider_glm` import in any TUI
  library module; binary references to concrete adapters are confined to
  composition wiring (registration, `provider_configuration_for`,
  `model_id_for_provider`, catalog-source construction).
- **C-4 No invented capability.** LM Studio capability fields require
  recorded evidence (primary documentation or captured response) filed in
  this PRD before advertisement.
- **C-5 Existing behavior preserved.** GLM sessions keep today's effective
  behavior (plan/model/thinking/auxiliary/mixture semantics, image support
  for vision models) — this PRD changes the *routing* of decisions, not the
  GLM rules themselves; GLM regression tests must stay green.
- **C-6 Gates stay green.** `cargo xtask architecture`,
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo test --workspace --all-features` (workspace CI gates) at every
  phase boundary.

## 6. Design decisions

- **D1 Capability source at the composition boundary (registry unchanged).**
  The TUI binary builds the `ModelCapabilityIndex` for the active provider at
  startup/switch: GLM from `GlmCatalog::snapshot()`; LM Studio from its
  existing `/models` fetch. Rationale: `ProviderRegistry` has no catalog
  accessor (E9); adding one requires erased-catalog plumbing in
  `register_with_all` for two call sites, while `apps/AGENTS.md` already
  permits concrete wiring at the composition boundary. The lib stays neutral
  (C-3). If a third real provider arrives, revisit a registry accessor.
- **D2 One mechanism for session controls.** `plan`/`generation`/`auxiliary`
  become superpower descriptors (FR-2) so the whole panel uses a single
  descriptor+policy pipeline; harness-native controls (`mixture`) remain
  TUI-owned but their eligibility/choices are capability-driven (FR-6).
- **D3 Hidden vs disabled.** Not-advertised ⇒ hidden (it is not a feature of
  this provider). Advertised-but-ineligible ⇒ visible + disabled + reason
  (the user must see *why*). Value-level invalidity ⇒ value omitted.
- **D4 Denial reasons are bounded strings** built from the denial site, never
  from raw provider payloads; `SafeMessage` reasons from `Unsupported`
  levels are surfaced verbatim (they are adapter-authored and bounded).
- **D5 Index stays capability-pure; provider rules live in the policy.**
  `adviser_candidates` filters only on advertised tool support and non-active
  identity. Provider-specific narrowing — GLM excluding vision models from
  text-adviser duty and applying plan availability — is applied by routing
  the candidate list through the active provider's
  `SuperpowerPolicy::valid_choices("mixture", …)`, preserving today's GLM
  adviser set exactly (C-5) without leaking GLM predicates into the index.

## 7. Phases

Each phase ends compiling with its tests green (no dark phases). Later phases
add regression tests so earlier guarantees cannot silently regress.

### P0 — PRD + DOX (this document)
- Scope: this PRD; `docs/AGENTS.md` ownership line for root PRD files.
- AC: docs land; no code change.

### P1 — Capability index module (TUI lib)
- Scope: new `apps/agent-vesper-tui/src/capabilities.rs`:
  `ModelCapabilityIndex`, `CapabilityDenial`, fail-closed query methods
  (FR-4), `#![forbid(unsafe_code)]`-consistent pure code, unit tests with
  synthetic `ModelDescriptor`s (Native/Emulated/Unsupported/Unknown/
  missing-model/active-model-exclusion).
- AC: `cargo test -p agent-vesper-tui --lib` green including new tests;
  no behavior change elsewhere yet; C-3 holds (module imports only
  `vesper-provider`/`vesper-domain` types).

### P2 — GLM adapter descriptor + policy completion
- Scope: `factory.rs` advertises `plan`/`generation`/`auxiliary`
  descriptors (FR-2); `GlmSuperpowerPolicy` gains the auxiliary-eligibility
  and plan/generation choice rules (moves E3's logic into the adapter);
  adapter tests extended (allowed values, plan filtering, vision-model
  exclusion from auxiliary, `main` always present).
- AC: `cargo test -p vesper-provider-glm` green; descriptors visible via
  `ProviderSuperpowers::superpowers()`; C-5 unchanged GLM behavior.

### P3 — Dynamic panel + choice routing (TUI frontend)
- Scope: `/settings` assembly and all value palettes in `main.rs` /
  `commands.rs` switch to descriptor+policy+capability sources (FR-1/3/7/8);
  `is_glm` splice, hardcoded `vec![]` lists (E1/E2), and the
  `GlmCatalog`-filtering in `/auxiliary` (E3) are deleted; disabled-with-
  reason rendering for ineligible controls (D3); regression tests use a fake
  surface/policy/index (vision model vs text-only model vs single-model
  provider).
- AC: no `vesper_provider_glm::` in frontend paths; tests prove
  hide/disable/omit in all three scenarios; existing palette tests
  (`palette_*` in `main.rs` tests) stay green; `--bins` tests green.

### P4 — Dispatch-time gating + provider switch
- Scope: `spawn_agent_turn` image gate via `accepts_image` (FR-5, replaces
  E4); mixture advisers via `adviser_candidates` + off-only when empty
  (FR-6, replaces E5); auxiliary dispatch validation via policy (FR-7);
  provider switch rebuilds surface+policy+index (FR-9).
- AC: fake provider without vision rejects paste with the model-named
  neutral reason; single-model provider locks `/mixture` to `off`; GLM
  vision-model image flow unchanged (C-5); switch test rebuilds all three
  surfaces; `cargo test -p agent-vesper-tui --lib --bins` green.

### P5 — LM Studio truthful capabilities (evidence-gated)
- Scope: verify LM Studio model-list capability fields against primary
  documentation (lmstudio.ai docs / captured `/api/v0/models` response);
  record the evidence (URL or captured JSON shape) in this PRD; map only
  verified fields (e.g. vision/tool flags if and only if documented) into
  `ProviderCapabilities`; thinking descriptor values derived from verified
  data only; everything else stays `Unknown` (C-4).
- AC: evidence link recorded here; LM Studio descriptors never claim an
  unverified capability; TUI shows LM Studio features disabled with honest
  reasons for unadvertised capabilities; no fabricated fields (C-4).
- Gate: if verification fails (docs unavailable/inconsistent), ship with
  fail-closed `Unknown` and mark the advertisement sub-task deferred here —
  never invent.
- **Status: SHIPPED.** Evidence: LM Studio developer docs, canonical
  source `https://raw.githubusercontent.com/lmstudio-ai/docs/main/1_developer/2_rest/list.md`
  (also served at <https://lmstudio.ai/docs/developer/rest/list>):
  `GET /api/v1/models` → `models[]` with `type` (`"llm" | "embedding"`),
  `key`, `display_name`, `max_context_length`, and optional
  `capabilities: { vision: bool, trained_for_tool_use: bool,
  reasoning: { allowed_options: ("off"|"on"|"low"|"medium"|"high")[],
  default } }`; the changelog
  (<https://lmstudio.ai/docs/developer/api-changelog>) confirms the same
  capability data on the v0 API. Implemented in
  `apps/agent-vesper-tui/src/lmstudio_provider.rs`
  (`native_models_url` / `capabilities_from_native` /
  `snapshot_from_native` / `refresh_catalog`); embedding models are
  skipped; unreported fields stay `Unknown`. **Honesty fix:** the previous
  unconditional LM Studio thinking dial (`disabled`/`enabled`/`high`) never
  reached the wire (no reasoning field exists on the OpenAI-compatible
  chat request we send) — it is removed. A thinking dial is advertised only
  when the pinned model reports `reasoning.allowed_options`, with those
  exact labels. Reasoning *display* (streamed `reasoning_content`) is
  response telemetry and is unaffected.

### P6 — Sweep + workspace verification
- Scope: audit for stray `vesper_provider_glm`/provider-name checks in TUI
  lib; rustfmt; `cargo xtask verify` (fmt + clippy `-D warnings` + tests +
  architecture). Update `docs/migration-status.md` if contracts change.
- AC: all workspace gates green (C-6); audit finds zero violations.

### P7 — DOX + README
- Scope: update `apps/agent-vesper-tui/AGENTS.md` (ownership entries for
  `capabilities.rs`; contract lines for dynamic gating),
  `crates/vesper-provider-glm/AGENTS.md` (new descriptors/policy rules),
  README feature table + dynamic-gating description (standing preference).
- AC: DOX chain current; README reflects dynamic gating.

### P8 — Release v0.20.59
- Scope: surgical version bump (workspace `Cargo.toml` **and**
  `registry/agent.json` together; only `vesper-*` pins), `cargo build
  --workspace` **before** tagging, explicit-path staging (never
  `git add -A`; never stage `.agent-vesper/`, `.glm-acp/`, `.agent/`),
  tag `v0.20.59`, push, verify **all four** CI workflows on the **final
  HEAD SHA** (re-check after any follow-up commit), local install via
  `scripts/install.sh`, Registry PR #539 updated in place (PUT to branch
  `agent-vesper/v0.20.51` on fork `99percentgrip/registry`, PATCH title/
  body; never close-and-replace). Shipped text contains no oracle real name.
- AC: release published; local binaries report `0.20.59`; Registry PR
  updated; CI green on HEAD.
- **Status: SHIPPED** (HEAD `250abdc`; 1140 tests green; 4 CI workflows
  success; Registry PR updated; local install verified).

### P9 — Final audit: ACP-side dynamic gating (the PRD's deferred item)
- **Audit findings (the §3 deferral note "picker already descriptor-driven"
  was wrong):**
  - **A1** `multi_provider_control_surface` appended the frozen GLM control
    set for EVERY acting provider — a Zed footer on LM Studio advertised
    GLM models/plans/thinking/generation/auxiliary/mixture.
  - **A2** Functional misroute: picking a GLM model while LM Studio acted
    wrote `zai:model` into the lmstudio envelope and
    `session_model_override` returned `lmstudio:<glm-model>` → next turn
    dispatched a nonexistent model to the local server.
  - **A3** `context_window` used `glm_context_window` for every provider —
    LM Studio sessions reported GLM's frozen 1M to the Zed token counter.
  - **A4** The ACP's `lmstudio_provider.rs` is a separate adapter copy that
    missed all of P5: `ProviderCapabilities::default()` (all `Unknown`),
    the unbacked `disabled/enabled/high` thinking dial, no native catalog.
  - **A5** (pre-existing, found during audit) `tests/provider_selection.rs`
    failed outside `--all-features` because the synthetic boot token is
    feature-gated in the composition but the test binary was not.
- **Fix (v0.20.60):** `controls.rs` advertises the ACTING provider's
  controls only (GLM full set for `zai`; a truthful live-catalog `model`
  picker — pinned model fallback — for `lmstudio`; nothing else), rejects
  GLM-only selections fail-closed while another provider acts, and derives
  `context_window` from the acting provider (LM Studio advertised
  `max_context_length`, conservative 8K floor when unadvertised). P5 was
  ported to the ACP adapter copy (native `/api/v1/models` fetch, verified
  schema mapping, truthful capabilities, catalog-driven superpowers without
  the unbacked dial, shared catalog cache across the registered factory
  clones, boot-time refresh when LM Studio acts first). The A5 test is
  `integration-test-harness`-gated (canonical `--all-features` run
  unchanged).
- **Race audit:** the new catalog cache is boot-refresh-then-read-only
  (`Arc<RwLock<..>>`, short critical sections, no reentrancy); the ACP
  surface is immutable post-construction with pure closures over runtime
  snapshots; the TUI capability index is built once with synchronous
  pre-spawn gates. No new race conditions.
- **Known residual (documented, not a misroute):** the surface is baked at
  construction (SDK shape), so a mid-session zai→lmstudio switch keeps the
  GLM rows visible in the client until the next session/new — their
  selections are rejected fail-closed meanwhile; and LM Studio
  model/thinking picks update the engine's acting model while the wire
  request still carries the factory-pinned model id (pre-existing adapter
  shape shared with the TUI path).
- AC: ACP lib+process suites green under `--all-features`; plain
  `cargo test -p agent-vesper-acp` green; workspace 1148/0; shipped as
  v0.20.60 (HEAD `4460b86` — tag re-pointed after the c4b563a staging miss;
  clippy `-D warnings` 0 errors; architecture 22 packages; 4/4 CI workflows
  success on the shipped SHA; local binaries verified at `0.20.60`).

## 8. Test plan

- **Unit (P1):** index fail-closed matrix — Native (accept),
  Unsupported (reason verbatim), Unknown (deny), missing model (deny),
  media-type mismatch (deny), adviser exclusion of active model,
  effort-levels extraction (deep vs base vs unsupported).
- **Adapter (P2):** descriptor presence/aliases/allowed values; policy
  `valid_choices` for model/plan/auxiliary/generation/thinking incl.
  vision-model exclusion and `main` always eligible; `validate` rejections.
- **Frontend (P3):** settings rows hidden vs disabled vs enabled across
  three fake providers (rich catalog, text-only single model, minimal);
  palette filtering; disabled entries not selectable.
- **Dispatch (P4):** image rejection reasons; mixture off-only; auxiliary
  validation; provider-switch rebuild; GLM regression suite unchanged.
- **Workspace (P6):** the four CI-equivalent gates.

## 9. Risks

| Risk | Mitigation |
|------|------------|
| 12k-line `main.rs` edit surface | phases isolate changes; every phase compiles + tests; reads (not grep) verify call sites after the tool's false-negative episodes this session |
| Palette/dispatch tests currently pass concrete `GlmSuperpowerPolicy` | acceptable in binary tests (composition); production path must use `TuiSession.policy` — enforced by P3 sweep + review |
| LM Studio capability verification unavailable | P5 evidence gate; fail-closed `Unknown` ships instead (C-1/C-4) |
| GLM behavior drift | C-5: GLM adapter + TUI GLM regression tests must stay green unchanged |
| Version-bump collateral | surgical `vesper-`-pin bump + `registry/agent.json` together + pre-tag workspace build (P8) |

## 10. Deferred (with evidence requirement)

- Registry-level `ModelCatalog` accessor — revisit at third real provider.
- LM Studio `ProviderCapabilities` beyond P5-verified fields.
- ~~ACP-side dynamic gating audit~~ — CLOSED by P9 (the audit found and
  fixed real gaps; see P9).
- Mid-session surface rebaking after a provider switch (SDK
  `SessionControlSurface` is baked at construction; fail-closed rejection
  covers the window) and wire-level model selection for the LM Studio
  adapter (engine acting model updates; the request still carries the
  pinned id) — both documented in P9.

## 11. Capability-aware switch suggestions (vision-first extension)

- `ModelRequirement`, `ModelCandidate`, and `CapabilitySuggestion` are bounded,
  provider-neutral DTOs. Candidate lists cap at three and cannot cross the
  active provider identity.
- `vesper-provider` owns the catalog-backed capability index/advisor port and
  bounded full-payload scan. Unknown capability fails closed without a
  fabricated candidate.
- GLM resolution lives in `GlmCapabilityAdvisor`, reads `GlmCatalog`, and
  filters by the active endpoint plan. Generic hosts never name or infer a
  specific GLM alternative.
- The TUI checks the preserved composer payload and history, presents an
  Up/Down/Enter/Esc consent picker, applies the choice through the existing
  session configuration path, then sends the preserved text and images.
- `AgentLoop` scans complete compacted outbound history before every request.
  Tool-returned images use the same content channel, and manual downgrade
  never strips older images.
- Adapter-classified unsupported-content errors carry the same typed
  requirement and map to the same outcome only before visible output.
- ACP preserves mixed and image-only protocol content and reports the active
  model plus catalog candidates; switching remains the user's action through
  the existing selector. Multimodal turns in both hosts use direct AgentLoop
  because the current VRO candidate interface is text-only.

Evidence is synthetic/loopback only: provider fail-closed/candidate-bound
tests, GLM catalog/plan tests, image-history refusal, and tool-image refusal
before the next provider dispatch.
