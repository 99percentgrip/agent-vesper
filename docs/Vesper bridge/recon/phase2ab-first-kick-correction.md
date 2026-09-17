# Phase 4e — Correction: First Kick at 51s (Session 2ab)

**Date:** 2026-09-16 · **Status: PASS (corrected) — Alex's ground truth confirmed by re-analysis; test re-run to the true first kick**

## The mistake (mine, and it was two-layered)

In 2aa I measured the kick band and my **first data row was the correct
answer**: `onset=51.560s (3.0x)`. I then discarded it with an invented
narrative — "a lone accent; the real kick is where energy doubles at
59s" — and stopped at 59.28s.

Alex listened to his own track and corrected me: **the first kick drum
hits at ~51s, and after that it's non-stop.**

## Re-analysis confirming his ground truth

Lowering the transient threshold from 3× to 2× baseline reveals what the
waveform actually says:

```
onsets: 51.54, 51.98, 52.40 | 55.46, 55.94 | 58.40, 58.84, 59.26, 59.68, 60.12, ...
gaps after 51.56s: 0.44, 0.42, [3.06], 0.48, [2.46], 0.44, 0.42, 0.42, 0.44, ...
```

- From **51.54s**, hits recur at ~0.43s — **the same tempo as the 59s
  "drop."** The beat starts at 51.5s.
- The 53–58s stretch is quieter kick hits (breakdown section), which my
  3× threshold filtered out entirely.
- The 59s energy doubling is the **arrangement getting louder**, not the
  beat beginning.

**Two failures compounding:** a threshold tuned too high for the quiet
section, and — worse — a confident narrative ("accent, not kick") built
on top of incomplete data instead of checking the human's stated ground
truth first. The listener was right; the theory was wrong.

## The corrected run

```
reopen Elisa → Next (guard-safe) → playlist/3 "Invisible World" verified
Stop → Seek 30,000,000µs (absolute from Stopped) → verified 30.000s
PLAY → polled → STOP at 51,888 ms   (kick = 51,540 ms; overshoot 348 ms)
Terminal: Stopped, position 0 → app closed → 0 MPRIS services
```

## Lesson recorded for the adapter layer

Event detection must not hard-code musical narratives. The honest
detector reports transient structure with its threshold **and** its
uncertainty; when a human states ground truth ("first kick ~51s"), the
correct move is to re-examine the data at the stated location — not to
defend the first interpretation. Recorded in the manifest alongside the
other measured-behavior rows.

## Receipts

```
corrected target:  51.540 s (onset #1 at 2x threshold, tempo-locked with the rest)
stopped at:        51.888 s (+348 ms poll overshoot)
terminal state:    Stopped, position 0, Elisa closed, MPRIS gone
```

## Verdict

**PASS (corrected).** The task now ends where the music says it should —
and the correction that got there was Alex's ears, not my analysis. That
division of labor is exactly right: humans supply ground truth; the
system must be built to accept it.
