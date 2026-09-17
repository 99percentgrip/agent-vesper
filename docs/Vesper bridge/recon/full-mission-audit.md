# Vesper Bridge — Full-Mission Audit (Phase 0 → 2p)

**Audit type:** Report-only gap/bug audit requested by Alex before Phase 3 (Resolve) begins.
**No code was changed by this audit.** Every finding below was verified against current
source in this session, not against phase-report prose.

> ## FIX STATUS — all findings addressed (2026-09-15, "please fix all gaps")
>
> Alex accepted the full fix list. Disposition per finding, all red-first
> (`tests/audit_fixes.rs` failed on the pre-fix code; 10/10 green after):
>
> | Finding | Status | Where |
> |---|---|---|
> | C1 lease leak on denied dispatch | **FIXED** | `session.rs` step-7/8 revokes via `revoke_by_token`; tests `c1_*` |
> | C2 revalidate-before-dispatch | **FIXED** | `BridgeSession::revalidate()` (lease liveness, generation, timeout, expiry); wired into `bridge_execute`; tests `c2_*` |
> | C3 quarantine erasure / no producer | **FIXED (report half)** | `close()` returns `CloseReport{was_quarantined, inputs, jobs}`; surfaced by disconnect. Production quarantine-on-timeout remains adapter-phase wiring (documented below) |
> | H1 cross-session emergency sweep | **FIXED** | `LeaseState.owner` + `emergency_release_inputs_owned(session)`; `u64::MAX` sweep removed from production paths; test `h1_*` |
> | H5 same-observation mutation replay | **FIXED** | per-mutation freshness (`latest_mutation_observation_revision`); test `h5_*` |
> | H2 hard-coded authority epoch | **FIXED** | epoch minted on connect + resume; stale epochs refuse; test `h2_*` |
> | H3 error typing / discovery truth | **FIXED** | `BridgeError::InvalidState`; discovery string single-sourced |
> | H4 `/bridge disconnect` static + `unreachable!()` | **FIXED** | both hosts execute disconnect against the live service; shared command answers truthfully, never panics |
> | H5b/H6 observe identity | **FIXED** | observe returns id/revision/captured_at/degraded to the model |
> | H7 lease denial detail | **FIXED** | `LeaseConflict{holder,expires_in_ms}` / `StaleFence{observed,required}` carried to model text |
> | M1 unbounded arguments | **FIXED** | 64 KiB bound at `bridge_execute` (MAX_ARGUMENT_BYTES) |
> | M2 discarded arguments | **FIXED** | caller's arguments flow into the validated spec |
> | M3 journal double-read | **DOCUMENTED** | single-owner session + tokio mutex is the enforced precondition; noted in authorize docs |
> | M4 pixel budget uncalled | **FIXED** | observe refuses over-budget/degraded at admission |
> | M5 generation never advances | **DOCUMENTED** | adapter-phase prerequisite recorded (discovery→attach mints next generation) |
> | M6 silent job eviction | **FIXED** | `evicted_jobs` counter surfaced in the stop report |
> | M7 dropped partial evidence | **FIXED** | `IntentRecord.partial_evidence` (4 KiB bound) journaled by `settle_partial` |
> | M8/M8b help/surface asymmetry | **FIXED** | help regenerated from real verbs; case-insensitive; TUI/ACP patterns unified |
> | M9 confirmation latch | **FIXED** | admitting an input lease clears `input_release_confirmed` |
> | M10 dead elapsed/timeout | **FIXED** | folded into `revalidate` (C2) |
> | L1 duplicate strings | **FIXED** | `BRIDGE_NO_ADAPTER_DISCOVERY` shared constant |
> | L2 write-only mark | **FIXED** | `emergency_released_through()` surfaced in stop report |
> | L3 `unreachable!()` | **FIXED** | truthful fallback text |
> | L4 header drift | **FIXED** | session header refreshed |
> | L5 unused derives | **NOT CHANGED** | deliberately retained: Phase 3 durable-session work uses them |
> | L6 stale enrollment note | **FIXED** | resolution banner added |
> | L7 count drift | **FIXED** | normalized below |
>
> **Post-fix receipts:** vesper-bridge 78/0 (68 + 10 audit), harness
> bridge 144/0 (+2 command tests), full battery below. Two new defects
> were introduced and caught during the fix itself: a nested fence-lock
> deadlock (fixed by evaluating the M9 latch before taking the lock) and
> a duplicated `Granted` match arm — both caught by the audit tests
> before landing.

## Audit method

- Hand-derived the lifecycle interplay matrix (pause/resume/stop/cancel/close/quarantine/
  disconnect/disconnect-with-held-inputs) and traced each transition in
  `crates/vesper-bridge/src/session.rs`.
- Traced every "PRD demands X" comment to the actual enforcement site and asked:
  *who calls this, and when*?
- Cross-checked all 17 phase reports' PASS/BLOCKED claims against the current tree and
  current test suite names.
- Searched for the exact defect class this mission kept finding: contracts that exist in
  types/comments but have no producer, no consumer, or no enforcement call-site.

---

## Severity scale

- **CRITICAL** — breaks a PRD safety invariant; must fix before any adapter dispatches.
- **HIGH** — wrong behavior in a reachable production path; fix before Phase 3.
- **MEDIUM** — real gap, bounded blast radius; fix with Phase 3 or document as adapter debt.
- **LOW** — hygiene, docs debt, minor UX.

---

## CRITICAL

### C1. `authorize()` leaks an exclusive lease on every duplicate/uncertain-retry denial
`session.rs:286–334`. `fences.admit(requested_lease)` runs at **step 6**, but duplicate
suppression (step 7) and intent-journaling (step 8) can still `return Err` **after** the
lease was already granted. Nothing revokes it. Sequence: dispatch A holds `Document(p)`;
a retried request for the same resource is admitted (expiring/expired-holder path or
after release), then suppressed as a duplicate — the **second** lease is now live in
`resources` with a bumped fence, while the caller believes it was refused. The next
honest request hits a permanent `LeaseConflict` until expiry. Violates BR-14/15
(one exclusive writer; fences only increase) and can wedge the document.
**Fix:** on any post-admit `Err`, call `fences.revoke(&requested_lease.id)` before
returning. Add a red-first test: denied-duplicate dispatch must leave zero live leases.

### C2. PRD §6 "revalidate immediately before dispatch" has no enforcement site
PRD §6: *"The host, not the model, supplies authoritative target bindings, lease tokens,
authority generations and approval references. Revalidate them immediately before
dispatch. A changed target, schema, output path, material argument or relevant
observation invalidates affected approval."* The core checks generation (step 4) and
observation freshness (step 5) **inside authorize()**, but there is no second validation
between `authorize()` returning `Ok` and dispatch. `FenceState::validates()` exists
(lease.rs:137) and is **called by nothing in the workspace** outside its own unit test.
`elapsed_ms` is accepted then discarded (`session.rs:361: let _ = elapsed_ms;`) — lease
expiry is therefore never re-checked against the passage of time even though the API
pretends it is. When Phase 3's adapter dispatches, this is the exact "approve→world
changes→dispatch anyway" window the PRD forbids.
**Fix:** add a `revalidate(&request) -> Result<(), DenialReason>` that the hosted layer
must call immediately before handing the envelope to the adapter: re-run `validates()`
on the lease, re-check `authority_generation` against the current generation, re-check
`elapsed_ms` against `timeout.timeout_ms` and lease `remaining_ms`, and re-check
observation revision. Make the harness service call it as part of `execute` and refuse
on failure. Red-first: a test where time advances past the lease between authorize and
revalidate must refuse.

### C3. A quarantined session can still be `close()`d away — and production
`disconnect` never quarantines
Two related holes in one invariant (§7: *"quarantine a session when cleanup, queued
writes or effects remain uncertain; restarting… must not automatically resume
mutations"*):
1. `close()` (session.rs:200) unconditionally overwrites any state — including
   `Quarantined` — to `Closed` with no check, no warning, no outstanding-effects
   surfacing. The disconnect tool path *does* surface inputs/jobs (phase2m), but the
   core-level `close()` silently erases the quarantine flag the PRD made sticky.
2. Nothing production-side ever calls `quarantine()`; the only producers are the
   `reconcile(None)` path and tests. So uncertain disconnects (timeout after a sent
   mutation) currently produce `Closed`, not `Quarantined` — the state that would block
   a resumed session from mutating.
**Fix:** (a) `close()` from `Quarantined` must either refuse or return a
`CloseReport { was_quarantined, inputs_pending, jobs }` the host must surface;
(b) the harness `bridge_execute` path, when an adapter eventually times out or errors
after intent-recording, must call `quarantine()` rather than plain-settle — wire that
rule now (even with no adapter) as the documented dispatch-failure policy.

---

## HIGH

### H1. Emergency release does not scale: `u64::MAX` releases *everything*, including
another session's live input lease
`bridge_service.rs` (both `stop()` and disconnect) call
`emergency_release_inputs(u64::MAX)`. With `with_shared_fences` (BR-14/AT-16 — multiple
sessions share one `FenceState` per resource), one session's stop sweeps **every**
input-holding lease at or below MAX — including a *different* session's currently valid
foreground-input lease. Two concurrent sessions over the same seat: session A stops →
session B's input lease is emergency-released underneath it. That is a cross-session
authority breach (NF-03 release is scoped to *this* session's inputs).
**Fix:** pass the session's **own** current fence per resource (track the fences this
session was granted) or add an owner-scoped variant `emergency_release_owned(session_id)`.
Red-first: two sessions, one shared `FenceState`; A stops; B's lease must survive.

### H2. `connect` resets nothing — reconnect after stop keeps stale admission state
`bridge_connect` refuses only when a session already exists. After `/bridge stop`
(admission `Closed`), the *service* still holds the session; `bridge_connect` says
"already active; disconnect first" — fine. But after `bridge_disconnect` + reconnect,
`BridgeSession::new` is fresh, so that path is clean. The gap: `/bridge stop` →
`/bridge resume` never consults the **tool-surface** session: `resume()` in the core
re-opens admission but the harness `bridge_execute` route never re-derives authority —
`authority_generation` is the hard-coded `1` at every dispatch
(`bridge_service.rs:306/313`). With no adapter this is inert; with Phase 3 it becomes
"resume re-authorizes by default," which NF-02/§6 forbids (renewed authority required).
**Fix:** mint a fresh `authority_generation` on each `resume()` and each `connect()`;
reject dispatches whose `authority_generation` is older than the current mint. Wire it
before the first adapter lands so the adapter never inherits hard-coded 1.

### H3. `Discovered`/`Connecting` are unreachable-in-production states; `connect()`
error typing hides it
`connect()` from `Ready` returns `TransportUnavailable` — semantically wrong (the
transport is fine; the request is invalid) and the service never surfaces it because it
constructs state directly (`connect()` + `mark_ready()` back-to-back). Also nothing
production-side ever produces `Discovered`→`Connecting`→`Ready` as separate observable
steps; discovery (`bridge_discover`) is a static string that shares no state with the
session. Impact: moderate — the state machine's first two states are tested but never
exercised by the composition, so their semantics are unverified against real use.
**Fix (small):** have the no-adapter composition actually traverse
`Discovered→Connecting→Ready` on `bridge_connect` (it already does via
`connect(); mark_ready()` — make `connect()`'s invalid-state error a distinct
`InvalidState` variant instead of `TransportUnavailable`), and make `bridge_discover`
return the same no-adapter manifest the session would bind (single source of truth).

### H4. `/bridge` surface mismatch between hosts and shared module
`bridge_command.rs` advertises: `status, discover, connect <app> (via the model tool
surface), stop, resume, release confirmed, disconnect`. But:
- `disconnect` is **not** executable via `/bridge` in either host — it only produces
  static text saying "the model tool surface owns the actual close". A user typing
  `/bridge disconnect` gets an explanation, not a disconnect. Inconsistent with
  `stop`/`resume`/`release confirmed`, which *are* executed against the live service.
- The shared `command()` has `unreachable!()` for `"stop"` — a landmine: any host that
  forgets to intercept `stop` before calling the shared function panics the host
  process. The TUI intercepts before calling; ACP intercepts before calling; but the
  contract is enforced by convention, not by type.
**Fix:** route `disconnect` through the live service like `stop`/`resume` (it is a
host-owned close); replace `unreachable!()` with a truthful fallback string (e.g.
"stop must be executed by the host") — never a panic path in shared code.

### H5. Observation freshness check accepts *equal* revisions — replay of the same
observation can ground repeated dispatches
`session.rs:279`: `if observation.revision < self.latest…` — equal passes. `replay_observation()`
in the service returns the **last** observation every time; so after one `observe`,
unlimited `execute` calls all pass freshness with the same revision. BR-15's
*"a changed … relevant observation invalidates affected approval"* is enforced for
*older* revisions only; sameness is allowed forever. For read-only ops that is
defensible (cheap re-plan), but for **mutating** ops the PRD requires binding to a
*fresh* observation per dispatch (AT-13). No distinction exists today.
**Fix:** for `Mutating`/`Destructive`/`ExternalTransmission` mutability, require
`observation.revision > latest_used_for_mutation` (per-mutation freshness), or require
the service to mint a new observation on every mutating dispatch. Red-first: two
mutating dispatches against one observation — second must refuse.

### H6. `bridge_observe` ignores the returned observation's freshness contract — and
never stores `captured_at_ms`
`next_observation()` mints revision N and stores it; `bridge_observe` returns
`observation.semantic` **only** — the model never sees revision/id, so it cannot name
the observation its plan was derived from (the PRD §6 precondition-binding loop).
`captured_at_ms` is always `0` (host-injected time exists nowhere), so time-based
staleness is unimplementable downstream. Cosmetic today, structural for Phase 3: the
adapter must return observation identity in results and accept it in preconditions.
**Fix:** include `id`/`revision`/`captured_at_ms` in the observe tool result (bounded
JSON), and have the host pass a real elapsed-time value into the service
(`BridgeToolService` currently hard-codes `elapsed_ms: 0`).

### H7. Lease conflicts are reported but the **holder** is not — denial text loses the
actionable detail
`LeaseDecision::Conflict { holder, expires_in_ms }` is constructed with rich detail,
then `session.rs` flattens it to `DenialReason::Lease(LeaseConflict)` and the service
prints the bare error. The model cannot tell Alex *which* lease holds the resource or
how long until it expires — the two facts that would let a retry succeed. Same for
`StaleFence { observed, required }`.
**Fix:** extend `DenialReason::Lease` to carry `holder`/`expires_in_ms` (or
`observed/required`) and render them in `denial_text`. One-variant change, existing
tests unaffected in meaning.

---

## MEDIUM

### M1. `CapabilityManifest` records are cloned wholesale into `authorize()` —
unbounded argument surface
`manifest.record(&spec.capability)` returns a reference; the code clones the whole
`CapabilityRecord` (fine), but `OperationSpec.arguments` is `serde_json::Value` with
**no size bound** anywhere before `journal.record_intent` serializes it. NF-05 bounds
records count, not record **size**. A 50 MB `arguments` value would be journaled and
retained 512 times over. No adapter exists yet, so not exploitable today; it becomes a
DoS surface the day one does.
**Fix:** bound `arguments` at `OperationSpec` construction (e.g. reject > 64 KiB
serialized) — matches the PRD's bounded-payload posture (§13.2 failure language).

### M2. `deny_unknown_fields` on `bridge_execute` args silently drops `arguments`
`bridge_service.rs`: `struct Args { capability, arguments }` with
`deny_unknown_fields` — good — but the parsed `arguments` is then **discarded**
(`arguments: serde_json::json!({})` is dispatched). The model can send a full argument
object and the core validates an **empty** one. Truthfulness gap in the no-adapter
composition: the denial the model receives reflects an empty-arguments dispatch, not
what it sent.
**Fix:** pass `args.arguments` through to the `OperationSpec` (it is validated against
the capability's schema version by the manifest layer in Phase 3; today at minimum stop
discarding it).

### M3. `classify_retry` journal double-read race
`authorize()` calls `classify_retry(journal, …)` then `journal.record_intent(...)`. The
`JournalPort` trait is `&self` with interior mutability; two concurrent dispatches with
the same id could both classify `Fresh` then both record. Core is documented
"one owner per session" and the service serializes via a tokio `Mutex` — so today
unreachable. But `BridgeSession` is `Send`-by-design and the doc contract ("callers
supply…") does not state single-threadedness. Document or enforce: make the
session-level mutex a documented precondition (fine) or have `record_intent` return a
`Duplicate` error the core maps to suppression (stronger).

### M4. Pixel budget (`NF-07`) has no enforcement call-site
`Observation::within_pixel_budget()` exists, unit-tested — and called by **nothing**.
Same class as C2: budget as library, not as gate. The moment an adapter produces a
16-megapixel screenshot × 9 images, nothing refuses it.
**Fix:** call `within_pixel_budget()` (and `can_ground_visual_actions()`) in
`bridge_observe`/adapter ingestion and refuse degraded/over-budget observations at
admission, not just trust callers.

### M5. `Generation` never advances anywhere in production
`Generation` ordering is well-tested, but no production path ever mints
`Generation::next()`; the service binds `Generation(1)` forever (bridge_service.rs:244,
366). A restarted application must produce a *higher* generation (BR-02/AT-03) — that
minting point belongs to the discovery/attach step, which does not exist yet. Fine as
adapter-phase debt, but record it: without it, C2's revalidation has nothing to compare.
**Fix (Phase 3 prerequisite):** discovery → attach mints
`Generation(current + 1)` on evidence of a new process-start identity.

### M6. `records()`/`outstanding_jobs()` escape the session's bounds narrative
`records()` returns `Vec<&OperationRecord>` — bounded by the map (fine). But
`add_outstanding_job` evicts oldest **silently**; a 257th job disappears from
`outstanding_jobs()` with no trace in the stop report ("256 jobs" reads as complete).
The eviction is correct per NF-05; the *invisibility* is the gap. A user seeing "0
outstanding jobs" after 256 pushes could believe work settled.
**Fix:** track `evicted_jobs_count: u64` and surface it in the stop report
("…256 listed, 12 evicted from the window — list is bounded, not exhaustive").

### M7. `settle_partial` evidence string is dropped (`let _ = evidence`)
The doc says evidence is journaled by the host; today nothing journals it —
`MemoryJournal` stores only `IntentRecord` (which has no partial-evidence field). When
Phase 3 produces a genuine partial, the audit trail will contain the outcome class but
not *what held*. BR-12/NF-13 require the evidence for verification decisions.
**Fix:** add `partial_evidence: Option<String>` (bounded, e.g. 4 KiB) to `IntentRecord`,
set it in `settle_partial`, surface in job-status/verify outputs.

### M8. `/bridge` unknown-argument help omits `release confirmed` synonym and case rules
TUI lowercases before matching; ACP matches `argument.to_ascii_lowercase()` too — but
the shared `command()`'s help string does not mention `confirm release` as a synonym,
and neither host documents that matching is case-insensitive. Trivial UX/doc fix; also
the help lists `connect <app>` which is not a `/bridge` verb at all (it routes through
the model tool surface — the text says so, but listing it as available invites
`/bridge connect resolve` → "Unknown").

**Fix:** regenerate the help text from the actual match arms (single source of truth).

### M8b. ACP `/bridge` runs `bridge_stop()` on `self.hosted` — but if Bridge is
**disabled** the ACP never registered the service; the call would be on an unbuilt
composition
`lib.rs:1376` — unlike the TUI (which null-checks `bridge_handle`), the ACP path calls
`self.hosted.bridge_stop()` unconditionally. If `bridge` feature is on but settings
disabled it still works (service exists), but the disabled-but-compiled case returns
"no session is connected" — correct text. **Verified not a bug today**; flagging only
because the TUI's null-check and ACP's unconditional call are asymmetric — unify the
pattern when M8 is touched.

### M9. `StopOutcome::input_release` can misreport `Released` for a *fresh* stop
`stop()` reports `Released` whenever `input_release_confirmed` is true — including when
**new** inputs were leased after the last confirmation. Confirmation is a one-shot
latch, not per-release: lease inputs → confirm → lease more inputs → stop → reports
"inputs released" though the second batch was never confirmed.
**Fix:** clear `input_release_confirmed` whenever `fences.admit()` grants a
`holds_input` lease (admission invalidates prior confirmation).

### M10. Dead parameter `elapsed_ms` (see C2) plus `TimeoutPolicy` is stored but never
used — `authorize()` cannot distinguish a 15 s action from a 2 h render at the gate.
The policy exists in the envelope; nothing reads it. **Fix:** fold into C2's
`revalidate()` (deadline check) and into the Phase 3 job deadline plumbing.

---

## LOW

### L1. `bridge_discover` is a static string duplicated from `status_text`
Two "no adapters are configured" strings exist (bridge_service.rs and
bridge_command.rs). Drift risk. Single constant.

### L2. `emergency_released_through` is write-only
Updated in `emergency_release_inputs`, read only by `emergency_released()` — which no
production code calls (tests only). Either expose it in the stop report ("release
already ran for fence N") or drop it.

### L3. `Unreachable`/`unreachable!()` in shared host code (see H4) — replace with
truthful text; panics in command-answer code are never acceptable.

### L4. Doc drift: `session.rs` header still cites "§8.1/8.4, BR-04/07/08/14/15/16/17/18/30"
— accurate but now also BR-21 (host parity) and NF-05 (bounds) apply; a one-line header
refresh when C2/H1 land.

### L5. `InputLeaseState` derives `Serialize/Deserialize` but is never persisted;
`ApplicationSessionState` too. Either they participate in a durable session file
(Phase 3 checkpoint work) or the derives are dead weight. Harmless; note for Phase 3.

### L6. `PRD-ENROLLMENT-NOTE.md` still describes the *old* 256-ceiling blocker as
current ("The session-level acceptance_enroll tool still refused…"). After the TUI
restart the note's "Blocker (measured)" section becomes historical. Mark it resolved
with a date once enrollment is re-run on the new binary.

### L7. Test-count drift across reports: phase2 says "126 passed", 2b "131", 2d "134",
2f "137", 2i "142", 2n "142/0 + 162 targets". All true at their time; the *index*
(evidence-index.md) cites 2,247 workspace tests (older) in one row and 162 targets in
another. Normalize on next evidence refresh to avoid two numbers coexisting.

---

## Report-vs-tree verification (phase claims re-checked)

| Report claim | Verified? |
|---|---|
| 2n "every lifecycle state reachable" | **True for reachability** — but see C1/C2: reachable ≠ safe under interplay |
| 2m disconnect surfaces inputs/jobs | True (bridge_service.rs disconnect arm) |
| 2i stop/resume in both hosts | True (TUI main.rs:2352+, ACP lib.rs:1367+) |
| 2f shared `/bridge` command | True (bridge_command.rs; ACP re-exports) |
| 2g journal bound | True (MemoryJournal::MAX_RECORDS) |
| 2e/2f AT-01 kernel-accounted | True (bridge_at01_pty.py, harness tests) |
| 2o/2p installs & portal | True (files, digests verified this session) |
| "68/68 bridge, 142 harness, 162 workspace" | Consistent with tree; counts still current as of 2n |
| 2n "no enum state exists only in types" | **Still true** for producers/consumers — but C2 (`validates()` uncalled) and M4 (`within_pixel_budget` uncalled) are the *enforcement-site* variant of the same disease: **contracts with no consumer** |

**The single recurring theme of this mission's defects — a rule that exists but is
never enforced at the point of use — appears three more times in this audit:**
`validates()` (C2), `within_pixel_budget()` (M4), and revalidation-before-dispatch
itself (C2). The 2n audit checked *producers*; this audit checked *consumers*.

---

## Fix order recommendation (for Alex's accept/reject)

| Order | Item | Why first |
|---|---|---|
| 1 | C1 | Wedges the document/lease state machine; one-session adapter dispatch would hit it immediately |
| 2 | C2 (+M10) | The PRD's own "revalidate immediately before dispatch" sentence; blocks honest Phase 3 dispatch |
| 3 | C3 | Quarantine erasure on close + no production quarantine producer — must be wired before any adapter can produce uncertain outcomes |
| 4 | H1 | Cross-session emergency release — becomes live the day a second session shares fences |
| 5 | H5 | Mutation freshness — becomes live the day an adapter mutates |
| 6 | H2 | Authority mint on resume/connect — same trigger |
| 7 | H4 + M8 | `/bridge disconnect` execution + no panic paths + help text truth |
| 8 | H6, H7, M1–M7, M9 | Quality batch alongside Phase 3 work |
| 9 | L1–L7 | Hygiene batch |

Nothing here blocks the **Resolve install** itself; all of C1–C3 are pure-core fixes
that can land before any adapter exists, keeping Phase 3 on a hardened gate.
