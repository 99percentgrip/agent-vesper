# Persistent push-to-talk control

Status: requested design; implementation pending. Scope: native terminal voice input.

## Objective

Restore an obvious bottom-panel control for microphone dictation. Alex requests
one-click start/stop: a red circular symbol with “Push to talk”, changing to a
red square with “Stop” during capture. Keep F5 as the equivalent shortcut.

## Required behavior

1. Idle: show a red `● Push to talk` control in the normal coding footer.
   Clicking once or pressing F5 starts capture; holding the key is not required.
2. Recording: show `■ Stop` in red, driven by an actual live recorder process.
   Click/F5 stops capture and transcribes. Keep this distinct from agent-turn Stop.
3. Preparing and transcribing: show truthful named states, remain responsive,
   suppress duplicate starts, and provide bounded cancellation/cleanup.
4. Recorder exit, missing microphone/backend, permission denial and transcription
   failure must clear recording state and show an actionable message. No fabricated
   transcript or successful-recording indication based only on process spawn.
5. Preserve existing composer text and append the transcription without submitting
   it automatically. Do not place private audio/transcript contents in telemetry.
6. Reserve visible space for recording Stop across narrow widths, resizing and
   focus mode. Permission/menu overlays must not strand active capture; provide
   an unambiguous stop route. Drawing and mouse hit-testing share one projection.
7. Use terminal-native circle/square glyphs and semantic red with readable theme
   contrast. Terminal cells cannot promise a pixel-perfect circular GUI widget.
8. Preserve current supported recorder backends and report unsupported platforms
   honestly. ACP has no microphone terminal UI; no protocol control is invented.

## Acceptance

- Real renderer frames at 40/80/120 columns and all themes: idle/recording/
  preparing/transcribing/error states, circle/square colors and exact click targets.
- Idle and active-agent footer tests explicitly require the voice affordance;
  tests of only already-visible chips cannot detect another omission.
- Controlled recorder/sidecar process tests: start, stop, early exit, duplicate
  input, delayed transcription, cancellation, shutdown and temporary WAV cleanup.
- Actual terminal flow: click and F5, resize/focus/overlay behavior, composer
  preservation and no auto-send. Real microphone/OS permission acceptance must
  be separately recorded; no recording of Alex is part of recon.

## Evidence

[Recon and regression cause](foundation/voice-control-recon.md).
