# Dynamic microphone and long dictation execution

Status: implemented; automated Linux acceptance passed. Real-device/macOS acceptance remains open. Baseline: `8c1d1bc`.

## Objective

Implement [the voice-control PRD](../voice-control-prd.md), then publish an update
without replacing Alex's installed Vesper or touching his skill library/GLM work.

## Methods and files

`ui.rs` projects the voice phase into red circle/square footer controls and elapsed
status, sharing mouse geometry. `main.rs` wires the worker, drains completed text
without auto-submit, and lets F5/Discard bypass modal key interception.
`voice.rs` owns cancellable setup, private audio, live recorder checks, warm sidecar,
chunk ordering/retry and cleanup. `voice_transcribe.py` consumes bounded 30-second
PCM slices. Removed the obsolete synchronous sidecar/bootstrap implementation and
its tests; the current worker's protocol and native fixture tests replace them.

## Evidence

The focused checks below and `cargo xtask verify` passed locally. No real microphone
or user audio is used in foundation tests. Recorder/inference fixtures exercise
production host code and PCM slicing but are not speech-recognition accuracy or
macOS microphone-permission evidence.

## Scope and limitations

Recording continues until Stop or an actual backend/disk failure. WAV/backend and
disk capacity are finite; “no short timer” is not unlimited-storage support.
Chunks retain order and completed work for retry. Fixed nonoverlapping slices can
split a spoken phrase; this release does not promise perfect speech recognition.
No progress for five minutes bounds a stalled chunk/model load, independent of
overall dictation duration. Audio remains private and session-local until success,
discard or normal exit; abrupt OS/process death may leave temporary artifacts.
Windows microphone capture is still unsupported. Native macOS capture/permissions
and real-device long dictation remain separate manual acceptance.

## DOX and readiness

TUI ownership, test ownership and foundation evidence docs updated. ACP remains
unchanged because microphone capture is the documented terminal-only exception.
No provider/policy/library behavior changes. Publication is tracked in [v0.22.6 release execution](v0.22.6-release-execution.md).

## Focused verification receipts

- `cargo test -p agent-vesper-tui --all-features`: passed (241 library + 152
  binary tests before the added overlay regression; one unrelated ignored test).
- `microphone_footer_states_are_visible_red_and_clickable`: passed across all six
  themes, 40/80/120 columns, idle/running states, focus mode and all voice phases.
- `voice_stop_survives_permission_and_command_overlays`: passed.
- `voice_chunks.py`: passed with real numpy; ten-minute PCM and exact 20-slice
  ordering, partial resume and complete resume.
- `voice_pty.py`: passed through the production binary, using isolated fake
  recorder/inference helpers. Ten-minute PCM, >90 seconds of real wall-clock
  progressing transcription, warm model reuse, mouse/F5, composer editing,
  command menu/focus mode, cancellation, retry/discard, early recorder exit,
  simulated disk failure and normal-exit audio cleanup. No user mic/provider call.
- Logs: `/tmp/voice-all.log`, `/tmp/voice-frame.log`, `/tmp/voice-overlay.log`,
  `/tmp/voice-chunks.log`, `/tmp/voice-pty-final.log`.

The first terminal fixture assertion looked for newly typed text beyond its
one-line visible composer width. Clearing the fixture's previous long dictation
made the responsiveness assertion observable; no production behavior or acceptance
was weakened. The timed long-transcription case passed on both subsequent runs.

Primary API check: [faster-whisper transcription source](https://github.com/SYSTRAN/faster-whisper/blob/master/faster_whisper/transcribe.py)
accepts numpy audio input; the worker supplies 16 kHz float PCM. This source check
is not a substitute for real-model/device acceptance.

## Requirement coverage

| PRD requirement | Current evidence | Boundary |
| --- | --- | --- |
| 1–2: visible circle/start, live square/stop | Renderer and real PTY mouse/F5 lifecycle | Controlled recorder, no physical microphone |
| 3: responsive phases, duplicate input, cancellation | Worker phase transitions, PTY typing during >90-second transcription, cancellation/cleanup tests | First-use downloads depend on network |
| 4: failures clear capture, actionable recovery | Early-exit/disk-failure PTY, invalid protocol/stall/cancel unit tests | Native OS permission denial unexecuted |
| 5: preserve composer, no auto-submit/privacy | PTY composer assertion; private TempDir, success/discard/exit cleanup | Abrupt process death can leave temp audio |
| 6–7: widths, focus/overlays, glyphs/colors/hit targets | Six-theme 40/80/120-column frames, focus/overlay tests and PTY | Terminal-native glyphs, not pixel GUI buttons |
| 8: backend and host boundaries | Linux fixture uses real process path; five-target release gates required | macOS real recorder unexecuted; Windows capture unsupported; ACP excluded |
| 9: long capture and elapsed time | No recorder duration flag; ten-minute PCM; elapsed PTY and disk-failure checks | Physical ten-minute recording unexecuted |
| 10: chunking/progress/retry/retention | Production Python slicing: 20 ordered chunks; resumptions; >100-second PTY; stall/cancel tests | Controlled inference proves lifecycle, not speech accuracy |

`cargo xtask verify` completed successfully, including the 23-case acceptance
regression gate (`/tmp/voice-verify.log`). The version-only bump passed locked
all-feature checks for both hosts (`/tmp/voice-version-check.log`). Canonical CI
now repeats the microphone-free PCM and timed PTY checks on the release commit.
No completion gate, skill source, provider adapter or policy was changed. Their
DOX contracts remain unchanged; no child index changed. CI/TUI/tests/foundation
contracts were updated for the new implementation and verification ownership.

Final local PTY rerun passed (`/tmp/voice-pty-release.log`), including the explicit
composer-only preservation assertion. Library (488 files), protected GLM files and
original checkout HEAD/dirty files match their recorded baselines.

## Release-commit verification

[Canonical CI](https://github.com/99percentgrip/agent-vesper/actions/runs/34752624278)
repeated the workspace, microphone renderer/worker and timed real-terminal checks
on `81884c68728e392e26415a11623aabd12597412b`. The microphone step passed ten-minute
PCM ordering/resume and >90-second progressing transcription through mouse/F5,
composer preservation/editing, retry/discard, early exit, disk failure and shutdown.
All five platform gates and MSRV passed; see the release ledger. The manual
real-device/macOS acceptance boundary above is unchanged.
