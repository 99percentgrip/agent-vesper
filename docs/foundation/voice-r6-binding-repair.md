# VRO-17 R6 Production Barge-In / Stop Binding Repair

> **Subsequent acceptance:** Alex completed the real-device matrix on
> 2026-09-24. One-press and repeated barge-in, explicit Stop, and later-F9
> recovery passed; old speech did not resume and Stop did not open capture.
> See [the device acceptance record](voice-r6-device-interruption-acceptance.md).
> The implementation unit below remains the source/automated evidence at its
> date.

## Objective

Repair the production gesture layer identified by the §2.4 reconciliation:

- one F9 press while the host is Speaking must execute genuine `BargeIn` semantics — stop playback, cancel synthesis, request transactional runtime cancellation, invalidate stale audio, and immediately open replacement capture;
- the gesture must not require a second F9 press or stage interruption context more than once;
- explicit Stop (`cancel_turn`, normally Ctrl+C) must stop active voice playback and synthesis and cancel the runtime without starting capture;
- non-voice Ctrl+C behavior must remain unchanged.

This unit did not perform microphone/speaker device acceptance and did not start PR-5. The earlier NPU listening record remains historical and is not evidence for this repair.

## Baseline and scope

- Repository revision at start: `8f258ba28f4ea2f749526fb32b5b180ecc7dcead` on `main`, plus the existing uncommitted VRO-17 working tree.
- Confirmed blocker: the historical production Speaking+F9 branch called mute-only `stop_speech()` and returned; Ctrl+C cancelled only the generic runtime.
- Existing owners retained: `VoiceSession::on_capture_started` owns genuine barge-in, `ConversationController` owns typed effects, `ConversationHost` owns speech/playback ordering, and the TUI owns key-to-host binding.
- No provider-specific branch, model change, Settings mutation, package installation, public provider call, installed-application replacement, commit, push, tag, release, or device acceptance was performed.

## Implementation

### One production F9 entry

`ConversationHost::apply_conversation_gesture(speech_active, capture_active)` now represents exactly one production F9 gesture. The production handler and the R6 integration test call this same method.

When speech is active and capture is not already active, it delegates directly to `barge_in()`. It does **not** compose `StopRequested` followed by `CaptureStarted`. `barge_in()` performs the urgent lane first:

1. marks old speech stopped;
2. calls `SpeechWorker::stop()`, which bumps the generation, flushes playback, and invalidates queued/in-flight synthesis;
3. calls the existing session-owned `CaptureStarted` transition, whose active-turn branch performs the single interrupt and immediately opens capture;
4. returns the one recorded runtime cancellation for execution through the existing generic runtime token.

Ordinary F9 capture start/stop continues through the same method. Its returned `capture_started` fact now comes from the host/session result instead of being reconstructed independently in `main.rs`.

### Explicit Stop

The production `cancel_turn` binding calls `InterruptionControl::Stop` whenever voice speech is active. That host operation flushes playback, cancels synthesis, runs the session's single `StopRequested` transition, returns no replacement capture, and leaves generic runtime cancellation on the existing Ctrl+C path. A completed runtime with speech still draining now reports `Voice playback stopped.` rather than the false `No active turn to cancel.`

Outside active voice speech, the pre-existing generic Ctrl+C path is unchanged.

## Red-first evidence

The stronger production-entry regression was deliberately mutation-checked before the final green run. In a temporary copy-on-restore edit, the production F9 entry was changed from `barge_in()` to `stop_and_cancel()`. The test failed exactly at the one-gesture contract:

```text
running 1 test
test speaking_f9_is_one_gesture_genuine_barge_in ... FAILED

thread 'speaking_f9_is_one_gesture_genuine_barge_in' ... panicked at .../voice_r6_binding.rs:119:5:
the same gesture opens capture

test result: FAILED. 0 passed; 1 failed; ...
RED_PROOF_EXPECTED_FAILURE exit=101 (sabotaged production F9 route to Stop)
```

The source was restored automatically, and the same suite then passed:

```text
running 4 tests
test non_voice_ctrl_c_leaves_voice_host_untouched ... ok
test explicit_stop_stops_everything_and_opens_no_capture ... ok
test speaking_f9_is_one_gesture_genuine_barge_in ... ok
test repeated_barge_in_cycles_recover ... ok

test result: ok. 4 passed; 0 failed
```

The tests prove, through the exact production host entry:

- one interruption event and one runtime-cancel request;
- same-gesture capture start;
- synthesis generation changes, making old work stale;
- old speech is suppressed;
- the replacement utterance submits once;
- interruption context appears exactly once;
- explicit Stop starts no capture;
- repeated barge-in cycles recover;
- a non-voice Ctrl+C leaves the voice host untouched.

The binary unit regression additionally passed:

```text
test tests::non_voice_ctrl_c_keeps_the_generic_runtime_cancellation_path ... ok
test result: ok. 1 passed; 0 failed
```

## Verification

All commands below were run after the repair. Logs are retained under `target/acceptance/r6-binding-logs/` (build artifacts/evidence cache, not durable source evidence).

### Targeted voice regressions

```text
voice_flm_route:             13 passed; 0 failed
voice_interruption_lifecycle: 5 passed; 0 failed
voice_multiturn_playback:    11 passed; 0 failed
voice_policy_parity:          7 passed; 0 failed
voice_provider_neutrality:    8 passed; 0 failed
voice_r20_default_capture:   11 passed; 0 failed
voice_r6_binding:             4 passed; 0 failed
voice_speech_pipeline:        2 passed; 0 failed
```

This covers interruption lifecycle, CPU/FLM route policy, continuity/playback, R20, R4, and two-provider neutrality.

### Pronunciation

```text
cargo test -p vesper-voice-kokoro --features ort --lib --tests

test result: ok. 42 passed; 0 failed
```

The four named pronunciation repairs all passed: final `now`, final `ready`, final consonants in `test`/`tool`, and mid-sentence controls.

### TUI suites

Combined feature command:

```text
cargo test -p agent-vesper-tui --features voice-flm,voice-kokoro \
  --lib --bin agent-vesper-tui --tests
```

It completed 17 test targets with zero failures, including:

```text
agent-vesper-tui lib: 281 passed; 0 failed
agent-vesper-tui bin: 158 passed; 0 failed
pr4_f9_gate: 6 passed; 0 failed
pr4_wiring: 10 passed; 0 failed
voice_execution_policy: 12 passed; 0 failed
voice_flm_route: 13 passed; 0 failed
voice_interruption_lifecycle: 5 passed; 0 failed
voice_multiturn_playback: 11 passed; 0 failed
voice_provider_neutrality: 8 passed; 0 failed
voice_r20_default_capture: 11 passed; 0 failed
voice_r6_binding: 4 passed; 0 failed
voice_speech_pipeline: 2 passed; 0 failed
```

Default/no-feature regression:

```text
cargo test -p agent-vesper-tui --no-default-features --lib --bin agent-vesper-tui

lib: 251 passed; 0 failed
bin: 156 passed; 0 failed
```

### Production binary harnesses

All harnesses used the new immutable candidate, isolated state, loopback provider/player/recorder fixtures, and existing verified local assets. They did not open a real microphone or speaker.

F5:

```text
PASS: mouse/F5, managed 120-second/4-MiB capture cap, >9-second progressing multi-chunk transcription, editable composer, retry/discard, early exit, disk failure and shutdown cleanup
```

CPU F9:

```text
PASS: F9 -> real agent/provider wire -> hygiene -> Kokoro am_michael (off) -> fixture player; PCM bytes=[52800, 124800]; peaks=[20247, 16064]; provider requests=1
```

Continuity/playback:

```text
PASS: F9 -> real agent/provider wire -> hygiene -> Kokoro am_michael (continuity) -> fixture player; PCM bytes=[353600, 278400]; peaks=[25064, 25919]; provider requests=1
Device/listening acceptance NOT performed. No fresh install; existing asset inodes reused.
```

FLM/NPU F9:

```text
PASS: Settings save -> Verify -> F9 -> FLM NPU adapter -> one agent turn; provider requests=1; player PCM bytes=[92000]; CPU recognizer untouched
```

This is the existing controlled synthetic no-device route regression, not device acceptance and not a new NPU placement claim.

### Static/repository gates

```text
cargo clippy -p agent-vesper-tui --features voice-flm,voice-kokoro \
  --lib --bin agent-vesper-tui --tests -- -D warnings
Finished `dev` profile ...

cargo fmt --check
PASS (no output)

cargo xtask architecture
architecture boundaries validated for 30 packages

cargo xtask naming-guard
naming-guard: clean (36 hits, all frozen in baseline)

cargo xtask acceptance
Acceptance regression gate: 23 exact cases passed in 17936 ms. Offline fixture model cost: zero; live-model effectiveness is not measured.
```

## Candidate

Build command:

```text
cargo build --release -p agent-vesper-tui --features voice-flm,voice-kokoro
```

Identity:

```text
path: target/voice-candidates/agent-vesper-tui-r6-binding-repair
sha256: 1b82cc4167ff9333d9de961e078e79a38832b158efa16dbd88c452b257670225
size: 22685896 bytes
mode: 755
```

The candidate was copied before documentation-only edits and was re-hashed afterward. No installer was run and Alex's installed application was not replaced.

## Files changed by this repair

- `apps/agent-vesper-tui/src/voice_conversation.rs` — single production F9 entry and genuine barge-in ownership.
- `apps/agent-vesper-tui/src/main.rs` — F9 delegates to that entry; explicit Stop status corrected while generic Ctrl+C remains.
- `apps/agent-vesper-tui/tests/voice_r6_binding.rs` — production-entry barge-in/Stop/recovery regressions.
- `apps/agent-vesper-tui/AGENTS.md` — durable R6 binding contract.
- `apps/agent-vesper-tui/tests/AGENTS.md` — R6 suite ownership.
- `docs/voice-oracle-extraction-prd.md` — current R6 implementation/device-acceptance status and this evidence link.
- `docs/foundation/AGENTS.md` — report ownership.
- `docs/foundation/evidence-index.md` — evidence ledger entry.
- `docs/foundation/voice-r6-binding-repair.md` — this execution report.

The pre-existing dirty VRO-17 tree and historical records were preserved. `voice-npu-user-acceptance.md` was not reused or edited.

## Deviations and unresolved items

1. The first `cargo xtask architecture` attempt resolved a stale compiled `vesper-testkit` manifest root under `/tmp/vesper-mcp-repair-check` and failed on a missing fixture. Rebuilding `vesper-testkit` from the current workspace corrected the executable provenance; the final architecture gate passed for 30 packages.
2. The first `cargo xtask acceptance` attempt similarly used a stale ACP test artifact and failed before its test body on `No such file or directory`. Rebuilding the ACP package from the current workspace produced the final 23/23 pass. These were local artifact-provenance failures, not product-test failures; both failed attempts are preserved here rather than hidden.
3. One combined command invocation was terminated by the execution environment after the combined-feature suite had completed and while the separate default suite was printing. The default suite was rerun independently to completion (251 + 156, zero failures).
4. No real microphone/speaker interruption matrix was run. Playback stop and synthesis cancellation are automated production-path evidence only until Alex performs device acceptance.
5. PR-5, exact-commit CI, MSRV, cross-target builds, supply-chain gates, release packaging, and public release remain outside this repair.

## Readiness effect

The §2.4 implementation blocker is closed in source and controlled acceptance:

- Speaking+F9 is bound to genuine, one-gesture `BargeIn`.
- playback stop, synthesis invalidation, transactional runtime cancellation, immediate capture, and one-time interruption context are covered by the production-entry regression;
- explicit Stop cancels voice output/runtime without capture;
- non-voice Ctrl+C and provider neutrality remain green.

At this repair unit's close, R6 was **implemented — device acceptance pending**.
The later device acceptance passed and now closes R6. VRO-17 is not advanced to
PR-5 or public-release readiness by either record.
