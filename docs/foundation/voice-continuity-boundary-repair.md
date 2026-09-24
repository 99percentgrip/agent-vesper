# Voice continuity boundary repair — recording review, banked handoff, honest readiness

> **Scope:** VRO-17 recording-and-readiness review directive (2026-09-23). Alex
> supplied a silent WebM screen recording (`looks like every sentance.webm`,
> sha256 `ae1654f21b47b6474a8ef87c39657f686898c100b15c0273d4c644923cf051fa`,
> 513,486 bytes, 25.677 s, VP9 820×1244, no audio stream) and a Settings
> screenshot. This unit is the scoped repair that followed the review's
> reconnaissance + red-reproduction requirements. Alex's listening evidence
> (excessive pauses after sentences and within some sentences) is preserved as
> the acceptance boundary; the silent recording is UI/state correlation only.

## Objective

Correlate the recording's voice-stage states with the production owners,
classify the inter-sentence pause mechanism, and apply the smallest
evidence-backed correction in the existing owner — without touching
segmentation policy, the model/voice, TTS engines, Settings save semantics,
or playback ownership.

## What the recording shows (software displays, not acoustic measurement)

Sampled footer states, decoded against `voice_speech_worker.rs` stage labels:

| t (s) | Displayed state | Decoded stage |
|---|---|---|
| 0–1 | Synthesizing voice · 5.8–6.8 s / Sending PCM to player · 5.8–6.8 s | A successor sentence's synthesis running while earlier PCM is still being written |
| 3–5 | Waiting for playback slot · 2.1–4.1 s / Waiting for player drain · 0.4–2.4 s | Producer finished a piece and is parked in the handoff while the lane writes and the player drains |
| 6, 13 | Sending PCM to player · 0.5 s / 7.4 s | Next unit's write (7.4 s ⇒ a large sentence's PCM at real-time consumption) |
| 15 | Waiting for player drain · 2.0 s | Terminal drain of the last unit |
| 16+ | Voice playback finished | — |

The displayed answer text is already complete at 0 s while these voice stages
continue — consistent with all speech work happening after the reply was
fully on screen (VRO final-only route). "Waiting for playback slot" parked
during a concurrent drain is the signature of the producer being unable to
hand off prepared audio while the lane finishes the previous unit.

The segment identities are not visible in the footer; the recording alone
cannot establish acoustic pause durations, whether every sentence is
synthesized only after the previous one finishes playing, or the exact
transition timestamps. The mechanism below is established from code plus the
red reproduction, not from the video.

## Mechanism (code-level, reproduced)

The synthesis thread and the playback lane meet in a **zero-capacity**
`sync_channel`. A `send` completes only when the lane executes its `recv` —
which happens only after `write_piece` returns **and** `end_stream` finishes
its full player drain (close stdin → child consumes to EOF → exit → receipt).
Consequences, re-derived by hand after the first fix attempt was falsified by
my own test:

- A successor **slower than the previous unit's playback** (production
  Kokoro RTF≈0.66, per-sentence 2–7 s) lands uncovered: with depth 0 the
  lane waits for handoff → drain → reopen while that synthesis is still
  running. Dead air at the unit boundary.
- **Depth 1 is insufficient**: the unit behind the bank overruns
  identically (bank absorbs ~one unit's audio of synthesis time; a slow
  successor needs the previous unit *plus* the bank to cover it).
- The early-piece/onset rule resets per **turn** (`segment == 0`), not per
  sentence — by design; the 5.2 s first-successor cold gap measured in the
  continuity repair (`voice-continuity-phoneme-repair.md`) is exactly the
  uncovered-regime gap, and the recon's "bounded audio-duration preparation
  … only needed if sentence-level pieces plus the pipe still starve on
  device" is the authorized remedy this unit applies.

This matches the recording's `Sending PCM · 7.4s → drain → slot` pattern.

### Red-first regression (application path, no device)

`apps/agent-vesper-tui/tests/voice_speech_pipeline.rs`
`unit_boundary_gap_next_unit_synthesis_must_overlap_previous_drain` — real
`SpeechWorker`, controlled subprocess engine (0.2 s units; third unit 2.5 s =
uncovered regime), real-time-paced no-device player logging monotonic
start/EOF. Pre-fix:

```
unit 3 began 1.019s after unit 2's audio ended: next-unit synthesis did not
overlap the previous unit's playback+drain
```

(1.019 s ≈ the 1.0 s uncovered synthesis remainder + drain window, matching
the hand arithmetic.) First attempt used a 1.5 s successor and PASSED — the
covered regime — which falsified the initial "send blocks through drain"
mechanism claim; the corrected derivation is the one above.

### Correction (smallest, existing owner)

`run_worker` rendezvous: `sync_channel::<PreparedSpeech>(0)` → `(2)`, with a
truthful comment: bounded two-unit bank, never a turn buffer (admission stays
32 × 8192 bytes; 16 MiB per-piece cap unchanged), stale banked audio discarded
by generation without reopening the player, Stop/Replace invalidate by
generation as before. The old fixture assertion `!third_started` ("lookahead
bounded to one sentence") expressed the superseded policy and only passed
post-fix by a ~45 ms race; it now asserts the real bounds (overlap happens;
banked audio never starts a player; stale never speaks; FIFO recovery
preserved).

## Second regression found by the review-mandated PTY loop (pre-existing, repaired)

`r3_loop_pty.py` continuity mode failed on the current tree — and on the
delivered FLM candidate — while **both preserved older candidates PASS**
(the decisive isolation):

```
PASS (long-continuity-repair, continuity-phoneme-repair):
PASS: F9 -> real agent/provider wire -> hygiene -> Kokoro am_michael (continuity)
-> fixture player; PCM bytes=[358400, 277600]; peaks=[25064, 24531]; provider requests=1
FAIL (tree before this unit, 0:02): voice inference failed: sidecar
transcription failed … 'native running frame hid the upstream voice wait status'
```

Root cause: the FLM session's conversation-CPU adapter
(`SharedSidecarStt::new`) resolved **only** the installed voice venv, dropping
the documented `VESPER_PYTHON_PATH`/`GLM_VENV_PATH` precedence that dictation
F5 honors (`candidate_whisper_pythons`). Isolated PTY runs (and any explicit
user override) therefore fell back to the installed venv, where the uncached
`base` model download path fails — a silent CPU-route regression from the
previous session, caught here because this directive required the real PTY
loop, not only the FLM-specific one. Repair: `shared_voice_python()` restores
the same precedence tiers (existence checks only; lazy model load unchanged).
Post-fix the continuity PTY reproduces the preserved byte pattern exactly
(`[358400, 277600]`), and `off`, `list`, `preview` plus the FLM NPU PTY all
pass on the repaired binary:

```
PASS: F9 -> real agent/provider wire -> hygiene -> Kokoro am_michael (continuity)
-> fixture player; PCM bytes=[358400, 277600]; peaks=[25064, 24531]; provider requests=1
PASS: F9 -> real agent/provider wire -> hygiene -> Kokoro am_michael (off)
-> fixture player; PCM bytes=[52000, 124800]; peaks=[22327, 13045]; provider requests=1
PASS: F9 -> real agent/provider wire -> hygiene -> Kokoro am_michael (list)
-> fixture player; PCM bytes=[62400, 128800]; peaks=[21808, 20350]; provider requests=1
PASS: actual Settings Preview -> real Kokoro -> fake player; click-to-first-PCM=2156.7 ms;
peak=17758; no recorder/provider
PASS: Settings save -> Verify -> F9 -> FLM NPU adapter -> one agent turn;
provider requests=2; player PCM bytes=[92800]; CPU recognizer untouched
```

## Readiness presentation (screenshot conflation, repaired)

The screenshot line "Speech recognition acceleration: using CPU — flm-npu
verified and ready" conflated route availability with selection/use. The
defect was presentation-only (`machine_capability_lines()` ignored the
scope); the F9 gate already refuses strict-NPU-unverified **before** capture
(`GateDecision::Blocked`), so no silent dispatch exists. Changes in the
existing functions (`voice_accel.rs`, rendered by `settings_host.rs`):

- `machine_capability_lines(&VoiceScope)` now resolves through the SAME
  shared `execution_rows` rule the F9 gate and Preview use: a CPU-selected
  configuration says "CPU selected for next request — flm-npu verified and
  available, not selected"; a strict stage either resolves the verified
  route or renders "unavailable —" + the refusal (never relabeled CPU);
  Automatic stays the ordinary verified-only policy. "using now" is never
  claimed from a static picture.
- New last-request receipt: `record_last_stt_route` /
  `last_stt_route_line()` — recorded at the two real F9 transcribe sites
  (`selected adapter (<provider>)` via the adapter's real descriptor, or
  `CPU sidecar`), rendered as "Last recognition request ran via: …" or
  "not run in this session". A dispatch receipt, never an offload claim.
- The separately-selected TTS engine/voice already render independently;
  the synthesis line now uses the same honest wording.
- Required UI tests added (`voice_execution_policy.rs`):
  `presentation_separates_selection_from_availability_and_receipt`
  (CPU+NPU-ready, strict-ready, receipt precision) and
  `presentation_strict_npu_without_verification_refuses_never_relabeled_cpu`
  (refusal shape, draft-vs-saved with no scope rewrite).

Not observable from the screenshot: whether Alex's saved STT scope is CPU
(deliberately untouched — "Verification is not activation"; changing saved
state to make text consistent is prohibited).

## Test-isolation repairs (latent defects the new tests exposed)

- `voice_flm_route`: `readiness_is_pending_until_real_verification` depended
  on declaration order after `verification_record_promotes_readiness` sets
  the process-global `FLM_VERIFIED` atomic (serial-run red, parallel-luck).
  Fixed with explicit per-test state contracts plus the test-support
  `reset_flm_stt_verification_for_test` (documented `*_for_test` precedent)
  and a suite-local state lock.
- The nondeterministic `f9_cpu_scope_never_refuses` failure (passed 3×,
  failed once) is the chdir race: `F9Root::new` mutates the process-global
  working directory from parallel threads. `F9Root` now holds a guard for
  its lifetime (distinct from the state lock).
- `voice_policy_parity`'s documented "run single-threaded" chdir flake: the
  same guard pattern on `TestRoot` makes it pass 4/4 fully parallel; the
  single-threaded note is removed from its DOX entry.

## Files

- `apps/agent-vesper-tui/src/voice_speech_worker.rs` — bank depth 0→2 + truthful bounds comment
- `apps/agent-vesper-tui/src/voice_shared_stt.rs` — interpreter precedence restored
- `apps/agent-vesper-tui/src/voice_accel.rs` — scope-aware capability lines; last-route receipt; dead `permit_label` removed; test-support reset
- `apps/agent-vesper-tui/src/voice.rs` — receipt recording at both transcribe sites; slot doc corrected (slot carries the selected CPU adapter too; one recognizer per process either way)
- `apps/agent-vesper-tui/src/settings_host.rs` — readiness panel renders receipt line
- `apps/agent-vesper-tui/tests/voice_speech_pipeline.rs` — boundary-gap regression; superseded-lookahead assertion updated honestly
- `apps/agent-vesper-tui/tests/voice_execution_policy.rs` — presentation contract tests; explicit state contracts
- `apps/agent-vesper-tui/tests/voice_flm_route.rs` — state lock + chdir guard
- `apps/agent-vesper-tui/tests/voice_policy_parity.rs` — chdir guard (parallel-stable)

## Verification receipts

```
lib (voice-conversation,voice-kokoro,voice-flm)   281 passed
voice_speech_pipeline                             2 passed (red→green above)
r3_speech_worker                                  6 passed
voice_interruption_lifecycle                      5 passed
voice_multiturn_playback                         11 passed
voice_execution_policy                           12 passed
voice_policy_parity                              7 passed ×4 parallel
voice_flm_route                                  11 passed ×4 parallel
vesper-voice (dev, all suites in scope)        86/13/13 passed
clippy --all-targets (3 features)                 0 warnings
cargo fmt --check                                 clean
default build (no voice features)                 Finished, clean
PTY: r3 continuity/off/list/preview + flm_f9      all PASS (receipts above)
```

Candidate (release, `voice-conversation,voice-kokoro,voice-flm`, byte-identical
to `target/release/agent-vesper-tui` at delivery):

```
target/voice-candidates/agent-vesper-tui-continuity-boundary-repair
sha256 fa0c371305fc95bc59b0c6155549e35e9770016beeba39b36ed62adf2b72a3c7
```

All prior candidates preserved (`voice-cpu-acceptance` → … →
`flm-npu-stt-candidate` 3c634c73…).

## Deviations and corrections recorded in-place

- The first fix attempt (depth 1.5 s test) passed wrongly and forced a
  mechanism re-derivation; the corrected claim (handoff/drain serialization,
  uncovered-regime arithmetic, depth-1 insufficiency) is what the code and
  tests now state.
- One edit introduced a broken placeholder (`unsafe_placeholder`); it was
  replaced in the same session before any commit, and the final signature is
  `impl Into<String>` — noted because delivery summaries must match code.

## Unresolved / open

- Subsequent user verdict: see [`voice-npu-user-acceptance.md`](voice-npu-user-acceptance.md)
  (2026-09-23) — Alex reports the tested NPU-enabled build sounds continuous
  and natural; this unit's open acoustic-listening boundary is superseded as
  a present status by that record and kept here as history.

- **Alex's listening acceptance** on the repaired build (longer replies +
  interruption, CPU and verified-NPU STT routes) — acoustic continuity can
  only be certified by an audio-bearing user observation. A successful
  paced-sink run or PTY receipt here does not establish audibility.
- Natural-speech (non-surrogate) recognition accuracy remains unmeasured.
- The ~745 MB partial Llama residue and the vendor fallback-download
  clarification remain pending Alex's decisions.
- Unrun here: full-workspace suite, MSRV, five-target matrix, CI.
- Placement evidence remains process/device/model correlation (no
  per-request offload field exists); no "full-NPU" claim is made.
