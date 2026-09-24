# VRO-17 PR-3 Execution — Voice-Session Orchestration, Interruption, Bounded State

Work unit: `VoiceSession` orchestration, STT/TTS port coordination,
host-facing effects/events, interruption context, truthful turn
reports — **PR-3 only, stopped at the gate.** No host wiring, capture,
playback, F5/dictation changes, Settings, ACP, downloads, cloud
selection, NPU, binary replacement, or release. Orchestration is
exercised through controlled in-memory doubles; **a simulated
conversation is not device-level voice.**

## 1. Baseline and reconciliation

- Revision `94ed16de502e98110498010b399b24659b17a63f` on `main`;
  uncommitted VRO-17 work (PR-0..PR-2 + docs) preserved untouched
  (dirty paths grew only by this unit's files). Toolchain rustc/cargo
  1.95.0; features at start: `stt-sidecar`, `stt-http`, `tts-subprocess`
  (all default-off).
- **Reported totals re-verified fresh**: voice crate
  `--features stt-sidecar,stt-http,tts-subprocess` → **137/0 confirmed**
  before any PR-3 code (matching the supplied summary). Final state:
  **166/0** (75 unit + 13 pr0 + 30 pr1 + 19 pr2 + 29 pr3). Workspace
  all-features final **2633/0** (172 targets) — grown from 2604 by the
  29 PR-3 tests; default 2478/0; doc 28; dictation **8/8** re-confirmed.
  Each change in totals is accounted: new tests only, none removed.
- PR-2 storage-receipt clarification (directed): the report's rounded
  `/home` free-space figures and the ~71 MiB `target/` directory growth
  are **different measurements** (available-bytes vs directory usage)
  and were never equivalent deltas; this unit therefore records **exact
  bytes with timestamps** and does not restate the PR-2 comparison as a
  corrected baseline.

## 2. Storage receipt (exact bytes, ISO timestamps)

| Measurement | Before (2026-09-18T23:09:14+08:00) | After (2026-09-19T00:03:10+08:00) |
|---|---|---|
| `/home` avail bytes | 290 229 809 152 | 290 016 071 680 (−213 737 472 B ≈ 203.7 MiB build/cache growth in existing `target/`) |
| `/tmp` avail bytes | 14 269 308 928 | 14 242 856 960 |
| `target/` bytes | 1 328 560 112 677 | (pre-PR-3 measurement; after-growth included in the `/home` delta above — `target` is on `/home`) |
| voice-owned `/tmp` residuals | 4 964 813 B (pre-existing PR-1 artifacts; **not removed** — no cleanup approval) | unchanged for pre-existing; PR-2 test-fixture dirs from earlier runs were our own owned test artifacts and were cleaned (see §7 deviation) |
| Production audio/transcript/cache files created by PR-3 | — | **0** (pure orchestration writes nothing; no disk spooling introduced; no new persistent voice log) |

Concurrent activity acknowledged: Alex's two running TUI instances and
normal system activity share these filesystems; deltas are not
attributed exclusively to PR-3. Build growth is separated from runtime
storage (runtime = 0). Existing `target/` reused; no duplicate trees,
no destructive cleanup, no cache-clearing commands. Free-space reserve
(≥1 GiB) trivially satisfied throughout (≈270 GiB free).

## 3. Implemented (`crates/vesper-voice/src/session.rs`, ~830 lines)

- **Reducer-style `VoiceSession`** over the PR-0 `HostEvent` vocabulary,
  producing typed **`VoiceEffect`s** (`SubmitTurn`, `CancelRuntimeTurn`,
  `SynthesizeUnit`, `CancelSynthesis`, `StopPlayback`) and
  `VoiceClientEvent`s. No generic event bus; follows the repository's
  state/effect ownership pattern. The host is the only executor — the
  test host records and acknowledges. Core imports no runtime/provider
  types (arch gate: unchanged 29 packages, allowlist intact).
- **Five concurrent dimensions** kept as independent state (capture,
  STT, agent, per-unit synthesis, playback); `phase()` is a derived
  projection. Overlap is asserted by tests (speech dispatched during
  generation; `Speaking` survives runtime settlement).
- **Turn coordination**: capture→final STT→**exactly one**
  `SubmitTurn` per accepted turn; partials are display-only (no
  submission path exists for them); `NoSpeech`/`LegacyEmpty`/
  `Failed`/oversized-input produce a terminal report **without** a
  runtime turn (PR-1 D21 provenance honored); STT finals are delivered
  by the host via `submit_stt_final` (the session never blocks on
  inference in a handler); the PR-2 hygiene gate is created per turn and
  fed **exactly once** (unit order preserved; protected spans never
  reset to bypass overflow); `flush_speech` exists only for legitimate
  boundaries (turn settlement / interrupt).
- **Interruption**: barge-in/Stop emit **urgent effects first**
  (`StopPlayback`, `CancelSynthesis` — never serialized behind slow
  cleanup or runtime settlement), then `CancelRuntimeTurn` through the
  established contract; runtime-settled + speech-remaining stops speech
  **without** pretending agent work rolled back; **cancellation before
  run identity preserves intent** (`cancel_before_identity`) and fires
  exactly against the late matching run (never a subsequent one —
  tested); repeated Stop is **idempotent** (interrupted-flag short-
  circuit, tested ×3); late text/PCM after freeze are rejected while
  old-run settlement is still consumed as evidence (tested); no
  voice-originated tool replay exists (type-level exhaustion test);
  pending input during unsettled interruption gets **bounded queueing
  (at most one) with visible rejection** for further input.
- **Playback receipts**: identity/generation validation (unknown
  segment ignored), monotonicity (regressing acks ignored),
  per-unit `acked_bytes`, `PlaybackUnavailable` → `Unknown` retained
  (never upgraded to heard); interruption cutoff freezes evidence at
  interrupt time; the **bounded note is composed once** from
  acknowledged sanitized unit text only, qualified honestly when only
  device-handoff or nothing is known ("delivered through: …" only when
  a unit's full text was acked; "before any sentence was fully
  delivered" otherwise), carried as ordinary next-turn content
  (bracketed, exactly-once — tested), never into operational logs.
- **Reports**: `VoiceTurnReport` populated from actual events with the
  injected `SessionClock` (`HostClock` provided; tests use a fake —
  ordering evidence only, never device latency); unobserved stages
  absent (no playback stage without playback, asserted); metadata-only
  (no transcript/PCM — asserted by serialization).
- **Bounds**: whole-capture budget with visible `ResourceExhausted`;
  hygiene overflow loud (validated boundary, never unvalidated flush);
  pending-speech bounded; Stop serviceable under full queues (tested
  with the queue filled); reports bounded. No second model/sidecar is
  instantiated — the session holds one STT port handle (shared-provider
  ownership preserved for PR-4); request-scoped cancellation cannot
  poison future turns (10× cancel/restart loop, no accumulation).

## 4. Reachable-state-by-event contract (documented table)

States = {Idle, Capturing, Transcribing(→final), Thinking(submitted,
unsettled), Speaking(+units), Interrupted(awaiting settlement),
Settling(audio draining), Done}. Per event: legal transitions, and the
invalid/duplicate/stale/out-of-order handling (all tested):

| Event | In state | Behavior |
|---|---|---|
| CaptureStarted | Idle | new turn |
| CaptureStarted | active, not interrupted | barge-in path then new turn |
| CaptureStarted | interrupted, unsettled | bounded-queue first input; visible rejection after |
| CapturedAudio | Capturing (budget ok/exceeded) | append / loud ResourceExhausted |
| CapturedAudio | any other | ignored (no active capture) |
| CaptureStopped | Capturing | STT dimension → running (host drives port) |
| CaptureStopped | otherwise | Failed(InvalidInput) — never fabricates a turn |
| submit_stt_final | Transcribing | submit (InferredText non-empty) / terminal-no-turn (silence, legacy empty, error, empty) |
| AgentText | submitted turn | gate push → units (effects) / overflow loud |
| AgentText | no gate / finalized | Failed — rejected, never spoken |
| AgentTurnIdentity | first for turn | recorded; deferred cancel applied **to this run** |
| AgentTurnIdentity | duplicate | ignored |
| AgentTurnSettled | unsettled | flush tail; close iff no pending audio; else Speaking |
| AgentTurnSettled | settled/none | ignored (duplicate: no second report) |
| PlaybackAck | known segment, monotonic | advance |
| PlaybackAck | unknown/non-monotonic | ignored |
| PlaybackUnavailable | any | Draining→Unknown (never heard) |
| DeviceOrProviderFailure Playback/Provider | speech active | stop speech; if settled → voice-failure + report (settlement preserved); else speech-only failure |
| DeviceOrProviderFailure Capture | capturing | Failed; no runtime turn created |
| BargeIn / StopRequested | active, not interrupted | urgent stop+cancel-synthesis → note freeze → runtime cancel (or deferred) → possibly close |
| BargeIn / StopRequested | already interrupted / settled+stopped | idempotent no-op |

Meaningful invariants (not exhaustive-concurrency claims): exactly-one
submission per accepted turn; exactly-one terminal report; no report
for never-accepted turns; stale data rejected while settlement
evidence retained; urgent effects precede slow effects; bounded
everything; metadata-only telemetry.

## 5. Test evidence (`tests/pr3_session.rs`, 29 tests, all families)

**A Normal/negative** (8): exactly-one submission; units dispatched
during generation with order; duplicate settlement no double report;
silence/legacy-empty never submit but still report; STT failure → no
runtime turn; runtime completion with pending audio doesn't close
early; all-omitted speech reports cleanly; partials display-only.
**B Interruption races** (8): urgent-effect ordering; idempotent ×3
Stop; pre-identity defer → exact late-run cancel (subsequent run
untouched); stale text rejected post-freeze while old-run settlement
consumed; interrupted-run settlement → one report; type-level no-replay
exhaustion; cross-session isolation (note never leaks).
**C Playback/failure accounting** (7): out-of-order receipt does not
advance past a gap (full-flow note assertion: gap ⇒ no "delivered"
claim); non-monotonic ignored; unknown segment ignored;
PlaybackUnavailable ⇒ honest qualified note; mid-stream playback
failure after settlement preserves `Completed` in the report.
**D Resource/privacy** (6): Stop serviceable with queue full; bounded
pending input with visible rejection; 10× cancel/restart without
accumulation; hygiene-before-synthesis (secret never reaches a
`SynthesizeUnit`, redaction marker present); capture budget enforced
visibly; reports metadata-only with absent stages absent. Plus
phase-overlap projection and STT egress class assertions.

All against the **production `VoiceSession`**; the only doubles are the
host recorder, scripted STT port, and fake clock. Runtime-cancellation
regressions elsewhere in the workspace remain the runtime's evidence —
PR-3 proves voice *effects*, not the real host's mapping (PR-4's).

## 6. Verification (final state)

| Gate | Result |
|---|---|
| PR-3 suite | **29/29** |
| Voice crate (all 3 features) | **166/0** |
| Workspace all-features | **2633/0** (172 targets; re-run ×3 stable after the flake fix) |
| Workspace default | 2478/0 |
| Doc tests | 28 targets, 0 failed |
| Dictation regression | 8/8 |
| Strict Clippy / fmt | clean / clean |
| Architecture | 29 packages (allowlist unchanged; no new deps) |
| Naming guard | clean (36 frozen) |
| Acceptance | 23/23 (6.3 s) |
| Bare no-features build | clean |

## 7. Deviations and corrections (recorded, not hidden)

1. **Flaky PR-2 storage tests found and fixed** (surfaced by repeated
   PR-3 verification runs): two `pr2_tts` snapshot tests scanned the
   whole `vesper-voice*` temp namespace, so parallel tests' fixture
   engines raced into each other's before/after diffs; additionally
   each test process leaked one fixture dir per run (35 accumulated).
   Fixes: snapshots scoped to the adapter's own production namespace
   (`vesper-voice-tts*`, which the adapter never creates — the
   assertion is now about exactly that), fixtures documented as
   test-owned, and the accumulated **test-owned** dirs removed (our own
   artifacts; the 4.6 MiB pre-existing PR-1 residuals and all other
   data untouched). Eight consecutive runs green; workspace ×3 stable.
   This was a test-isolation defect, not adapter behavior — the
   adapter's zero-file property was never violated.
2. Session-implementation fixes driven by the new tests (red-first
   where behavior was new): interrupt idempotence short-circuit;
   bounded pending-input queueing with loud rejection (replacing the
   earlier drop-silently path the tests rejected); borrow-scoped
   interruption snapshot. All landed with their tests.

## 8. Open gates (unchanged; not satisfied by orchestration tests)

R3 local+cloud TTS (cloud vendor + final voice = Alex); **positive
real-speech STT receipt** (binding PR-4); **R20** capture-storage
acceptance (binding PR-4); cross-target/MSRV/CI (not run);
native-STT feasibility prerequisites; NPU (device-present/no-userland,
no claims); real-device interruption/playback behavior (doubles prove
logic only). The optional installed-engine baseline approval is
unchanged and un-broadened.

## 9. PR-4 readiness

Ready for bounded host integration: the host must map `VoiceEffect`s
to the real runtime (`SubmitTurn`→existing prompt submit,
`CancelRuntimeTurn`→existing transactional cancel), drive STT finals
through the PR-1 adapter (one shared instance — the session holds no
second engine), execute `SynthesizeUnit` via the PR-2 TTS port, and
emit honest `PlaybackAck`s from a separately designed playback owner.
Prerequisites for integrated acceptance: PR-4 wiring + Settings
activation, the real-speech STT receipt, R20 capture items, R9
baseline proof on deterministic surfaces, and the
assistant-audio-retranscription real-device case.

**Stop at PR-3.** The success claim: tested voice-session orchestration
around existing components — not a speaking TUI, not device-level
bidirectional voice, not verified NPU inference, not application-wide
SSD safety.

## Verification

- §6 gates run on the final source state; storage numbers are exact
  bytes with timestamps; no caches cleared; no model downloads; no
  microphone/speaker use; running instances untouched; historical
  receipts preserved.
