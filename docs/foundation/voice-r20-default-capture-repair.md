# VRO-17 R20 — Default-Build Capture Bounds Repair

Work unit: 2026-09-23 (fourth session). Baseline `main` @ `8f258ba` + the
uncommitted VRO-17 voice tree (all prior candidates preserved; unrelated
dirty work untouched). Authorizing directive: close **R20 only** — the
default-build/feature-off F5 dictation capture-bounds defect the final
completion audit identified.

## Before / after call paths

**Before (defect, source-verified):** `Worker::start()` created captures via
`Audio::new()` — a plain `tempfile::TempDir` with `recording.wav`, **no
duration/byte cap, no aggregate accounting, no free-space gate, no crash
recovery**. A feature-gated block ran `ManagedCapture::start()` in
`voice-conversation` builds, but the resulting object was **never used**:
the recorder still wrote the tempdir (`self.managed` was dead state — the
audit's "feature builds use the store" reading was itself half-wrong: both
paths were unbounded; the feature path merely *believed* it had a store).

**After (one store, one ownership policy):**

- Default + feature, F5 + F9: `ManagedCapture::start_passthrough(root)` →
  recorder streams (`arecord … -t wav -`) through a pump thread into the
  store's capped writer (byte cap at the writer boundary; 120 s time cap;
  32 MiB cross-instance aggregate; 1 GiB free-space reserve; lease-backed
  dead-lease recovery; foreign/symlink preservation).
- Stop: pump joins, capture finalized and **retained** for transcription
  (`Audio::for_managed`); Drop cleans the retained dir on every terminal
  outcome (success, no-speech, error, cancel/discard).
- Cap-stop: an explicit `cap_hit` flag distinguishes a hard-cap recorder
  exit (a **normal** end-of-capture; audio retained, no auto-submit) from
  an abnormal recorder death (error surfaced, capture still salvaged for
  F5 Retry / Del Discard).
- macOS: `afrecord` writes the store's capture path directly (file mode);
  same caps/cleanup semantics.
- F5 semantics unchanged: editable dictation, never auto-submit,
  Stop/Retry/Discard preserved, VAD preserved, STT route selection
  preserved; F9/conversation path shares the ownership policy only.

## Exact defect (red receipt, `/tmp/r20-red-receipt/red.txt`)

`recorder_seam_remains_arecord_wav_to_a_path` FAILED pre-fix:
"the recorded default-build gap text must be gone after repair" — the
production source literally documented "Default builds keep the
pre-existing private tempdir". 10/11 store tests passed (the store itself
was complete); the failing test pinned the **call path**, which is where
the defect lived.

## Red → green

- `voice_r20_default_capture` (11 tests, **runs in every feature set**):
  managed-namespace destination; cap constants; byte-cap never exceeded;
  cap-stop has no auto-submit; **cleanup-after-every-outcome leaves zero
  residual** (success/discard/transcribed); aggregate refusal with a live
  first capture; unmeasurable-destination deferral; foreign+symlink
  preservation; dead-lease reclaim / live-lease survival; failed-recorder
  leaves no owned directory; production seam shape. RED→GREEN ×6 runs
  stable (suite serialized on `STORE_LOCK` — the free-space probe and
  recovery scans share the host tmpfs).
- Production PTY (real F5 path, default-build semantics): `voice_pty.py`
  **PASS** on the repaired candidate — mouse/F5 lifecycle, editable
  composer, retry/discard, early-exit and disk-failure error paths,
  shutdown cleanup; capture-existence and cleanup assertions migrated to
  the managed namespace (`data/agent-vesper/captures/cap-*/capture.wav`),
  residual `cap-*` = none after every outcome.
- F9 zero-regression: `r3_loop_pty.py` **PASS** (one agent turn, provider
  wire, hygiene, Kokoro PCM); `flm_f9_loop_pty.py` **PASS** (Settings save
  → Verify → F9 → FLM NPU adapter → one turn, PCM [92800], CPU recognizer
  untouched). F9 remains on the same owner — now genuinely bounded.

## Fixtures modernized to the streaming seam (recorded, not weakened)

- `voice_recorder_fixture.py`: `-` now means stdout (real `arecord`
  semantics; header once + frames, no re-seek), real-time-ish bounded pace,
  failure probes exit before any cap so abnormal exits stay observable.
- `voice_model_fixture.py`: the 30 s-only length assert relaxed to
  any-nonempty (captures are ≤120 s by design now).
- `voice_pty.py`: long-transcription check re-scoped from ">90 s of
  transcription on a 600 s capture" (now impossible **by design** under
  the 120 s cap) to ">9 s multi-chunk progressing transcription within the
  capped capture"; capture location assertions moved to the managed
  namespace.
- `flm_f9_loop_pty.py` recorder: streams on stdout (same fixture payload).
- `voice_speech_pipeline.rs` registered with its real required-features
  (fixes pre-existing `--no-default-features --tests` compile breakage,
  verified pre-existing on the stashed clean tree); `voice_execution_policy`
  required-features corrected to include `voice-flm` (it references FLM
  verification state).
- `voice_accel::reset_flm_stt_verification_for_test` cfg-gated to
  `voice-flm` (voice-conversation-without-flm builds now compile).

## Storage/resource measurements (constants + test receipts)

Max duration **120 s**; max bytes **4 MiB** per capture; aggregate **32 MiB**
across instances; free-space reserve **1 GiB** (unknown space defers).
Residual owned bytes after every tested terminal outcome: **0** (test
asserts; PTY asserts no `cap-*` remains). Peak owned usage in tests:
bounded by the aggregate test's near-cap filler (≈32 MiB) and rejected
beyond. **No new runtime files/directories** beyond the existing managed
namespace; no persistent audio archive (only a live owned capture while in
progress). No caches cleaned; the partial Llama residue untouched.

## Changed production files

- `apps/agent-vesper-tui/src/voice_capture_store.rs` — `start_passthrough`
  constructor (no header injection for recorder-owned WAV containers;
  finalize skips patching), `finish()` (public finalize+close).
- `apps/agent-vesper-tui/src/voice.rs` — capture ownership through the
  store in both build flavors; pump thread + `PumpHandle`; cap-vs-failure
  recorder-exit classification; `Audio::for_managed` + Drop cleanup;
  salvage-on-abnormal-exit.
- `apps/agent-vesper-tui/src/lib.rs` — `voice_capture_store` and
  `voice_capture_root` ungated (the shared capture-safety primitive; pure
  std; no conversation behavior enters the default path).

## Gates (commands + results)

- `cargo test -p agent-vesper-tui --test voice_r20_default_capture` —
  11/11 ×6.
- `cargo test -p agent-vesper-tui {--lib,--bin}` default — 251 + 155 ok.
- `cargo test -p agent-vesper-tui --features voice-flm,voice-kokoro
  --tests` — 15 suites ok, 0 failed (incl. r3_worker, interruption,
  multiturn, policy, parity, flm_route, r20).
- `cargo test -p agent-vesper-tui --no-default-features --tests` — 8 suites
  ok (pre-existing example compile breakage unchanged; test-target
  registration fixed).
- PTY on candidate bytes: `voice_pty` PASS, `r3_loop` PASS,
  `flm_f9_loop` PASS.
- Clippy `-D warnings`: lib, bin (default + voice features), tests — clean.
- `cargo fmt -p agent-vesper-tui` applied.
- `cargo xtask architecture` 30 pkgs; naming-guard clean; acceptance 23/23.
- R20 status change → acceptance gate rerun per directive: 23/23 (above).

## Candidate

`target/voice-candidates/agent-vesper-tui-r20-default-capture` — SHA-256
`70cfb12209b01c8e88ec4f34…` (byte-identical to `target/release`, features
`voice-flm,voice-kokoro`), source = `8f258ba` + dirty tree (this file
records the association; no claim of source-equality beyond it). Prior
candidates untouched; no installed binary replaced. No microphone/speaker/
NPU/provider live test required or run for this storage-safety unit
(audible behavior unchanged; `voice_pty` used the fixture recorder).

## R20 verdict

**PASS.** The default-build F5 dictation path now obeys the full R20
contract through the same managed store as the conversation path. Both
prior R20 sub-verdicts (feature IMPLEMENTED / default OPEN) collapse to
one PASS with no scoped limitation.

## Remaining VRO-17 OPEN items (post-R20, re-derived from the amended PRD)

- **R4** — production partial-transcript wiring (or scope decision).
- **R6** — Alex-operated interruption/recovery device acceptance.
- **PR-5** — ACP voice decision/exclusion doc + parity test; commit the
  voice tree; exact-commit canonical/MSRV/five-target/web-driver/release;
  user docs; tag.

(R2/R3/R16b are settled by the approved amendments — cloud optional, NPU
TTS capability-gated, CPU conformant.)
