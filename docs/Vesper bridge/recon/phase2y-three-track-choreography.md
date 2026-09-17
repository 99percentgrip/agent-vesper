# Phase 4b — Three-Track Sequenced Choreography (Session 2y)

**Date:** 2026-09-16 · **Status: PASS — all three phases executed, timed, and verified; one real application defect found en route**

## The choreography (Alex's spec)

Open Elisa → track 1 for 30s → track 2 for 15s → track 3 for 30s → pause.

## Execution timeline (all position-polled against the player's own clock)

| Phase | Action | Measured | Transition | Identity verified |
|---|---|---|---|---|
| Launch | Open Elisa | MPRIS-ready ~1s | — | restored `playlist/2` (app persistence) |
| Navigate | 2× Previous (spaced 1.5s) | — | → `playlist/0` | **"Euphoric Progression.wav"** ✓ |
| 1 | Play → poll to 30s | **30,122 ms** | Next | → `playlist/1` **"Euphoric Progression (1).wav"** ✓ |
| 2 | Play → poll to 15s | **15,274 ms** | Next | → `playlist/2` **"Invisible World (High Energy Mix)"** ✓ |
| 3 | Play → poll to 30s | **30,192 ms** | **Pause** | stayed `playlist/2` ✓ |
| Terminal | Status + frozen ×2 | — | — | `Paused`, position **30,192,000µs twice, 2.5s apart** ✓ |

Timing precision: each phase landed within **122–274 ms** of target (poll
interval + DBus round-trip) — same bounded overshoot class as 2u/2y,
visible in the receipts.

## Defect found: Elisa crashes under rapid Next/Previous inversion

During the *previous* attempt (2026-09-16, session 2y-first-try), rapid
alternating `Next`/`Previous` calls with no spacing **killed the Elisa
process** (MPRIS service vanished; `pgrep` empty; relaunch restored
`playlist/2`). This run deliberately used:

- forward-only navigation (`Next` while playing) — no inversions;
- **spaced** `Previous` calls (1.5s apart, with an identity probe and a
  liveness check between each) when back-navigation was required.

Result: zero crashes. **Classification: application defect (Elisa's MPRIS
handler), not a Bridge defect** — but it is exactly the kind of
target-behavior knowledge the compatibility manifest exists for: the
adapter layer should rate-limit direction inversions until upstream fixes
it. Recorded here as the measured workaround.

## What this test proves beyond 2u/2y

- **Three distinct timed windows** in one session with different durations
  (30/15/30) — parameterized timing, not a repeated script.
- **Track continuity across transitions**: `Next` while playing flows
  directly into the next phase with no manual re-Play — the choreography
  rode the player's own autoplay.
- **Terminal state stability**: paused, frozen, correct track, 2.5s apart.
- **Recovery discipline**: an app crash mid-test was diagnosed, worked
  around with a safer pattern, and completed cleanly — not retried blindly.

## Receipts (verbatim transitions)

```
PHASE 1: T+30122ms → NEXT → playlist/1 "Euphoric Progression (1).wav" Playing
PHASE 2: T+15274ms → NEXT → playlist/2 "Invisible World (High Energy Mix)" Playing
PHASE 3: T+30192ms → PAUSE → Paused, position 30192000 frozen ×2
```

## Verdict

**PASS.** The full three-phase choreography executed with per-phase timing
accuracy of ±274 ms or better, verified identity at every transition, and a
stable terminal state — plus one honest application-defect finding with a
measured mitigation recorded for the adapter layer.
