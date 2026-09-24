# Native terminal process verification

## Purpose

Verify terminal interaction through the production TUI binary with isolated state.

## Ownership

- `voice_speech_pipeline.rs` isolates PATH/HOME in a test subprocess and uses
  synthesis-only WAV/player fixtures without devices. It requires the second
  synthesis to overlap first-player drain, asserts the bounded two-unit
  synthesis bank (banked audio never starts a player; stale generations never
  speak) via the boundary-gap regression `unit_boundary_gap_…` (red at a
  1.019 s inter-unit gap on the pre-fix tree), checks Stop
  and stale admission, verifies cumulative progress across the latency-sized
  multi-piece unit, and verifies new-generation FIFO playback after Stop.

- `voice_execution_policy.rs` proves the clarified-R16 no-NPU matrix through the
  production selection seams (`voice_accel`, `vesper_voice::execution`, real
  save/read, an instrumented registry fixture): zero-accelerator-call CPU
  policy, ordinary-CPU Automatic on absent/unsupported hardware, stage-specific
  strict refusals naming the exact blocker, independent STT/TTS resolution,
  local revalidation of copied strict preferences (no silent rewrite), readiness
  cache identity/invalidation, honest placement attribution, and Settings row
  honesty — extended 2026-09-23 with the presentation-separation contract
  (CPU-selected vs available-not-selected vs unavailable-refusal vs last-request
  route receipt, draft-vs-saved with no scope rewrite; tests that require a
  specific FLM verification state hold `FLM_VERIFICATION_LOCK` for the whole
  test and set it explicitly via `reset_flm_stt_verification_for_test` or
  `record_flm_stt_verification`; they never infer a prerequisite from the
  developer machine. No devices, no downloads, no NPU runtimes.

- `voice_r20_default_capture.rs` (2026-09-23 R20 closure) pins the
  default-build capture contract in EVERY feature set (no
  `required-features` — that is the point): managed-namespace destination,
  cap constants, byte-cap enforcement, no-auto-submit on cap, zero residual
  after every terminal outcome, aggregate refusal, deferral on
  unmeasurable space, foreign/symlink preservation, dead-lease reclaim
  with live-lease survival, and the production recorder seam shape
  (`arecord ... -t wav -` streaming). The suite serializes on
  `STORE_LOCK`: the free-space probe (`df`) and recovery scans share the
  host tmpfs, and parallel runs flaked before serialization.

- `voice_playback_diagnostics.rs` uses fake player processes to verify safe
  device-error classification and stderr-pressure drainage without audio devices.

- `voice_flm_route.rs` also pins the transport failure-classification contract
  (2026-09-23 Verify-read repair): a reset from a dying server reports
  `owned ASR process exited while answering`, never the timeout phrasing; a
  silent server reports `owned ASR read timed out`; a clean EOF is never a
  timeout. The read-deadline seam
  (`voice_flm::with_request_read_deadline_for_test`) is process-global: every
  test that reads through `transcribe_request` holds `FLM_STATE_LOCK` while
  doing so, or the short override shrinks a parallel test's deadline. Readiness
  tests assert the machine-independent evidence rule (reset is never Ready;
  recording verification promotes to Ready), without requiring physical FLM
  prerequisites on CI runners.

- `voice_pty.py` requires the **voice-venv python** as its second argument
  (`python3 tests/voice_pty.py <binary> ~/.local/share/agent-vesper/
  voice-venv/bin/python`); any other second argument is used as the fixture
  interpreter and fails at the recorder shebang. `settings_pty.py`'s
  Settings-submenu click currently drifts on every preserved candidate
  (root menu stays visible after clicking `Implementation acceptance`);
  tracked as pre-existing, not a per-unit regression.

- `r3_loop_pty.py` exercises F9 through the actual agent/provider dispatch using
  a loopback LM Studio protocol fixture, recorder/STT/player doubles, and real
  installed Kokoro inference. It asserts the voice instruction on the chat wire,
  text retention, positive PCM and no speech failures. eSpeak is allowed only as
  a quiet IPA phonemizer, never as fallback audio. Existing verified asset inodes
  are reused in an isolated cache; no downloads, devices or public providers.
  Run with the voice-capable binary; optional arguments are fixture Python,
  `am_michael|af_heart`, and reasoning mode (`off`, `balanced`, or `preview`).
  Balanced uses a math prompt and asserts VRO was exercised; `preview` drives
  Settings → Voice → Natural Voice pack → Preview through the real screen
  (policy-gated row) and asserts real Kokoro PCM with no recorder/provider.
  No production reply-injection environment switch is supported.

- `voice_policy_parity.rs` proves Preview and F9 share one execution-policy rule
  through the real seams (`voice_accel::preview_policy_gate`, the gate
  reconstruction, `ConversationHost::speech_state`): strict-NPU scope refuses
  identically on both paths; Automatic with the empty registry is an ordinary
  CPU outcome; a no-op engine reload keeps the generation/engine; a real voice
  change still replaces at the unit boundary. Chdir roots hold a lock for
  their lifetime; the suite is parallel-stable (the historical
  "run single-threaded" note is obsolete).

- `voice_provider_neutrality.rs` pins both reasoning-provider neutrality and
  the R4 presentation contract. Final-only recognizers show the **live
  preview** as unsupported while stating that final text still appears after Stop;
  capability gating never destroys the saved preference.

- `voice_interruption_lifecycle.rs` drives the real ConversationHost +
  SpeechWorker through the §4 matrix rows: five interruption cycles then
  full-turn recovery, no transcript replay, failure-before-audio
  classification/recovery, session-exit clean-lane teardown, and the
  one-Preview-worker-per-pack-screen lifetime.

- `voice_r6_binding.rs` owns the §2.4 production binding regression. It calls
  the same `ConversationHost::apply_conversation_gesture` entry as the F9
  handler and proves one Speaking gesture stops playback/synthesis, invalidates
  stale audio, records one transactional runtime cancel, opens capture, and
  stages interruption context once. It separately proves explicit Stop opens no
  capture, repeated cycles recover, and the voice host remains untouched by a
  non-voice Ctrl+C. Red proof must mutate the production entry to Stop and fail
  on missing same-gesture capture, not merely retest the reducer.

- `voice_multiturn_playback.rs` proves sustained multi-turn playback through
  the real worker with realistic-rate controlled players (canonical PCM
  consumption, delayed first read, mid-stream abort, early success, short
  consumer): ten consecutive single-process turns, turn-3 player death
  failing once with same-worker recovery, inter-piece gaps never ending a
  stream, original-error (errno) preservation, no `--fatal-errors` in the
  production argv, appended-stream byte accounting, and the EPIPE-mechanism
  pin (write-after-reap fails; write-after-unreaped-signal still succeeds).
  Never opens a real audio device.

- `voice_chunks.py` verifies ten-minute production PCM slicing and resume order
  with real numpy and controlled inference.
- `voice_vad_worker.py` verifies the persistent backend-neutral Silero worker
  protocol with a controlled VAD module: distinct silence, canonical atomic
  output, warm multi-request reuse, shutdown, and redacted failures. It requires
  a Python containing numpy (normally the existing voice venv) and uses no device.
  The worker itself was additionally probed against the real installed Silero
  model for both digital silence (distinct no-speech, no output file) and a
  synthetic speech surrogate (speech-positive filtering with exact retained-
  window fidelity); see `foundation/voice-npu-stt-implementation-progress.md`.
  Installed faster-whisper 1.2.1's `collect_chunks` returns
  `(audio_tuple, segments)` — the worker concatenates the per-segment arrays
  before quantization (a defect fixed 2026-09-22; keep that shape handling).
- `voice_pty.py` exercises the real voice footer/worker with microphone-free
  `voice_recorder_fixture.py` and `voice_model_fixture.py`. An isolated Python
  with numpy is passed explicitly; no microphone/provider/package setup is used.
  It verifies the managed 120-second/4-MiB capture bound, >9-second progressing
  multi-chunk inference, warm reuse,
  retry/discard, recorder/disk failures, editable input and normal-exit cleanup.

- `dependency_setup_pty.py` checks real setup consent/decline/failure/retry with
  an explicit missing engine override, structurally preventing package installation.

- `settings_pty.py` owns the stdlib-only Linux/macOS Settings lifecycle smoke test.
- `skill_routing_pty.py` reuses its isolated terminal driver for Skills draft,
  discard, keep-editing, save, restart, model-assistance opt-in and unchanged-library checks.
- `update_*_fixture.sh` are immutable offline download/version fixtures for the
  Rust updater test, which runs the shipped installer only in temporary roots.
- Rust rendering and configuration unit tests remain beside their source modules.

## Local Contracts

- Use temporary HOME, workspace and global data roots plus synthetic credentials.
- Block outbound proxies, submit no provider prompt, and never install a public release or write to real user state.
- Exercise production keyboard/mouse handlers and inspect saved files and restart
  behavior. Kill and observe child processes before removing fixture directories.

## Work Guidance

- Keep terminal fixtures bounded and dependency-free; failures print the last screen.

## Verification

- `r3_loop_pty.py` delays fixture Python import by two seconds, allows background
  integrity preflight, then requires recorder-process onset within one second.
  It records first-PCM timing separately; 250 ms key polling and synthetic
  transcription/provider responses must be disclosed with timing receipts. A
  deliberate one-second provider hold verifies that the upstream voice wait label
  is visible in the actual running frame, in both direct and VRO paths.

- `r3_loop_pty.py` also asserts fixed-reply PCM peaks at the fake player;
  nonempty but near-zero audio is a failure, not speech acceptance.

- Run `cargo test -p agent-vesper-tui --bin agent-vesper-tui updater_installs_verified_fixture`
  for exact-version download, checksum refusal, payload replacement and state preservation.
- Build `cargo build -p agent-vesper-tui --all-features`.
- Run `python3 apps/agent-vesper-tui/tests/settings_pty.py target/debug/agent-vesper-tui`.

## Child DOX Index

No children.
