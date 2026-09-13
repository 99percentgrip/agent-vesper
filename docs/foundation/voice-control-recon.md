# Push-to-talk footer reconnaissance

Status: recon complete; proposed repair not implemented. Date: 2026-09-13.
Source baseline: `e425da0`, released source tag `v0.22.5` at `39d6b9f`.

## Objective

Explain the missing bottom-panel voice control and specify Alex's requested
red circle “Push to talk” → red square “Stop” toggle.
Owning requirements: [PRD](../voice-control-prd.md).

## Methods and inspected sources

Read root/documentation contracts; inspect `ui.rs` footer inventory, candidates,
layout, renderer and tests; trace `main.rs` F5 action, recorder and sidecar.
Commands: `rg -n 'voice|talk|record'`, `git log -S 'fn footer_candidates'`,
`git show e924596 -- apps/agent-vesper-tui/src/ui.rs`, targeted `sed` reads.
No microphone access, dependency installation, provider calls or program tests.

## Findings

1. Regression introduced by `e924596` (2026-09-03, “render structured edits and
   live run chrome”). `ui.rs` formerly projected all `FOOTER_ACTIONS` into wrapped
   rows. That commit changed it to a one-row state-aware `footer_candidates` list.
   The new list omits `toggle_voice` in idle and agent-running states. This is
   not merely width clipping: it cannot appear even with ample horizontal space.
2. `ui.rs:277–292` still lists “F5 Push to talk” in `FOOTER_ACTIONS`, but that
   inventory is no longer the renderer's source. `footer_candidates` at 310–384
   controls visibility; `footer_action_rows` at 391 controls layout. Mouse targets
   use the same projection, so the omitted item has no clickable footer target.
3. `main.rs:4317` retains the F5 binding; `main.rs:5055` dispatches `toggle_voice`.
   `toggle_voice_recording` at 4769 still starts/stops the recorder, transcribes,
   and appends to the composer. The backend was not deleted by the UI change.
4. `ViewModel` has no voice state. The recorder lives only in `TuiSession`.
   Footer colors currently distinguish only ordinary/warning chips, not microphone
   recording. A dynamic control requires explicit state projection and styling.
5. The same regression commit replaced assertions that all footer actions appear
   with contextual tests. The present test at `ui.rs:2809` validates visible
   targets and Send/Cancel/Help, but never asserts that voice is present.

## Adjacent implementation risks

- Setup and transcription run synchronously in the action handler; sidecar
  transcription uses blocking `recv_timeout`. A Preparing/Transcribing label alone
  will not make progress visible while the event loop is blocked. Move that work
  off the render/input loop as part of the requested dynamic flow.
- Recorder success is currently inferred from successful spawn. There is no
  periodic recorder-liveness projection; an early recorder exit can leave an
  apparent recording until the next toggle. New Stop state must track liveness.
- Linux `arecord`, macOS `afrecord` and local faster-whisper dependencies remain;
  container setup from v0.22.5 does not provide microphone dependencies. Current
  code explicitly declines recording on other platforms. No new platform support
  is claimed by this recon.
- Existing `VoiceRecording::drop` kills/waits and removes the WAV. Preserve that
  cleanup ownership while introducing background work and cancellation.

## Recording duration follow-up

Alex reported a remembered short-recording limitation. Current `main.rs:4845–4859`
passes no duration argument to arecord/afrecord, and the host has no short capture
timer in `toggle_voice_recording`. This does not establish unlimited backend or
filesystem capacity, nor reproduce the historical symptom.

There is a fixed **90-second transcription response timeout** in
`transcribe_via_sidecar` (`main.rs:4743–4766`), applied after Stop. The sidecar
consumes every segment before returning a single result (`main.rs:1060–1064`).
Long audio or a slow CPU can therefore time out without exposing partial progress.
On error the sidecar is dropped; the caller then removes the WAV before examining
the outcome (`main.rs:4794–4795`). This can lose retryable audio. It is a concrete
current defect, but not proof that it caused Alex's remembered recording cutoff.

The PRD now requires user-controlled long capture, elapsed time, progress-aware
chunked transcription, genuine stall bounds, and private session-local Retry/Discard
recovery. These are requirements, not implemented behavior. Follow-up verification
was source inspection and Markdown/whitespace checks only.

## Proposed repair

Restore a high-priority persistent voice control; project Idle/Preparing/Recording/
Transcribing/Error from host state into the renderer; use red circle/square symbols
with state labels; retain F5 and share the rendered mouse geometry. Reserve active
Stop before low-priority chips. Do not mix microphone Stop with agent cancellation.
The PRD defines lifecycle, responsiveness, narrow-width and acceptance requirements.

## Files, verification and readiness

Created the owning PRD and this report; updated documentation ownership, evidence
index and root user preference. Source/runtime files are intentionally unchanged:
this work unit is reconnaissance. Read-back/link/whitespace checks only; no full
suite for prose. No release/version bump, local installation change or recording.
Implementation and actual microphone/OS acceptance remain open. The root cause is
established from source and history; this does not claim a new runtime fix.
