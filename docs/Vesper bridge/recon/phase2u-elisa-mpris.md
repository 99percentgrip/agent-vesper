# Phase 4a — Second Application: Elisa Music Player (Session 2u)

**Date:** 2026-09-16 · **Status: PASS — played first song via MPRIS, verified at three independent layers**

## Objective

The PRD's second-application test: an unrelated app through the same Bridge
core, demonstrating that per-app connection is a **profile**, not a project.
Target: open Elisa, play the first song from the list.

## Execution log (every step, no human in the loop)

| Step | Command (typed, bounded) | Result |
|---|---|---|
| Recon | `which elisa` | `/usr/bin/elisa` installed |
| Recon | `busctl --user list \| grep mpris` | none running (app not started) |
| Launch | `nohup elisa` (own process) | pid 50012 |
| Connect | `busctl` → `org.mpris.MediaPlayer2.elisa` | **on the bus ~6s after launch** |
| Capabilities | `CanPlay/CanPause/CanGoNext/CanControl` | **all `true`** |
| State read | `PlaybackStatus` | `Stopped`, track `Euphoric Progression.wav` already queued as playlist/0 |
| **ACT** | `busctl --user call … Player Play` | accepted |
| **VERIFY 1** | `PlaybackStatus` (1.5s later) | **`Playing`** |
| **VERIFY 2** | `Position` ×2, 2s apart | **9,386,000µs → 11,434,000µs — advancing** |
| **VERIFY 3** | `pw-top` (PipeWire graph) | **`elisa` stream: `S16LE 2 48000`, ACTIVE in the audio graph** |

Track: `Euphoric Progression.wav` (first in Elisa's list), `mpris:trackid
/org/kde/elisa/playlist/0`, length 345.96s.

## Verification standard held

Same as Resolve: a command acknowledgment is not a result. Proven at three
independent layers — **player state** (`Playing`), **time-base** (position
advancing at wall-clock rate), and **audio graph** (PipeWire shows Elisa's
stream actively consuming). Not one but three truths.

## Route verdict — the ladder claim, now measured

| | Resolve free (rung 4: no API, archaeology) | Elisa (rung 1: standard API) |
|---|---|---|
| Discovery time | ~4 hours | **~40 seconds** |
| Code written | ~700-line Lua worker + 12 diagnostics | **zero** (DBus calls only) |
| Launch paste | 1 per session | **0** |
| Verification | file + ffprobe + sha256 | **typed property reads** |

One finding sharpens the story: Elisa also **imports your Downloads folder** —
it auto-discovered your music (`love will never comeback again.mp3`,
`Invisible world`, `Euphoric Progression`). No library setup existed in
`~/Music`; Elisa's first-list entry came from its own scan.

## Generalization

The adapter surface for **every** MPRIS player (VLC, Amarok, Spotify,
browsers playing audio) is one profile: `play/pause/next/previous/status +
CanX property gates`. The Bridge core's session/lease/verify model applies
unchanged. This is the "app #2 is a profile file, not a project" claim, now
with a receipt.

## Honest limitations

- `Next`/`Previous`/`Seek` exist but weren't exercised this session.
- Playlist **enumeration/reordering** is not in MPRIS (Elisa-side list
  management would need a different route).
- Play targeted the **already-queued first track** — selecting an arbitrary
  song by name would use `SetTrack`-class MPRIS ops or the playlist interface;
  not tested.
- MPRIS control is user-session-wide: any app on the bus can talk to it. That
  is the standard's model, not a Bridge weakness — but Bridge's authority
  gates still apply on our side.

## Artifacts

- Session commands are all `busctl`/`pw-top` invocations (in this report)
- Elisa left running with playback active for Alex's inspection

## Sequencing test (same session, later run): second track, 30s, pause

Objective from Alex: open Elisa, play the **second** track, and pause it
after exactly 30 seconds.

| Step | Action | Measured result |
|---|---|---|
| State check | busctl | Elisa had been closed (only `org.kde.elisa` activatable) — relaunched, MPRIS up in 7s |
| Baseline | Metadata | `playlist/0`, "Euphoric Progression.wav", Stopped |
| Advance | `Next` + `Play` | accepted |
| **Verify track 2** | Metadata | **`playlist/1`, "Euphoric Progression (1).wav", Playing** — trackid and title both changed |
| 30s window | `Position` polled every 0.5s | advancing: 19.1s → 20.1s → 25.2s → **30.293s** |
| **Pause** | `Pause` at ≥30s | fired at 30,293ms |
| **Verify 1** | PlaybackStatus | **`Paused`** |
| **Verify 2** | Position ×2, 2s apart | **30,293,000µs both times — frozen** |
| **Verify 3** | PipeWire graph | elisa stream present (`S16LE 2 48000`) but **idle** — ERR 1906 static, no active flow |

Note on Verify 3: PipeWire keeps the paused stream node alive (Corked: no,
but zero processing load) — the honest audio proof here is the frozen
position clock plus the player state, not stream disappearance. Recorded
as measured, not assumed.

Timing precision: pause landed 293ms past the 30s target (poll interval +
DBus round-trip) — bounded overshoot, visible in the receipt.

## Verdict (sequencing test)

**PASS.** Multi-step, time-bounded control executed: track selection,
playback, a 30-second window enforced against the player's own position
clock, and a verified pause. This is the PRD's "operate an unrelated
desktop application" flow in miniature — three typed commands and one
measured wait, no clicks, no screenshots, no per-app code.



**PASS.** Second application connected and controlled in under a minute with
zero per-app code — the exact contrast to Resolve that validates the ladder
architecture.
