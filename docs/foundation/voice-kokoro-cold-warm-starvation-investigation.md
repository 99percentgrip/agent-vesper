# VRO-17 — Kokoro Cold-vs-Warm Playback Starvation Investigation

Investigation unit: 2026-09-23/24. Baseline `main` @ `8f258ba` + the
dirty VRO-17 voice tree. **R6 remains OPEN and unchanged.** The
pronunciation repair was verified intact before starting (terminating
newline present; 4/4 pronunciation tests green; one duplicated comment
block from the prior edit deduplicated — no logic change; candidate
`agent-vesper-tui-pronunciation-repair` = `3b1bb9d3…` unchanged).

## User observation

An early short spoken response had buffering-like stops; later in the
SAME process a much longer multi-sentence prompt sounded natural and
continuous (and the extended connected-speech test had no pronunciation
mistakes). Hypothesis to test: cold/warm or startup starvation.

## Method

Fixed texts only (mission-specified short sentence + paragraph), no
provider. Harness: `apps/agent-vesper-tui/examples/cold_warm_starvation.rs`
(retained as the measurement instrument) driving the REAL
`SpeechWorker` → Kokoro engine (`2` intra-op threads) → depth-2 bank →
`PlaybackOwner` chain, with a **real-rate paced no-device player double**
(32,000 B/s = 16 kHz s16, timestamping first byte and EOF). Cold = run 1
in a fresh process; warm = immediate repeats reusing the same
worker/engine (production F9 host lifetime). Production-shape mode: the
job is enqueued at worker spawn (a fast short reply arrives while the
lazily-constructed host is still building).

## Primary metric

`playback_end(piece N) → first_audio_start(piece N+1)` — operationalized
through the paced player's own timestamps: **if playback ever starved,
the player's EOF time would exceed the total audio duration** (it waits
for data); continuous delivery shows EOF earlier than audio duration by
the pipe head start (64 KiB pipe).

## Results (12 runs: 2 voices × 2 texts × 3)

| Run | audio (s) | player EOF (s) | slack | Verdict |
|---|---|---|---|---|
| michael/para cold | 29.32 | 26.36 | +2.97 | CONTINUOUS |
| michael/para warm ×2 | 29.32 | 25.08/25.07 | +4.25 | CONTINUOUS |
| heart/para cold | 25.57 | 21.95 | +3.63 | CONTINUOUS |
| heart/para warm ×2 | 25.57 | 21.91/21.89 | +3.67 | CONTINUOUS |
| michael/short cold | 4.30 | 3.41 | +0.89 | CONTINUOUS |
| michael/short warm ×2 | 4.30 | 3.41/3.41 | +0.89 | CONTINUOUS |
| heart/short cold | 3.90 | 3.11 | +0.79 | CONTINUOUS |
| heart/short warm ×2 | 3.90 | 3.11/3.11 | +0.79 | CONTINUOUS |

**Starvation events: cold = 0, warm = 0.** Identical under 4 concurrent
CPU burners (still EOF 3.41 s vs 4.30 s audio). The production F9 PTY
loop agrees (`r3_loop` PASS; continuous PCM per piece).

## Cold vs warm — what IS different

| Metric | Cold | Warm (2nd) |
|---|---|---|
| Engine build (host construction) | 0.33–0.99 s (once, overlaps capture in production) | 0 (reused) |
| First piece synthesis complete | 2.6–3.8 s from enqueue | 2.2–2.4 s |
| First-piece RTF (short text) | **1.24–1.38** | 0.91–1.07 |
| Paragraph successor RTF | 0.78–0.99 | 0.79–0.93 |

Cold adds ≈ +1.3 s to the FIRST inference (ORT graph warmup), not to
steady state. Onset RTF > 1 for short first pieces is real, but the
64 KiB pipe head (≈2.0 s) plus the depth-2 bank absorb it in every
measured configuration — no audible-gap mechanism survived measurement.

## Worker/session lifetime (traced)

- Per **process**: one `ConversationHost` (built lazily on the FIRST
  enabled F9 gesture; never at startup; dropped on disable/session
  switch/shutdown) → one `SpeechWorker` → one Kokoro `EngineState`
  (ORT session + vocab) + one espeak-ng-per-pass phonemizer.
- Per **turn**: controller/session state only; the engine, worker, bank
  and player are reused across turns (the warm model).
- Per **speech segment/piece**: one espeak process + one ORT inference
  (serialized on the session mutex); PCM banked (≤2 units, ≤16 MiB/piece).
- **Preview** keeps its own worker for the pack-screen lifetime
  (documented earlier); it does not warm the F9 host.
- No initialization is repeated per turn; no worker is dropped between
  F9 turns.

## Hardware evidence

20-core machine; during synthesis the harness shows ~120–190 % CPU,
RSS ~190–390 MB, no player underrun/xrun messages (double + production
PTY both clean), ORT at its configured 2 intra threads. Smooth warm
repeat on the same machine/voice/load with similar utilization →
**hardware: UNLIKELY** as the owner.

## Root cause classification

**NOT_REPRODUCED** (as playback starvation). The measured mechanism
underneath Alex's perception of "stops" in an early short reply is the
**cold first-piece onset**: first byte of audio arrives ≈2.4–3.8 s after
the reply text is ready (engine build where capture did not cover it +
first-inference warmup + first-piece synthesis), and for short first
pieces the onset RTF exceeds 1. Within-speech boundaries measured
CONTINUOUS in every cold/warm/voice/text/load configuration — the
depth-2 bank plus the player pipe head cover the cold transient. Longer
speech sounds smoother for the measured reason that steady-state RTF
(0.78–0.93) builds lead, exactly the steady-state-overlap behavior
already recorded for the bank.

Primary class: **NOT_REPRODUCED** (secondary, non-defect observation:
COLD_MODEL_OR_SESSION_INITIALIZATION + first-inference warmup cost
affects onset latency only; no Vesper-owned starvation defect found).

## Repair

**None applied.** No Vesper-owned defect was proven at the bank, bank
policy, admission, player-startup/drain, or scheduling layer. Per the
mission's repair policy, no buffer-depth change, no prebuffer, no
lifecycle change is justified by this evidence — depth 2 was never
insufficient in measurement, and increasing depth would only move the
measured-nonexistent pause. The existing lazy host construction already
overlaps engine build with capture in the ordinary F9 flow.

## Pronunciation regression

PASS — fix intact, 4/4 tests green; the investigation added no
pronunciation changes.

## Verification

Harness runs (12 isolated + loaded, re-run after the marker-file env
change: identical CONTINUOUS results) · production `r3_loop_pty` PASS
(stop-to-first-PCM 4.9 s = the cold-onset number, consistent) ·
`voice_pty`/`flm_f9_loop` untouched this unit (no code change; the
pronunciation-repair candidate remains current). Clippy `-D warnings`
clean for lib/bins/tests and the harness example; the pre-existing
`verify_repro.rs` example fails to compile under a voice-kokoro-only
feature set (it references `voice_flm` without requiring the feature —
pre-existing on the stashed tree too, out of scope here). No production
code changed → no candidate built; `agent-vesper-tui-pronunciation-repair`
(`3b1bb9d3…`) remains the latest. Diagnostic audio from the prior unit
still at `/tmp/vesper-pron-ab` (1.1 MiB).

## Residual honesty / follow-up options for Alex

1. If early-reply "stops" remain audible in real sessions, the next
   measurement should capture the **real first turn** with a
   listening-marked recording (the isolated harness cannot reproduce a
   gap; the difference would have to be turn-context load beyond 4
   burners, e.g. provider streaming + STT finalization + UI together).
2. If the cold **onset** (~2.4–3.8 s to first audio) itself bothers, the
   bounded option is preparing the engine at an earlier safe lifecycle
   boundary (e.g. on voice-enabled app entry rather than first F9
   gesture) — a separately scoped, explicitly owned change, not taken
   here without evidence it is wanted.
3. Short-reply piece granularity (one ORT call per sentence) has fixed
   overhead making tiny pieces' RTF > 1; merging sub-clause sentences
   would be a segmentation change — out of scope without Alex's ask.

R6 status: **UNCHANGED / OPEN.**
