# VRO-17 R6 — Real-Device Interruption / Recovery Acceptance — PASS

## Final device acceptance closeout (2026-09-24)

Alex completed the real-device interruption/recovery test after the
[production binding repair](voice-r6-binding-repair.md). The accepted observations
are:

| Observation | Result |
|---|---|
| A baseline speech | **PASS** |
| B one-press barge-in | **PASS** |
| B old speech stopped | **PASS** |
| B replacement turn submitted exactly once | **PASS** |
| B old speech resumed later | **NO** |
| C repeated barge-in/recovery | **PASS** |
| D explicit Stop / Ctrl+C | **PASS** |
| D capture started accidentally | **NO** |
| Later manual F9 remained usable | **PASS** |

Alex clarified that old canceled speech did not resume, Ctrl+C stopping speech
was correct, and the cancellation message shown in that tested case was
acceptable. Those observations supersede the earlier mistaken interpretation
that old speech resumed, explicit Stop opened capture, or a pre-audible
cancellation message was itself an R6 blocker.

The device observations and the binding repair's automated production-entry
evidence jointly cover the §2.4 contract:

- one Speaking+F9 gesture stops playback, invalidates synthesis, requests
  cancellation through the normal runtime path, and opens replacement capture;
- the replacement utterance submits one new turn, old speech does not resume,
  and a repeated interruption recovers;
- explicit Stop stops playback/synthesis and requests runtime cancellation
  without opening capture; a later manual F9 remains available;
- voice submission stays on the generic runtime/provider path, with no
  reasoning-provider-specific voice branch.

The user-visible test does not independently prove interruption-note audibility,
tool-replay behavior, every provider, every platform, or a numeric latency
threshold. Those are not inferred. The automated evidence establishes the
internal cancellation and stale-generation behavior; the device evidence
establishes the tested observable outcome.

**R6 verdict: PASS on the tested setup.** Real-device barge-in, repeated
interruption, explicit Stop, and post-interruption recovery are user-confirmed.
Old canceled speech did not resume, replacement turns submitted exactly once,
and later F9 voice remained usable.

## Current accepted audio baseline (separate from R6)

Alex described the latest short-reply retest as **“Looks like smooth.”** This is
bounded user acceptance of the short-reply quality repair on the tested setup,
not a numeric acoustic threshold, universal gap-free claim, or cross-platform
acceptance. The identified artifact is
`target/voice-candidates/agent-vesper-tui-vro17-voice-quality`, SHA-256
`f3b736f6f757e387d3969d02718aaec28f0e11c4fc4fca6135e0b801df1a2e14`,
as recorded in [the quality repair report](voice-short-reply-quality-repair.md).

The pronunciation repair also remains user-accepted on the tested path; Alex
reported no new pronunciation regression during the smoothness retest. This
does not expand the pronunciation claim beyond that path.

The interruption observations were supplied as behavioral results without a
separate executable checksum. This record therefore does not guess a distinct
R6 test-artifact identity or silently attach the quality candidate's checksum
to the interruption run.

## Historical acceptance-session record

The material below preserves the session as it existed before the production
binding repair and final device result. Its withdrawn brief and
implementation-open verdict are historical evidence, not the current verdict.

Acceptance record opened 2026-09-23 (late evening). This was an
**acceptance session**: no implementation is authorized or performed. All
production paths below were verified read-only before asking Alex to
test; if any had been unreachable, the session would have stopped with a
defect report instead.

## Tested artifact (established before the session)

- **Path:** `target/voice-candidates/agent-vesper-tui-r4-optional-partials`
- **SHA-256 (full):**
  `673f9fb8f82d4c5f5ec2ac421223999bf9e3e7d68e62d01dea4d8b4ba84ac181`
- **Profile/features:** release (optimized), `voice-flm, voice-kokoro`
  (implies `voice-conversation`). Byte-identical to the current
  `target/release/agent-vesper-tui` at build time.
- **Source:** `main` @ `8f258ba` + the dirty VRO-17 voice tree — the
  exact tree that produced the R4 candidate; contains the complete R6
  interruption stack + R20 capture repair + R4 capability Settings
  (verified in the binary: FLM route strings, child-registry, managed
  captures, "unavailable for the selected speech-recognition backend"
  row, "Speech stopped. Press F9 again to speak.").
- **Earlier candidates preserved** (cpu-acceptance, multiturn,
  long-continuity, phoneme, flm-npu, boundary, verify-read,
  r20-default-capture). Nothing installed; no binary overwritten.
- **Per-process NPU Verify:** required once per process **only if Alex
  selects the NPU route** for this test (Settings → Voice → Accelerated
  recognition → Verify). With the saved CPU scope, no Verify is needed
  and F9 uses the CPU recognizer. Either backend exercises the identical
  interruption path (interruption is backend-independent by design).

## Read-only production-path verification (this session)

**Explicit Stop / F9-while-speaking** (`voice_conversation.rs` +
`main.rs:4670-4686`): F9 during Speaking → `ConversationHost::stop_speech()`
→ `worker.stop()` (generation bump: queued jobs stale; in-flight inference
non-preemptible but its output discarded) + `PlaybackOwner::stop_flush()`
(kills + reaps exactly our child; queue cleared) → status "Speech stopped.
Press F9 again to speak." **No new capture is auto-started.**

**Session Stop path** (`ConversationController::stop_requested`):
playback flush **first** (urgent lane, never queued behind cleanup) →
`HostEvent::StopRequested` → effects: `StopPlayback`, `CancelSynthesis`,
`CancelRuntimeTurn` → the existing generic `turn_cancellation` token —
the same transactional path typed Ctrl+C uses. Provider-neutral.

**Barge-in**: F9 during speech stops speech within one tick; the **next**
F9 starts a fresh capture; its final transcript becomes exactly one
ordinary `SubmitTurn` through `dispatch_voice_submission` → the identical
typed-Enter seam. No keystroke simulation, no slash dispatch, no approval
reuse.

**Interruption note** (`session.rs::stage_interruption_note`): composed
**once**, from **acknowledged playback units only** (sanitized unit
text); when no unit was acknowledged it says *"[context: … playback
progress was not confirmed.]"* — honest uncertainty, never a "heard"
claim from synthesis completion. Bounded, ordinary context, no hidden
reasoning/tool args/secrets/never-queued text.

**Guards verified in source + existing suites:** stale-generation
rejection (`SpeechOutcome::Stale`; generation bump on stop/replace);
idempotent repeated Stop (`voice_interruption_lifecycle` five-cycle test:
exactly one submission + one synthesis dispatch per cycle, no
transcript/tool replay across cycles — transcripts-do-not-replay test);
no next-turn submission over an uncleared live turn (submit gating);
canceled-generation audio cannot resume (player killed + reaped; queue
cleared; `reset` only on a new stream); runtime cancellation is the
existing no-ambiguous-tool-replay contract (ADR 0007).

## Launch + setup for Alex

```sh
cd /home/Alex/Projects/agent-vesper
./target/voice-candidates/agent-vesper-tui-pronunciation-repair
```

*(Earlier launch text referenced the R4 candidate — the record's
original artifact at its date; the current baseline above supersedes it
for this session.)*

- Enter the coding surface (Enter on "Start coding").
- F9 = start/stop voice capture (the footer shows Recording/Stop).
- Saved scope today is CPU recognition + Kokoro `af_heart` — nothing to
  change. To test on the NPU route instead: `s` → Voice → Accelerated
  recognition → **Verify** (once, ~4 s) → Speech recognition compute →
  **NPU required** → Esc → Save changes. Either way works for R6.

## Tests to perform (speak naturally; no timing needed)

**A — baseline:** F9 → "Do not use any tools. Explain in several
sentences why Rust ownership helps prevent memory-safety bugs." → F9 to
stop → listen.

**B — barge-in:** during that spoken answer, press F9 and say: "Stop
there. Now answer only this: what was my interruption request?" → F9 to
stop.

**C — repeated interruption:** start another long no-tool answer, then
F9 + "Interrupt again. Reply with exactly: recovery works." → F9.

**D — explicit Stop:** during another spoken answer press F9 **once**
(stop-speech, not a new capture) and observe; then start a normal F9
turn afterward.

## Observation form (reply with exactly these)

- A baseline speech: PASS / FAIL
- B barge-in stopped old speech: PASS / FAIL
- B new turn submitted exactly once: PASS / FAIL
- B old speech resumed later: YES / NO
- C repeated interruption recovery: PASS / FAIL
- D explicit Stop: PASS / FAIL
- Later F9 turn still speaks: PASS / FAIL
- Anything strange heard/seen: <free text>

## Current-baseline re-verification (2026-09-23/24, pronunciation-repair candidate)

- Candidate re-hashed: `3b1bb9d34396b9b35371a88563a982c8b78ac0acb9118585157d8846d83b7e52`
  — **this unit's test artifact** (supersedes the R4 candidate named
  earlier in this record; that text remains as history below).
- In-binary verified: R20 managed-capture markers (`cap-`,
  `flm-child-registry`), the R4 capability row ("unavailable for the
  selected speech-recognition backend"), the R6 interruption stack
  ("Speech stopped. Press F9 again to speak.", playback stages), FLM
  route strings, and the Kokoro model identity. Byte-identical to the
  current `target/release` build; pronunciation newline fix present in
  source with 4/4 repair tests green.
- Read-only R6 path reconfirmation on current source: barge-in
  (F9-while-speaking → `stop_speech` → worker generation bump + player
  kill/reap/queue-clear → status "Speech stopped…"; **no auto-capture**;
  next F9 starts a fresh capture → final → exactly one typed-seam
  submission); explicit Stop (flush-first, then session effects → the
  generic `turn_cancellation`). Old audio cannot resume (generation
  invalidation + reaped player); `voice_interruption_lifecycle` 5/5
  (idempotent cycles, no transcript/tool replay) and
  `voice_provider_neutrality` 8/8 re-run green on this baseline.
- Live machine observation at re-verification: Alex is **currently
  running this exact candidate** (host PID 1218190, started 23:35, with
  its owned+registered FLM child on 18130). Untouched, as required.

## Pre-flight re-verification (session-ready, read-only)

- Candidate re-hashed unchanged:
  `673f9fb8f82d4c5f5ec2ac421223999bf9e3e7d68e62d01dea4d8b4ba84ac181`.
- FLM loopback range free; child registry empty; **no orphaned owned
  servers** (no device-context contention for the NPU variant).
- Alex's two live TUI sessions (installed binary) untouched — they are
  his, never terminated; the test candidate is run from its own
  terminal, so both can coexist (separate process, separate registry
  entries).
- No microphone/speaker/NPU/provider workload was started by this unit.

## PRE-ACCEPTANCE BLOCKER — §2.4 contract reconciliation (2026-09-24)

**Historical and superseded:** this source finding was correct for the
pre-repair tree. `voice-r6-binding-repair.md` subsequently repaired the binding,
and the final device evidence above closes its acceptance condition. It remains
here so the earlier incorrect two-press interpretation cannot reappear.

**R6 is reclassified: IMPLEMENTATION-OPEN (not device-acceptance-ready).**
Alex must NOT run the device acceptance until this mismatch is repaired.
No code was changed in this reconciliation unit.

### The binding contract (PRD §2.4, current amended text)

> Both `BargeIn` and `StopRequested` produce two separable effects —
> (1) stop speech output, (2) request transactional runtime cancellation
> — and differ only in that **barge-in immediately opens a new capture**
> while Stop does not.

### Actual production event mapping (source-traced)

| Gesture while Speaking | What executes | §2.4 classification |
|---|---|---|
| **F9 (conversation gesture)** | `host.stop_speech()` ONLY: worker generation bump + player kill/reap/queue-clear. **The session is never told** — no `StopRequested`, no runtime cancellation, no interruption-note staging. Runtime turn keeps running invisibly; status "Speech stopped. Press F9 again to speak."; `return` (no capture). | **Neither BargeIn nor StopRequested** — a speech-mute only. Violates §2.4: no runtime cancel, no new capture. |
| **Second F9 (after the mute)** | `gesture(CaptureStarted)` → session `on_capture_started`: active turn not settled, not interrupted → `on_interrupt(true)` = the session's **genuine BargeIn** (note staging + `CancelRuntimeTurn`) **and** the new capture opens in the same transition. | Genuine **BargeIn** — but reached on the second press, after a silent first-press dead zone. |
| **Ctrl+C (`cancel_turn`)** | `cancel_active_turn_preserving_partial`: runtime cancellation through the generic token. **Does not touch the voice host** — speech is NOT stopped (no worker stop, no playback flush). | Runtime-cancel only. Violates §2.4's effect (1) for both variants. |
| `ConversationHost::stop()` / `controller.stop_requested()` | Flush-first + session `StopRequested` + `CancelRuntimeTurn` — the **correct StopRequested implementation** (PR-3 tested). | Correct — but **no production call site invokes it** (grep: only tests). |

### Answers to the directive's questions

1. **Does production emit/execute genuine BargeIn?** Only on the *second*
   F9 press, via the session layer's `on_capture_started` interrupt path.
   The first Speaking+F9 press executes neither §2.4 variant.
2. **Is there another production gesture that performs genuine barge-in?**
   **No.** No production site sends `HostEvent::BargeIn` (grep: only the
   session's own handler and PR-3 tests). No other control performs
   stop+cancel+immediate-capture in one gesture.
3. Therefore: **R6 = implementation-open.** The prior "Tests A–D" brief
   (which described the two-press flow as barge-in) is **withdrawn** —
   it would have accepted a non-contract behavior.

### Smallest existing-owner correction (proposed, NOT implemented)

One owner: the F9 handler in `main.rs` (~line 4676). Replace the
stop-only block for `Speaking + F9` with the genuine barge-in:
`host.stop()` (flush-first, existing `ConversationHost::stop`, which
already routes `StopRequested` + `CancelRuntimeTurn` through the session)
**then immediately proceed to `gesture(CaptureStarted)`** instead of
`return` — i.e. delete the early-return and let the existing
capture-start flow run in the same keypress. Explicit Stop stays
stop-only by binding the existing `ConversationHost::stop()` to an
explicit control (Ctrl+C's `cancel_turn` should additionally call
`host.stop()` so runtime-cancel always stops speech — the same
flush-first ordering). This preserves: playback flush-first, generation
invalidation, transactional runtime cancellation, provider neutrality,
R20, R4, and the pronunciation repair; no new state machine.

Historical note (why stop-only exists): the in-code comment cites
"Alex's device round 3" — F9-while-speaking previously opened the
microphone via a path that staged a confusing interruption note and made
replies vanish. That failure came from the *old* flow's note staging,
which the PR-3 session now handles correctly (acked-playback-only,
once). The repair over-corrected to speech-mute-only; the §2.4 contract
was never amended to permit it.

## Status

**Current: R6 PASS on the tested setup.** The implementation-open statement in
the historical blocker section describes the pre-repair tree. The production
binding repair and Alex's later device observations supersede it. PR-5 remains
separate and was not started by this acceptance closeout.
