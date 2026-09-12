# VRO-16 Reconnaissance: External Governance Patterns (governance alpha, governance beta)

Read-only reconnaissance for the proposed **VRO-16 (Advanced Hive Governance)**
feature. No production code was read, written, or modified. Sources were
analyzed from documentation, configuration examples, and agent-facing guides
only (`.md`, `.yaml`) to bound context ingestion.

## Evidence pinning

| Source | Commit | License | Constraint on reuse |
|---|---|---|---|
| governance alpha (Go team-CLI upstream) | `ba8b69370d23a4946c8d59f932aa71061d1799a6` (shallow HEAD) | **AGPL-3.0** | Concepts only. No code, text, or structure may be copied into Vesper. |
| governance beta (Python research-pipeline upstream) | `be4ba4755bf1b52220f25e13b2293b5956590070` (shallow HEAD) | MIT | Concepts mappable; attribution of ideas is courteous, copying unnecessary. |

Citations below are repository-relative doc paths in `.tmp_recon/` (deleted
after this report; re-clone at the pinned commits to re-verify).

## Vesper anchor state (what VRO-16 maps onto)

Ground truth from current workspace evidence, not aspiration:

- `crates/vesper-swarm/src/hive/orchestrator.rs` — `HiveOrchestrator`; currently
  a sequential pipeline (VRO-15 audit F01), unbounded turns and vanishing
  failed goals (F03).
- `crates/vesper-swarm/src/bus.rs` — `MessageBus`, 4-tier priority queues with
  TTL, ack-required, bounded eviction (`swarm-oracle-extraction-prd.md` §PR-4).
- Navigator/Driver roles over `WorkerPort` (`ProviderSession` + filtered
  `ToolExecutor`), shared HNSW ledger with scope filters
  (`docs/swarm-oracle-extraction-prd.md` §1.7, §2).
- Swarm is **default-off** pending VRO-15 repairs
  (`docs/foundation/vro15-gap-audit.md` verdict; repair execution tracked in
  `docs/foundation/vro15-repair-execution.md`).
- No HITL/pause primitive exists in the swarm layer today. The nearest existing
  pause surface is harness-level: VesperLens
  `request_human_input`/`request_human_review` (blocking browser interview),
  ACP opt-in checkpoints, and permission gating in the agent loop.

**VRO-16 ordering constraint:** the audit's F01–F03 repairs (real task-DAG
dispatch, bounded budgets, truthful failed-goal state) are prerequisites for
any governance layer — gates wrapped around a sequential pipeline that loses
failed goals would be decorative.

---

## Part 1 — governance alpha mechanisms

### M1. Context grid: role × project composition

**Mechanism.** Agent context composes along two axes — role (horizontal) and
project (vertical) — merged automatically at `hire` time as
`organization → team → role → project`. Changing a role prompt propagates to every
agent holding that role on the next `sync`.
(`README.md` §"Six design pillars" 1; `docs/workspace-layout.md`.)

**Vesper mapping.**
- Role axis → Navigator/Driver are already `WorkerPort` configurations
  (system instructions + tool subset + turn budget). Add a
  **role-template layer**: named, versioned instruction/tool-set presets
  (`navigator`, `driver`, plus governance roles below) resolved at hive
  admission, not hardcoded strings in `orchestrator.rs`.
- Project axis → the existing ledger scope system (`Swarm` vs `Worker` scopes,
  `swarm-oracle-extraction-prd.md` §2) already provides per-run isolation;
  project-merge corresponds to composing hive config with workspace-scoped
  provider/skill settings at boot.
- `sync` propagation → hive admission already reconciles role classes
  (`hive/lifecycle.rs`); extend reconciliation to detect stale role-template
  versions and close-and-readmit rather than silently mixing versions.

**Gap flagged.** Vesper has no persisted, versioned role-template store. This
is a new small artifact, not a change to existing crates' semantics.

### M2. Heartbeat: non-overlapping wakeup loop + wakeup routine + jitter

**Mechanism.** The scheduler is a per-agent loop: wake → drain all pending
tasks → sleep `interval` → wake. When the queue is empty, a **wakeup routine**
(`wakeup.md` prompt) fires to proactively find work. Active-hours windows,
active-days, startup jitter (anti-thundering-herd) are built in.
(`docs/commands.md` §`scheduler`; `README.md` pillar 2.)

**Vesper mapping.**
- Non-overlapping loop → `HiveOrchestrator` tick discipline: one in-flight goal
  batch per worker lease; the audit already demands pool leases and bounded
  concurrent futures (F01 solution). Heartbeat = the tick cadence of the hive
  event loop, not a new thread.
- Wakeup routine → an **idle-goal hook**: when no eligible `WorkerTask` exists
  in any lease, the Navigator may be invoked with a bounded
  "find follow-up work" prompt against the ledger. Critically, output must be
  validated through the same bounded task-DAG admission (audit F03: goal queue
  currently unbounded) — a wakeup routine that can enqueue unlimited goals is
  exactly the failure mode VRO-15 F03 documents.
- Jitter → staggered worker admission already exists conceptually; jitter
  belongs at driver-wake scheduling if multi-driver polling loops are
  introduced.

### M3. Inbox: two-plane messaging (blocking gates vs async messages)

**Mechanism.** Every participant (agent or human) has an inbox with two
distinct planes:
1. **Task confirmations** — `task confirm-request` moves the running task to
   `awaiting_confirmation` (archived), and the task is *blocked* until a human
   replies via `inbox reply`.
2. **Async messages** — non-blocking `inbox send/reply/fwd` between any
   participants; **unread messages are auto-injected at the top of the next
   wakeup prompt** — no polling in agent code.
(`docs/commands.md` §`inbox`; `docs/workflow-design.md` §2.4, §4.1.)

**Vesper mapping.**
- Async plane → `MessageBus` already has per-agent inboxes with type filters;
  add a **`Governance` message type tier** (or reserve the highest priority
  tier for gate resolutions so a human decision outranks normal work).
  Auto-injection at wakeup maps to: bus messages with pending acks are
  prepended to the next `WorkerTask` prompt construction — this is exactly the
  audit F01 fix ("make bus delivery task-ID-checked, not ceremonial").
- Blocking plane → new `Paused` orchestrator state (see M4). The inbox reply is
  the gate-resolution event that transitions out of `Paused`.

### M4. Task state machine with `awaiting_confirmation`

**Mechanism.** Seven states: `pending → in_progress → {done_success,
done_failed, blocked, awaiting_confirmation}`; `awaiting_confirmation` tasks
are **archived** (append-only `tasks_archive.yaml`) and complete only via human
inbox reply; `blocked` awaits dependency completion; failed tasks retry with
`max_retries`. Priorities 0–3; scheduler picks highest priority first.
(`docs/workflow-design.md` §3.1–3.3, §4.2.)

**Vesper mapping.**
- Vesper's hive needs a **first-class goal/task state machine** — the audit's
  F01/F03 solution already calls for "persisted-in-run task status and explicit
  failed/partial outcomes." governance alpha's shape is a good reference topology:
  `pending → in_progress → {succeeded | failed(retry-bounded) | paused(awaiting
  governance) | blocked(dep unmet)}`.
- `awaiting_confirmation` being *archived* is the interesting detail: a paused
  task must be durably recorded (Vesper: swarm ledger entry with provenance)
  so a crashed hive can resume with truthful state — aligning with Vesper's
  stream-interruption contract (never replay ambiguous side effects; a paused
  task that already executed tools must not re-execute them on resume).
- `blocked` → dependency edges in the task-DAG the audit demands.

---

## Part 2 — governance beta mechanisms

### M5. Tiered intervention modes

**Mechanism.** Seven modes (`full-auto`, `gate-only`, `checkpoint`, `co-pilot`,
`step-by-step`, `express`, `custom` per-stage policy) select *where* the
pipeline pauses — from never, to 3 fixed gate stages, to all 23 stages.
(`docs/HITL_GUIDE.md` §3.)

**Vesper mapping.** A per-hive governance profile selecting pause points:
`Auto` (no gates; current behavior target), `Gates` (pause at
decomposition/synthesis boundaries), `Review` (pause after every Driver
receipt), `Custom` (per-phase policy in hive config). Must live in hive
configuration surfaced through native Settings/TUI+ACP parity — per root
contract, activation belongs in `/settings`-style controls, not manual file
editing. ACP host needs an explicit governance surface since its checkpoint
state is opt-in by default.

**Tension flagged (important).** Vesper's root contract requires *bounded
autonomous continuation* — "users must not have to babysit an active native
plan." governance alpha and governance beta invert this: the human is a blocking gate by default.
VRO-16 must not regress the babysitting problem. Resolution: gates pause
*bounded, explicitly-scoped decision points* (as governance beta's gate stages do — 3
gates, not 23), every pause carries a default-continue timeout consistent with
Vesper's bounded-continuation contract, and unresolved gates surface in the
plan/status UI rather than silently hanging the hive. The safety ceiling
(ultimate termination) remains the only unbounded stop.

### M6. Pause action set + detached interaction

**Mechanism.** At any pause: `approve / reject(+rollback) / edit($EDITOR) /
collaborate(multi-turn) / inject-guidance / skip / rollback-to-stage / abort /
view`. Detached commands operate on a paused run from another terminal:
`status / attach / approve / reject / guide --stage N --message`.
(`docs/HITL_GUIDE.md` §4–6.)

**Vesper mapping.**
- The action set maps to gate-resolution variants on the `Paused` state:
  `Approve` (resume), `Reject` (goal → failed with reason, no auto-requeue),
  `Redirect` (inject guidance = append bounded directive to the task prompt,
  resume), `Revise` (collaborate = one bounded follow-up Navigator turn with
  the human directive), `Abort` (hive cancel with partial-history
  preservation, no replay). `Skip` should map to "complete-without-synthesis
  for this subtask" rather than governance beta's stage-skip, since Vesper tasks are DAG
  nodes.
- Detached interaction → in the TUI host, gates surface as plan items with
  keybound resolution; in the ACP host, as session messages the client answers
  asynchronously (mirroring governance beta's detached `attach`). Both hosts must expose the *same*
  resolution set (host-parity contract).

### M7. SmartPause: dynamic pause decision

**Mechanism.** Beyond fixed gates, pauses are decided dynamically from four
signals: quality score (PRM/heuristic), stage criticality (lower threshold for
high-impact stages), historical rejection rate per stage, and model confidence
("asks for help when uncertain"). No configuration required.
(`docs/HITL_GUIDE.md` §10.)

**Vesper mapping.** A **governance scorer** consulted at task-receipt
boundaries in `HiveOrchestrator`: inputs = Driver `TurnReceipt` metrics
(already in the port contract), task criticality class (from the role
template, M1), cumulative hive budget consumption, and ledger-recorded prior
gate outcomes (rejection history). Output = pause/no-pause against the active
governance profile's threshold.

**Honesty constraint.** "Confidence" is not something Vesper providers
currently report portably. Either define confidence from observable signals
only (receipt success flags, retry counts, budget pressure, ledger anomaly
flags) or gate that input behind per-adapter capability metadata — never
fabricate a confidence number (root contract: never fabricate quotas/values).

### M8. Pre-stage guidance injection

**Mechanism.** `guide --stage N --message "..."` injects guidance
for a stage *before it runs*, at any time; guidance is folded into that stage's
LLM context automatically. (`docs/HITL_GUIDE.md` §6.)

**Vesper mapping.** Pending directives attached to a `WorkerTask` (or task
class) in the DAG, prepended at prompt construction — same mechanism as M3's
message auto-injection. Bounded size, provenance-tagged in the ledger, visible
in synthesis input so the Navigator cites which directives shaped a result.

### M9. PIVOT / REFINE decision loop

**Mechanism.** Stage 15 (`RESEARCH_DECISION`) autonomously chooses PROCEED /
REFINE (tweak params, loop back to stage 13) / PIVOT (new direction, loop back
to stage 8), with automatic artifact versioning per iteration.
(`README.md` §features, pipeline diagram; `the upstream root AGENTS.md` phase F.)

**Vesper mapping.** A **Navigator decision node** after synthesis (or after a
retry-bounded Driver failure): emit `Proceed | Refine( amended subtask set,
bounded iterations ) | Pivot( new decomposition, new task DAG )`. Maps onto
the audit-demanded goal state machine as bounded re-entry: Refine re-enqueues
amended tasks (deduplicated, iteration-capped); Pivot discards pending tasks,
versions the partial ledger snapshot, and restarts decomposition. The
iteration cap is mandatory — an unbounded Pivot↔Refine cycle is the
governance-layer version of F03's unbounded queue.

### M10. Debate engine: structured multi-model peer review

**Mechanism.** Opt-in panel (multiple models) bound round-robin to
perspective roles (innovator/pragmatist/contrarian). Round 0: independent
opening statements. Rebuttal rounds: each role sees a **snapshot** of all
other roles' previous-round positions (order-bias control); a role whose
generation fails twice is dropped, empty rebuttals keep the prior position.
Scoring and synthesis are **split into two calls with different models**:
judge ranks perspectives (rigor/evidence/falsifiability) without rewriting;
synthesizer merges anchored on the judge's ranking. Exception: at *peer
review* the split is deliberately **not** used — reviewer independence means
the author must not synthesize reviews of their own work. Every step
try/except-falls-back (score-fail → synthesize unranked; synth-fail → return
raw concatenation). Full record in `debate_record.json`.
(`docs/debate_engine.md` — full document.)

**Vesper mapping.** This is the strongest single pattern for VRO-16:
- Perspective roles → Drivers are already interchangeable `WorkerPort`
  configs; a review panel = N Drivers instantiated with distinct role
  templates (M1) over the same artifact, each writing ledger entries.
- Snapshot rebuttal → a bounded number of ledger-scoped rounds where each
  reviewer reads a frozen snapshot of peers' prior entries — maps directly
  onto ledger scope filters + MessageBus delivery of "review round N open"
  events. The snapshot rule is the order-bias control the audit wants for bus
  correlation (F01).
- Judge/synthesizer split → **Navigator is the synthesizer, never the judge of
  its own decomposition.** A review panel's scoring must be a Driver-role turn
  (or separate Navigator instance) whose output the composing Navigator
  consumes as ranked input. Peer review of the *final synthesis* inverts: no
  separate synthesizer — the reviewers' verdict stands, mirroring governance beta's
  Stage-18 exception.
- MessageBus tiers → review-round events ride the bus at a priority above
  normal work but below governance resolutions (M3).
- Degradation policy (drop-failed-role, keep-prior-position, fall back to raw
  concatenation) matches Vesper's bounded-recovery/no-hang posture; every
  round and score is a ledger entry → the `debate_record.json` equivalent is
  the hive run's own ledger + a small governance decision record.

### M11. Verification layers

**Mechanism.** Layered: (a) **Claim Verifier** — extracts citation/numerical/
factual claims from AI text and cross-checks against collected source data,
flagging ungrounded content; (b) **SHA256 manifest** per stage artifact —
out-of-band modification detected; (c) **Sentinel watchdog** — background
monitor for NaN/Inf, paper-evidence consistency, citation relevance,
anti-fabrication; (d) **cost budget** with 50/80/100% pause thresholds and
tiered escalation (terminal → Slack → email → auto-abort at 24h).
(`docs/HITL_GUIDE.md` §9; `README.md` features.)

**Vesper mapping.**
- Claim verification → a governance Driver role over the artifact + its
  ledger provenance refs: every factual assertion in a synthesis must trace to
  a cited ledger entry/artifact; untraceable claims are flagged, not silently
  included. This is the swarm-level echo of Vesper's existing
  completion-evidence discipline (PRD claims must trace to current evidence).
- SHA256 manifests → artifact digests already conceptually present in
  `TurnReceipt.artifacts`; governance verification recomputes digests before
  treating a receipt as evidence.
- Cost/budget watchdog → hive-level token/time budget with pause-at-threshold
  (not just abort), feeding M7's scorer. Auto-abort at ceiling maps to
  Vesper's safety-ceiling termination with explicit surfacing in both hosts.
- Escalation channels → Vesper's hosts are terminal-first; escalation beyond
  the session is out of scope for VRO-16 unless a notification port already
  exists — do not invent integrations.

### M12. Intervention learning (ALHF) and quality prediction

**Mechanism.** Approve/reject/edit events train per-stage pause thresholds
(stages always approved → auto-approve; frequently rejected → pause more).
Quality predictor estimates final output quality from current artifacts.
(`docs/HITL_GUIDE.md` §10.)

**Vesper mapping.** **Defer.** This requires persisted cross-run user-state
in the swarm layer, which conflicts with the ephemeral-ledger design
("the ledger dies with the hive unless exported") and Vesper's careful
opt-in durable-state posture in ACP. Record as a possible post-VRO-16
extension fed by exported run records only.

---

## Synthesis: proposed VRO-16 mechanism map

| # | External pattern | Source | Vesper primitive | New surface required |
|---|---|---|---|---|
| 1 | Role×project context grid, sync propagation | governance alpha M1 | `WorkerPort` role templates + ledger scopes | Versioned role-template store |
| 2 | Non-overlapping heartbeat + wakeup routine + jitter | governance alpha M2 | Orchestrator tick + bounded idle-goal hook | Idle hook with DAG admission |
| 3 | Two-plane inbox; auto-inject at wakeup | governance alpha M3 | `MessageBus` tiers + per-agent filters | `Governance` message tier; prompt-prepend delivery |
| 4 | `awaiting_confirmation` archived task state | governance alpha M4 | Goal/task state machine (audit F01/F03 fix) | `Paused` state + durable gate record |
| 5 | Tiered intervention modes | governance beta M5 | Per-hive governance profile | Settings surface, both hosts |
| 6 | Pause actions + detached attach/approve/guide | governance beta M6 | `Paused`-state resolution variants | TUI keybinds + ACP gate messages |
| 7 | SmartPause dynamic scorer | governance beta M7 | Receipt-metrics scorer at task boundaries | Governance scorer (observable signals only) |
| 8 | Pre-stage guidance injection | governance beta M8 | Directive attached to `WorkerTask` | Bounded directive field + ledger provenance |
| 9 | PIVOT/REFINE bounded loop | governance beta M9 | Navigator decision node over task DAG | Iteration caps + ledger versioning |
| 10 | Debate engine w/ judge–synthesizer split | governance beta M10 | Driver review panel + Navigator synthesis + bus snapshot rounds | Panel orchestration + independence rule |
| 11 | Claim verify + digests + budget watchdog | governance beta M11 | Verifier Driver role + receipt digests + hive budget | Budget thresholds feeding scorer |
| 12 | Intervention learning / quality prediction | governance beta M12 | — | **Deferred** (cross-run state conflict) |

## Design constraints VRO-16 must respect (from Vesper's own contracts)

1. **Prerequisite:** VRO-15 repairs F01–F03 (real parallel dispatch, bounded
   budgets, truthful task state) land first; governance on a sequential
   pipeline is decoration.
2. **No babysitting regression:** every gate is bounded and scoped; default-
   continue behavior and explicit plan surfacing follow the bounded autonomous
   continuation contract; only the safety ceiling terminates unbounded.
3. **No replay:** resuming from `Paused` must never re-execute possibly
   side-effecting tool calls (stream-interruption contract); gate records must
   capture which side effects already ran.
4. **Host parity:** identical governance resolution sets in TUI and ACP in the
   same change, with documented host-specific UX exclusions in the nearest
   AGENTS.md.
5. **No fabricated signals:** the SmartPause analog uses observable receipts
   and budgets only; no invented provider "confidence".
6. **License hygiene:** governance alpha is AGPL-3.0 — architectural inspiration and
   cited doc reading only; zero code or text reuse.
7. **Activation UX:** governance profiles activate through native settings
   controls, not manual config editing.

## Open questions for the VRO-16 PRD

- Gate timeout semantics: default-continue after N minutes (contract-friendly)
  vs wait-forever (governance alpha behavior) — recommend bounded with visible
  countdown in plan UI.
- Whether the review panel's judge role runs on a *different provider/model*
  than the synthesizer (governance beta's independence principle) or merely a different
  session — provider-neutral registry makes both expressible; policy default?
- Ledger export format for governance decision records — reuse hive ledger
  entries vs a dedicated run report artifact.
- Does `Paused` state apply to individual tasks only, or can a whole hive
  pause (governance beta pauses the whole pipeline; governance alpha pauses one task)?
  Task-level is the better fit for a parallel DAG.

## Method note

- Shallow clones at the pinned commits under ignored `.tmp_recon/`; analyzed
  `README.md`, `docs/*.md`, `SKILL.md`, `the upstream root AGENTS.md`,
  `the example config.yaml` only. No `.go`/`.py`/`.rs` files opened.
- `.tmp_recon/` deleted after this report; the temporary `.gitignore` entry
  was reverted so no Vesper file retains a modification from this task
  (except documentation).
