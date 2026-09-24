# VRO-17 — Short-reply quality and Voice Settings presentation repair

Work unit: 2026-09-24. Source baseline: `main` at `8f258ba28f4e` plus the
existing uncommitted VRO-17 tree. Scope: the two remaining user-observed voice
issues only. PR-5, release/version/tag work, installation, and live public
provider traffic were not started.

## Objective

Investigate the production voice path without assuming a buffering cause, fix
occasional audible stops in short Kokoro replies, clarify the confusing partial-
transcript presentation, and preserve CPU/NPU STT, F5/F9, barge-in/Stop, R20,
R4 provider capability rules, reasoning-provider neutrality, pronunciation and
accepted long-form playback.

## Root cause

### Short Kokoro stops

The existing cold/warm investigation had already measured zero player-
starvation events across voices, text sizes, repeats and CPU load. Re-running
the current paced production worker likewise showed bounded delivery. Buffer
depth, player recovery and warm-session reuse were not the owner.

The remaining stop reproduced in the full production shape:

`HygieneGate → two SpeakUnit segments → SpeechWorker → real installed Kokoro → PlaybackOwner file sink`.

For `Please reply now. I am ready for the test.`, hygiene correctly emitted two
sentences. Before the repair, the real Michael waveform carried **0.972 s** of
low-amplitude padding across that boundary: **0.561 s trailing + 0.411 s
leading**. This is fixed per-inference Kokoro envelope padding, so it dominates
short speech and occurs only for reply shapes that cross a synthesis boundary.
That content dependence explains “occasional.”

An initially broad edge trim was rejected. It reduced the audio lead that hides
long successor inference and caused **2.153–2.211 s** paced starvation gaps in
the accepted long passage. This red result established that long-form envelope
bytes are part of the working buffering margin and must remain intact.

### Voice Settings wording

Every current STT adapter is truthfully final-only. The production row said:

`Partial transcripts · unavailable for the selected speech-recognition backend`

Only the optional interim display is unavailable; final transcription remains
healthy after Stop. The row therefore conflated a missing optional preview with
recognition availability and used an internal term (“partial transcripts”) as
the primary user label.

## Exact fix

`voice_speech_worker.rs` now applies conservative PCM edge cleanup only when:

- the selected engine is Kokoro neural voice; and
- the hygiene unit is at most 64 characters.

It detects the measured low-amplitude envelope at absolute sample amplitude
below 400, preserves **50 ms** on each artificial within-unit join and **100 ms**
on each real sentence edge, and leaves the first reply onset intact. A buffer
with no sample above the threshold is returned unchanged, so wholly quiet or
very-soft output cannot be erased. System speech and long Kokoro units are
unchanged.

`voice_accel::partials_settings_row` now renders:

`Live transcript preview · not supported by this recognizer (final text still appears after Stop)`

A future capable backend uses `Live transcript preview · current ON/OFF
(mode)`. The R4 capability authority, non-flippable final-only behavior and
saved preference are unchanged.

## Red → green evidence

1. The pure PCM guard regression failed against the no-op implementation:
   expected 14,400 bytes after bounded trimming, observed 22,400 bytes. It
   passes with neural-only guarded cleanup and also asserts system speech is
   byte-preserved.
2. The production Settings-row regression failed with the old output on the
   left and the new final-transcript-explicit output on the right; it now passes.
3. The real full-path Michael receipt failed at **0.972 s** stacked boundary
   quiet. The repaired path passes at **0.200 s** (0.100 + 0.100). Heart passes
   the same real-model receipt at **0.200 s**.
4. The rejected broad trim produced long-form gaps of **2.153–2.211 s** and
   26.218 s of retained audio. The final short-unit-only policy restored the
   three chunking cases to **0.277–0.452 s** maximum measured gaps and 30.254 s
   audio. The pre-change observation was 0.293–0.540 s and 31.125 s; the
   large new starvation regression is absent.

These are software/waveform receipts, not claims that a person heard the
candidate.

## Production-path and regression evidence

Commands executed after the final source repair:

```text
cargo test -p agent-vesper-tui --features voice-flm,voice-kokoro --lib --bin agent-vesper-tui --tests
cargo test -p vesper-voice-kokoro --features ort --lib --tests
cargo test -p agent-vesper-tui --no-default-features --lib --bin agent-vesper-tui
cargo clippy -p agent-vesper-tui --features voice-flm,voice-kokoro --lib --bin agent-vesper-tui --tests --examples -- -D warnings
cargo fmt --all --check
cargo run --release -p agent-vesper-tui --features voice-kokoro --example short_reply_boundary_receipt
cargo run --release -p agent-vesper-tui --features voice-kokoro --example short_reply_boundary_receipt -- af_heart
cargo run --release -p agent-vesper-tui --features voice-kokoro --example continuity_turn_receipt
```

Receipts:

- combined-feature TUI: library **282/0**, binary **158/0**, and every
  registered voice integration target green, including R6, R20, execution
  policy, provider neutrality, interruption, multi-turn playback and speech
  pipeline;
- Kokoro adapter: **42/0**, including all four pronunciation repairs;
- default/no-feature TUI: **251/0** library + **156/0** binary;
- strict combined-feature Clippy: clean; scoped/all-workspace fmt check: clean;
- paced long-form: all three input chunkings completed, with one bounded gap per
  run (**0.277–0.452 s**) and no broad-trim starvation recurrence.

Exact candidate production loops:

```text
python3 apps/agent-vesper-tui/tests/voice_pty.py <candidate> <voice-python>
python3 apps/agent-vesper-tui/tests/r3_loop_pty.py <candidate> <voice-python> am_michael off
python3 apps/agent-vesper-tui/tests/r3_loop_pty.py <candidate> <voice-python> am_michael continuity
python3 apps/agent-vesper-tui/tests/flm_f9_loop_pty.py <candidate> <voice-python> am_michael
```

- F5: PASS, including the managed 120-second/4-MiB R20 cap, progressing
  multi-chunk transcription, edit/retry/discard and cleanup.
- CPU F9 short reply: PASS, one provider request, real Kokoro, PCM
  `[40092, 100342]`.
- CPU F9 continuity: PASS, one provider request, PCM
  `[353600, 278400]` — byte-identical to the prior accepted automated
  continuity baseline.
- FLM/NPU F9: PASS, Settings save → Verify → selected FLM adapter → one agent
  turn → Kokoro PCM `[77554]`; CPU recognizer untouched.

Repository gates: architecture validated **30 packages**; naming guard was clean
at **36 frozen hits**; completion acceptance passed **23/23** exact cases with
zero live-model cost.

## Changed files

- `apps/agent-vesper-tui/src/voice_speech_worker.rs` — bounded short-unit
  Kokoro edge cleanup and unit regressions.
- `apps/agent-vesper-tui/src/voice_accel.rs` — clear Live transcript preview
  wording.
- `apps/agent-vesper-tui/tests/voice_provider_neutrality.rs` — production row
  regression.
- `apps/agent-vesper-tui/examples/short_reply_boundary_receipt.rs` — retained
  device-free real-model full-path receipt for both voices.
- applicable TUI/tests/examples/foundation DOX, this PRD, migration status and
  evidence index — current contracts and traceability.

## Candidate

- Path: `target/voice-candidates/agent-vesper-tui-vro17-voice-quality`
- Features: `voice-flm,voice-kokoro` (release profile)
- Size: `22,688,200` bytes
- SHA-256: `f3b736f6f757e387d3969d02718aaec28f0e11c4fc4fca6135e0b801df1a2e14`
- Byte-identical to `target/release/agent-vesper-tui` at build and after all
  candidate loops.
- Nothing was installed or used to replace Alex's current application.

## User acceptance

Alex subsequently retested this current voice-quality candidate and reported
**“Looks like smooth.”** This is user acceptance of the short-reply repair on
the tested setup. It is not converted into a numeric acoustic threshold,
universal gap-free behavior, or cross-platform acceptance. The pronunciation
repair remained accepted on the same tested path, with no new pronunciation
regression reported in this retest.

## Deviations and unresolved items

- This implementation unit opened no real speaker or microphone. Alex's later
  user-operated retest accepted the short-reply result on the tested setup; no
  broader acoustic claim is inferred.
- Current providers remain final-only. The Settings change clarifies that fact;
  it does not add live partial recognition or weaken R4.
- PR-5, commit/exact-commit CI, MSRV, cross-target/package/supply-chain/release
  gates were intentionally not started.
- Existing historical reports keep their original observations; this report and
  D26 own the current repair.

## Readiness effect

The two requested remaining voice-quality implementation issues are repaired,
locally verified on one immutable combined-feature candidate, and subsequently
user-accepted on the tested setup. VRO-17 public release readiness remains open
on PR-5 and its exact-commit external gates; the implementation unit itself made
no device-listening claim.
