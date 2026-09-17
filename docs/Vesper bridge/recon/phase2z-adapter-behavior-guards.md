# Phase 4c — Measured-Behavior Adapter Guards (Session 2z)

**Date:** 2026-09-16 · **Status: PASS — the adapter now knows how Elisa actually behaves, with every guard tested against a scripted fake and re-verified live**

## Objective

Alex asked "can we improve the behavior?" after spotting that the
choreography gap traced to two unhandled player behaviors. The improvement:
encode the **measured facts** into the adapter so it refuses or verifies
instead of blindly forwarding calls.

## The three guards (all from live measurements in 2y)

### 1. Previous-while-paused → refused with guidance
Measured: `Previous` while paused **restarts the current track** — an
unwanted mutation that silently destroys position. The adapter now reads
`PlaybackStatus` first and refuses:
> "previous while paused restarts the current track on this player
> (measured); resume playback before navigating back, or use a seek to 0
> explicitly"

### 2. Direction-inversion cooldown (1200 ms)
Measured: rapid `Next`/`Previous` **inversion crashes the player's MPRIS
handler** — process death, service vanished. The adapter tracks the last
successful navigation direction and refuses an opposite-direction call
inside the cooldown:
> "direction change within the 1200ms cooldown — rapid Next/Previous
> inversion crashes this player (measured); retry after the cooldown"

Same-direction calls are unaffected (forward-only sequences run at full
speed).

### 3. Navigation settles only on trackid change
§7 invariant applied to hops: `Next`/`Previous` read `mpris:trackid`
before and after; the call settles `Verified` only when the trackid
actually changed. A same-track result (the previous-restarts-track trap,
or an out-of-range hop) settles `Failed` with both trackids in the
summary — **a call acknowledgment never proves navigation**.

## Testability refactor that made this possible

`MprisBus` seam: production spawns `busctl`; tests inject a scripted fake
with **separate property/ack queues** so the script can never desync from
the adapter's call order (my first single-queue fake desynced by one call
— exactly the class of bug the split queues eliminate).

## Bugs found while building this

1. **My first fake bus desynced** — one queue for both property reads and
   call acks; `next` consumed a metadata string as its call ack. Fixed
   with type-keyed queues.
2. **Sloppy assert**: my hop-evidence assertion didn't account for the
   full trackid path prefix (`playlist/1 -> /org/kde/elisa/playlist/2`).
   The code was right; the test was wrong.
3. `let`-chains are not valid in `for` conditions (only `if`/`while`) —
   caught by 1.88 clippy, reverted to a legal nesting.

## Receipts (fresh)

| Gate | Result |
|---|---|
| guard tests (scripted fake) | **3/3** |
| harness `--features bridge --lib` | **152/0 in 1.20s** |
| harness feature-off | **122/0** |
| clippy stable **and** 1.88.0 | 0 errors |
| architecture / acceptance | 28 pkgs / 23/23 |
| **LIVE** `live_mpris_player_control` | **PASS** (with guards active) |

## Manifest

Compatibility manifest row added: MPRIS players — all three measured
behaviors with their guard responses, evidence pointers.

## Files

- `crates/vesper-harness/src/bridge_adapters.rs` — `MprisBus` seam,
  three guards, `current_trackid`, `settle_navigation`
- `crates/vesper-harness/src/bridge_adapter_guard_tests.rs` — 3 guard
  tests + scripted fake (split queues)
- `docs/Vesper bridge/recon/compatibility-manifest.md` — measured row

## Verdict

**PASS.** The adapter is no longer a dumb remote: it refuses the
track-restarting trap, paces direction changes under the crash-inducing
threshold, and refuses to call navigation done until the track actually
changed. Every guard traces to a measured fact, every fact is in the
manifest, and the original choreography (track 3 → track 2 via spaced
Previous-while-playing) is now the adapter's own enforced pattern.
