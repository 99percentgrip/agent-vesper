# PRD — VRO-16: Advanced Hive Governance

| Field | Value |
|---|---|
| Status | **COMPLETE — all three PRs implemented and locally verified.** PR-1 (§2.3), PR-2 (§3.3) and PR-3 (§4.3) acceptance criteria verified locally: every `HostCommand` variant, injected-clock timeout, barrier parallelism, restart, naming-guard extension, both-host rendering, settings round-trip; cap enforcement with audited exhaustion, panel snapshot isolation and zero-panel failure, judge/author hash-distinctness, cross-version evidence retrieval, dangling-rationale fail-closed; trace/digest fail-closed proofs, tampered-digest rejection with audit, budget 50/80 emissions and 100% hard-stop with truthful partial state, full-pipeline audit chain, per-host surface proofs (shared harness pipeline e2e + real ACP process `/swarm audit`). Evidence baseline: workspace all-features 2179/0, default 2107/0, clippy clean, `cargo xtask acceptance` 20/20. Exact-commit release gates apply unchanged when a release is cut; live-provider swarm effectiveness is not measured by these offline proofs. |
| Directive | VRO-16 (advanced hive governance PRD) |
| Recon evidence | `docs/architecture/recon_vro16_governance.md` — twelve mechanisms extracted from two external upstreams, each mapped to existing Vesper primitives. |
| Naming rule | The two external upstreams are referenced exclusively as **governance alpha** (Go team-CLI upstream, AGPL-3.0, pinned `ba8b6937…`) and **governance beta** (Python research-pipeline upstream, MIT, pinned `be4ba475…`). No upstream brand names, URLs, or branded file/command identifiers appear in this document, the reconnaissance document, AGENTS.md files, README, or source. PR-1 extends `cargo xtask naming-guard` with the new tokens following the VRO-15 pattern (§5.1). The alias pair `alpha`/`beta` follows the `web oracle alpha/beta/gamma` precedent in `docs/web-oracle-extraction-prd.md`; bare aliases are never used — always `governance alpha` / `governance beta`. |
| Prerequisites | **VRO-15 repair completion (F01–F18).** Governance layered onto the audited sequential pipeline would be decorative: real task-DAG dispatch, bounded budgets, and truthful persisted task state are load-bearing for every mechanism below. `docs/foundation/vro15-gap-audit.md` F01/F03 own the defect evidence; `docs/foundation/vro15-repair-execution.md` owns repair progress. VRO-16 PRs may not begin against an unrepaired hive. |
| Terminology | Ratified vocabulary is canonical: `YieldToHost` (gate request), `HostCommand` (host resolution), `Suspended` (task state). The reconnaissance document's earlier terms `Paused` and "governance gate" map to these; the mapping table is §1.4. |
| Implementation index | PR-1 Task-level SmartPause & HITL boundary → `crates/vesper-swarm/src/hive/governance.rs` (new), orchestrator state extension, bus tier, both-host surfaces, `xtask` guard tokens; PR-2 PIVOT/REFINE loops + judge separation → `hive/decision.rs` (new), role routing config, ledger versioning, `AuditEvent` variants; PR-3 deterministic verification gates → `hive/verify.rs` (new), receipt digest checks, budget watchdog, cross-host e2e. |

---

## 0. Executive Summary

### 0.1 Objective

Bolt **deterministic, bounded governance** over the VRO-15 swarm extraction. The
hive can already dispatch parallel workers over a shared ledger; what it cannot
do is stop the right thing at the right moment, ask a human a bounded question,
change direction without losing the audit trail, or prove that a synthesized
result traces to real worker evidence. VRO-16 adds exactly those four
capabilities — no new providers, no new topology modes, no consensus protocol.

Governance here means **machine-checked decision points**, not opinions: a gate
is a state transition with a timeout, a decision is an immutable audit event,
a verification gate is a digest/trace check that fails closed.

### 0.2 Explicit non-goals (excluded from all phases)

- **No infinite hangs.** No state in the governance machine may wait for a
  human without a countdown and a pre-configured default action. The only
  unbounded stop in Vesper remains the ultimate safety ceiling
  (root contract: bounded autonomous continuation).
- **No global locks.** No hive-wide pause state exists. Suspension is strictly
  per task/goal; independent workers must keep executing while a sibling task
  is `Suspended` (ratified decision D2).
- **No byzantine/raft consensus.** Navigator decisions remain authoritative;
  peer review ranks and informs, it does not vote (carried from ADR 0025 §1.9).
- **No intervention learning or cross-run personalization** (recon M12
  deferred): conflicts with the ephemeral-ledger design and opt-in durable
  state posture. Reconsider only against exported run records, post-VRO-16.
- **No escalation integrations** (Slack/email/webhook): hosts are
  terminal-first; a notification port does not exist and will not be invented.
- **No new swarm activation surface**: the `swarm` feature stays default-off
  and exact-commit release gated per VRO-15; VRO-16 changes nothing about
  activation mechanics beyond adding governance settings *inside* that surface.
- **No vendor bindings**: judge/author routing uses the existing
  provider-neutral registry only.

### 0.3 What the reconnaissance actually contributes

Of the twelve extracted mechanisms, eight are load-bearing for VRO-16 and four
are explicitly not adopted:

| Recon mechanism | Disposition |
|---|---|
| M1 role×project context grid | **Adopted (partial)** — versioned role templates feed D3 routing; no org-chart/store subsystem. |
| M2 heartbeat/wakeup routine | **Not adopted** — hive tick discipline already owned by VRO-15 repairs. |
| M3 two-plane inbox, wakeup injection | **Adopted** — governance tier on `MessageBus`; directive prepend at prompt construction. |
| M4 `awaiting_confirmation` archived state | **Adopted** — becomes `Suspended` + durable gate record (D1/D2). |
| M5 tiered intervention modes | **Adopted** — per-hive governance profile; native Settings surface only. |
| M6 pause action set + detached interaction | **Adopted** — `HostCommand` variants; TUI keybinds + ACP gate messages. |
| M7 SmartPause dynamic scorer | **Adopted with constraint** — observable signals only; no fabricated model "confidence". |
| M8 pre-stage guidance injection | **Adopted** — bounded directive on `WorkerTask`, ledger-provenanced. |
| M9 PIVOT/REFINE loop | **Adopted** — bounded Navigator decision node (PR-2). |
| M10 debate engine, judge/synthesizer split | **Adopted** — review panel over ledger snapshots + D3 separation (PR-2). |
| M11 verification layers | **Adopted** — claim/trace checks, digests, budget watchdog (PR-3). |
| M12 intervention learning | **Not adopted** (non-goal above). |

---

## 1. Ratified Architecture Decisions

The four decisions below were ratified by Alex and are binding on every PR.
Each is stated with its exact operational semantics; PRs may not weaken them.

### 1.1 D1 — Bounded gates with default-action timeouts

**Rule.** When a task enters a `YieldToHost` gate, the orchestrator starts a
countdown (default **5 minutes**, per-profile configurable, bounded range).
If no `HostCommand` arrives before expiry, the orchestrator executes the
gate's **pre-configured fallback action** — one of:

- `FailTask` — the specific task transitions to `Failed(reason: gate_timeout)`
  with its partial history preserved; siblings unaffected. Default fallback.
- `SafestPath` — resume with the most conservative pre-registered continuation
  (e.g. re-run with narrowed tool scope). Only valid for gates that declare a
  safest path at creation; otherwise `FailTask`.

**Constraints.**

- The countdown is part of the gate's `AuditEvent`; expiry is an audit event.
- Timeout duration is bounded at both ends (minimum long enough for a human to
  read the gate summary; maximum bounded by profile ceiling) — no
  `timeout: infinity` configuration exists.
- A gate whose task has already executed side-effecting tools records which
  ones in the gate record, and the fallback path **must not re-execute** them
  (stream-interruption no-replay contract).
- Gate surfacing follows the bounded-continuation contract: both hosts render
  pending gates with their countdown in the plan/status surface. A silent host
  never converts a bounded gate into a de-facto hang.

### 1.2 D2 — Strictly task-level suspension

**Rule.** `Suspended` is a state of a goal/task in the task DAG, never of the
orchestrator. There is no hive-level pause API, state, or lock. If Driver A's
task suspends on an ambiguity gate, Drivers B/C/D continue until they hit a
strict dependency wall on A's task — which is ordinary DAG blocking, not
governance.

**Constraints.**

- The VRO-15-repaired state machine gains `Suspended` as a first-class
  persisted state with transitions:
  `Running → Suspended (YieldToHost)` and
  `Suspended → Running (HostCommand::Resume | Redirect | fallback-fire)`.
- Suspension is durable in-run: a crashed/restarted hive resumes with the
  gate still open and the countdown recomputed from the gate's creation
  audit event (wall-clock semantics; a restart may not reset the countdown
  to gain more time, nor may it expire a gate early).
- Cancel/shutdown while suspended follows the existing owned-service
  shutdown semantics: partial history preserved, no replay.

### 1.3 D3 — Role-based model routing with mandatory prompt separation

**Rule.** The hive configuration maps **roles**, not vendors: Driver (author)
and Navigator (judge/reviewer) roles may be bound to different
`ProviderSession` configurations — e.g. fast model for execution, high-reasoning
model for evaluation — through the existing provider-neutral registry.

**Minimum guarantee.** Even when author and judge bind the same model, their
system prompts must be **strictly separated and structurally distinct**: the
judge prompt is generated from a distinct role template that never includes
authoring instructions, and the adapter layer can prove separation
(prompt-template identity is recorded in the run's audit trail).

**Constraints.**

- Routing is configuration over the existing registry; no new provider, no
  vendor-specific code paths, no capability claims beyond registered adapters.
- Judge-on-different-model is **permitted and recommended**, never required:
  single-model hives remain valid and must pass all acceptance tests with
  prompt separation alone.
- The debate-engine independence rule (recon M10) is preserved: the composer
  of a synthesis is never the judge of its own decomposition, and at final
  peer review no separate synthesizer exists — reviewers' ranked verdict
  stands (mirrors governance beta's stage-18 exception).

### 1.4 D4 — Structured audit events in the hybrid ledger

**Rule.** Every governance decision is written to the ledger's structured log
as an immutable `AuditEvent` variant — exact-match filterable, replay-safe:

- `GateOpened { task, reason, evidence_refs, countdown, fallback }`
- `GateResolved { task, command: HostCommand, actor, at }`
- `GateExpired { task, fallback_action }`
- `DecisionIssued { goal, verdict: Proceed|Refine|Pivot, rationale_refs, iteration }`
- `GuidanceInjected { task, directive_ref, actor }` — **amended (final audit, 2026-09-12): not a separate variant.** The directive and its actor ride `GateResolved.command` (`Redirect { directive }`), which is already append-only and structured; a second event carrying the same payload would duplicate the trail without adding filterable information. `GuidanceInjected` is removed from the required set; D4's contract is met by `GateResolved` alone.
- `VerificationVerdict { artifact, checks, failures }`
- `BudgetThreshold { level, consumed, ceiling }`

**Constraints.**

- The structured log is the single paper trail: "why did the swarm change
  direction" must be answerable by filtering `AuditEvent`s alone — no
  narrative reconstruction from free text.
- Audit events are append-only within a run; no variant rewrites another.
- `rationale_refs`/`evidence_refs` must resolve to ledger entries or receipt
  artifacts; dangling references fail the writing PR's tests.

**Terminology reconciliation** (recon → ratified): recon `Paused`/gate record →
`Suspended` + `GateOpened`; recon "governance tier message" → `MessageBus`
governance priority tier; recon "scorer" → SmartPause evaluator (§3).

---

## 2. Feature 1 (PR-1) — Task-Level SmartPause & HITL Boundary

### 2.1 Problem

A Driver hitting an ambiguous artifact (unparsable table, conflicting peer
findings, permission-denied on a declared-safe path) currently has only two
outcomes: guess, or fail. Both lose information. The hive needs a bounded way
to escalate exactly that decision to the host's human without stalling
anything else.

### 2.2 Mechanism

**`YieldToHost` gate.** A Driver (or the SmartPause evaluator on its behalf)
emits `YieldToHost { task, reason, evidence_refs, proposed_commands }`. The
orchestrator transitions the task to `Suspended`, writes `GateOpened`, starts
the D1 countdown, and publishes the gate on the `MessageBus` governance tier.

**`HostCommand` resolution set** (both hosts, identical semantics):

| Command | Effect |
|---|---|
| `Resume` | Approve; task returns to `Running` with no prompt change. |
| `Redirect { directive }` | Resume with a bounded directive prepended to the task prompt (M8 mechanism); `GuidanceInjected` audit event. |
| `Fail { reason }` | Task → `Failed(reason)`; partial history preserved; no auto-requeue. |
| `Cancel` | Goal-level cancel via existing owned-service semantics; no replay. |

**SmartPause evaluator** (recon M7, constrained): a pure function consulted at
task-receipt boundaries. Inputs — observable only: receipt success flags,
retry counts, cumulative hive budget consumption, ledger anomaly flags
(dangling refs, digest mismatches), task criticality class from the role
template. Output: `YieldToHost` or proceed. **No model-confidence input**: no
fabricated confidence numbers (root contract).

**Governance profile** (recon M5): per-hive configuration selecting gate
policy — `Auto` (SmartPause only), `Gated` (SmartPause + fixed gates at
decomposition/synthesis boundaries), `Custom` (per-boundary policy). Surfaced
**only** through native Settings in both hosts (root contract: no manual file
editing as activation workflow).

### 2.3 Acceptance criteria

- `hive/governance.rs` unit tests: every `HostCommand` variant produces the
  specified state transition + audit pair; unknown commands are rejected.
- Timeout proof: a gate with a silent host (no command) fires its fallback at
  the configured deadline — tested with an injected clock, no sleeps.
- Parallelism proof (barrier test, `hive_concurrency_regressions` style):
  while task A is `Suspended`, tasks B/C/D demonstrably complete; A's
  suspension never blocks a non-dependent task.
- Restart proof: hive killed after `GateOpened`, restarted → gate still open,
  countdown derived from the audit event, no side-effect replay.
- Naming guard extended (§5.1) and green with **zero baseline growth**.
- Both hosts render pending gates with countdown; identical `HostCommand`
  sets in TUI keybinds and ACP gate messages (cross-host registration test).
- Settings profile change requires no file edits (settings round-trip test).

---

## 3. Feature 2 (PR-2) — PIVOT/REFINE Feedback Loops & Judge Separation

### 3.1 Problem

A completed subtask set may be insufficient: wrong direction (needs Pivot),
right direction with weak parameters (needs Refine), or adequate (Proceed).
Today the Navigator has no bounded, auditable way to change course, and its
synthesis is judged by nobody — including itself.

### 3.2 Mechanism

**Decision node.** After synthesis (or a retry-bounded Driver failure), the
Navigator emits `DecisionIssued` with verdict:

- `Proceed` — goal completes through existing synthesis path.
- `Refine { amended_tasks, iteration }` — amended subtasks re-enqueue through
  DAG admission; **iteration-capped** (per-goal ceiling, default 3); each
  iteration versions the affected ledger snapshot.
- `Pivot { new_decomposition }` — pending tasks discarded (each gets a
  terminal audit event), ledger snapshot versioned and retained, new task DAG
  admitted; **pivot count itself capped per goal** (default 2).

Unbounded Pivot↔Refine cycling is the governance-layer restatement of audit
F03's unbounded queue; the caps are mandatory, and hitting a cap emits
`DecisionIssued { verdict: Proceed-with-failure }` or `Fail` — never a silent
loop.

**Review panel** (recon M10 core): before a synthesis is accepted, a panel of
Driver-role reviewers (distinct role templates, D3 routing) evaluates it
against ledger snapshots — each reviewer sees a **frozen snapshot** of peers'
prior round entries (order-bias control); bounded rounds (default 1); a
reviewer whose turn fails twice is dropped with an audit event, empty output
keeps the prior position; panel of zero after drops = verification failure,
not silent acceptance.

**Judge/synthesizer separation** (D3): panel scoring is a judge-role turn
whose ranked output the composing Navigator consumes as input. The Navigator
never scores its own decomposition. At final peer review of the goal's
synthesis, no separate synthesizer runs — the ranked verdict stands.

### 3.3 Acceptance criteria

- Decision-node unit tests: each verdict produces the specified DAG/ledger
  effects; caps enforced; cap-exhaustion path explicit and audited.
- Ledger versioning: Refine/Pivot preserve prior snapshots; retrieval of any
  iteration's evidence succeeds.
- Panel tests: snapshot isolation (a reviewer cannot see same-round peers),
  drop-on-double-failure, empty-output retention, zero-panel failure mode.
- Prompt-separation proof: judge and author role templates hash-distinct;
  recorded in audit trail; single-model hive passes all panel tests.
- Every decision answers "why": filtering `DecisionIssued` + `rationale_refs`
  resolves to real ledger entries (no dangling refs — enforced by test).
- Reviewer independence: the scoring turn's input provably excludes the
  author's own scoring of the same artifact.

---

## 4. Feature 3 (PR-3) — Deterministic Verification Gates

### 4.1 Problem

Synthesized results can currently cite evidence that does not exist, drift
from what workers actually produced, or burn unbounded budget. These are
machine-checkable failures; VRO-16 makes them gates, not vibes.

### 4.2 Mechanism

**Claim/trace verification** (recon M11, adapted): every factual assertion in
an accepted synthesis must trace to a cited ledger entry or receipt artifact;
untraceable claims are flagged in `VerificationVerdict` and — per profile —
either block acceptance or attach as visible flags. This is the swarm-level
echo of the repository's completion-evidence discipline (ADR 0028).

**Receipt digest checks**: before a `TurnReceipt`'s artifacts count as
evidence, digests are recomputed; mismatch = `VerificationVerdict` failure
and the receipt is not admissible to synthesis input.

**Budget watchdog**: hive-level token/time budget with pause-at-threshold
(50/80/100%): thresholds emit `BudgetThreshold` and at 100% the hive
completes in-flight bounded work, writes final audit events, and stops —
surfaced explicitly in both hosts (root contract: safety-ceiling termination
is visible, never silent).

### 4.3 Acceptance criteria

- Trace check: a synthesis citing a nonexistent ledger ref fails closed; a
  synthesized-from-real-refs one passes (canary test).
- Digest check: tampered artifact → receipt inadmissible; audit event written.
- Budget: 50/80 thresholds emit events without stopping; 100% stops the hive
  with truthful partial state; no unbounded consumption path (injected clock).
- End-to-end (both hosts): Driver produces artifact → reviewer panel scores →
 Navigator synthesizes → verification gates pass → `AuditEvent` chain
  complete and filterable — one e2e per host, registered in the cross-host
  parity test surface.

---

## 5. Cross-Cutting Rules

### 5.1 Naming-guard extension (PR-1)

Following the VRO-15 PR-1 pattern: add the governance-upstream tokens to
`FORBIDDEN_HEX` in `xtask/src/main.rs`, regenerate the baseline **only if
existing frozen content legitimately contains them** (it must not — the
reconnaissance document was re-aliased to `governance alpha`/`governance beta`
before this PRD so the extension requires zero baseline growth), and keep
`cargo xtask naming-guard` green in CI. Word-bounded matching applies; both
the compound and short forms of the Python upstream's name are listed so
neither slips through as a substring of the other.

### 5.2 Host parity

Identical `HostCommand` sets, gate rendering with countdown, governance
profile Settings surface, and audit-event visibility in both hosts, shipped
in the same change (root contract). Host-specific UX (TUI keybind layout vs
ACP gate messages) is composition-only; semantics are shared and tested by
the cross-host registration/advertisement tests.

### 5.3 Evidence discipline

Per ADR 0028 and the root contract: PR completion claims trace every required
behavior to current, scope-appropriate evidence. `cargo xtask acceptance`
runs before any gate here is called verified. Nothing in this PRD is
"done" until its acceptance criteria have executed evidence; this document
is requirements, not completion.

### 5.4 Dependencies

`vesper-swarm` gains no new external dependencies. Role routing uses
`vesper-provider` ports via the composition crate only. No production crate
depends on `vesper-testkit`, frontend crates, or `spikes/` (root contract).

---

## 5A. Final audit record (2026-09-12)

The post-completion audit (reported in
`docs/foundation/evidence-index.md` VRO-16 entry) found nine integration
gaps between the tested engines and the composed production path. All
nine were repaired in the same pass with executing proofs in
`crates/vesper-swarm/tests/hive_governance_audit_fixes.rs`. Two PRD
amendments result: `GuidanceInjected` is removed from the D4 variant set
(§1.4 — the directive rides `GateResolved.command`), and the review
panel's composition point is fixed as pre-synthesis on
governance-enabled hives only, preserving bare-hive VRO-15 turn-count
contracts.

## 6. Phase Plan

| PR | Scope | Depends on | Exit gate |
|---|---|---|---|
| PR-1 | `Suspended` state + `YieldToHost`/`HostCommand` + D1 timeout + governance bus tier + SmartPause evaluator + Settings profile + both-host surfaces + naming tokens | VRO-15 F01–F18 complete | §2.3 all green; guard green; `cargo xtask acceptance` |
| PR-2 | Decision node (Proceed/Refine/Pivot + caps) + ledger versioning + review panel + D3 judge separation + decision `AuditEvent`s | PR-1 | §3.3 all green |
| PR-3 | Trace/digest verification + budget watchdog + verification `AuditEvent`s + cross-host e2e | PR-2 | §4.3 all green; `docs/migration-status.md` updated with evidence |

Each PR lands with its own regression suite in
`crates/vesper-swarm/tests/` (governance-focused files, following the
`hive_*_regressions.rs` naming pattern) and updates
`docs/foundation/evidence-index.md`.

## 7. Risks

- **Sequential-pipeline relapse**: governance code built before F01 repairs
  will bake in sequential assumptions. Mitigation: hard prerequisite.
- **Timeout theater**: a countdown that resets on restart or ticks against a
  mutable clock is a silent hang. Mitigation: injected-clock tests, audit-
  derived countdown on restart.
- **Judge separation erosion**: "temporarily" letting the Navigator score its
  own work under single-model pressure. Mitigation: structural prompt-template
  identity check in tests, not convention.
- **Audit sprawl**: event variants accreting free-text fields that defeat
  exact-match filtering. Mitigation: every variant's fields are enumerated in
  this PRD; additions require a PRD amendment.
- **AGPL hygiene**: governance alpha is AGPL-3.0. Concepts only — zero code,
  text, or structural copying; the naming embargo keeps the boundary greppable.
