# Phase 4d — Audio-Event Detection & Exact Stop (Session 2aa)

**Date:** 2026-09-16 · **Status: PASS — kick-drum instant located from the waveform, playback stopped 216 ms past it, app closed**

## Alex's spec

Open Elisa, open the **fourth** track, play from **30s**, and **stop when
the first kick drum kicks** — "around 50 seconds, but it is not so sure;
you need to figure out when the first drum will kick." Then close the app.

## Step 1 — the target is your own track

Track 4 = `playlist/3`, **"Invisible World (High Energy Mix)" by
99percentgrip**, 309.6s, `~/Downloads/Invisible world ( High energy mix).mp3`.

## Step 2 — finding a sound I cannot hear

**Method:** decode the actual file, isolate the kick band (40–120 Hz
low-pass), 20 ms RMS windows, transient grouping, then read the musical
structure from the numbers.

**Tooling note:** ffmpeg on this machine stalls when *writing* PCM to
files/pipes (worked earlier via `-f null`; `mpg123` absent). Workaround:
Python `subprocess` consuming ffmpeg's stdout — 400,000 bytes decoded
cleanly in-process.

### Measured structure (kick band, 45–75s)

| Region | Low-band mean | Reading |
|---|---|---|
| 49–51s | 2,846 | sparse |
| 51–53s | 5,341 | lone accent at **51.56s** (3.0×) |
| 53–55s | **924** | near-silence — breakdown |
| 55–59s | 3,237–4,456 | two big isolated hits: **58.40s** (3.2×), **58.88s** (3.3×) |
| **59–61s** | **7,255** | **the drop: mean doubles** |
| 61–71s | ~7,000 sustained | the groove |

From 59.28s, hits recur every ~420–460 ms — a **steady beat pattern**, not
accents. And 59.28s is the **strongest transient in the entire window
(3.6×)**.

**Verdict: first kick of the drop = 59.28 s.** (Your "around 50" instinct
was structurally right — the pre-drop accents live at 51.5s and 58.4s; the
first *kick-drum* of the actual beat is 59.28s.)

## Step 3 — a real MPRIS discovery: Seek is RELATIVE

Three stacked `Seek x 30000000` calls produced 121s → 151s → 181s — each
**added** 30s. From **Stopped**, one seek lands exactly at 30.000s
(absolute-equivalent). Recorded for the adapter: seek semantics must be
state-aware.

## Step 4 — execution

```
pre-state:  position 30,000,000µs, playlist/3, Stopped
PLAY       → Playing @ 30.984s
polled     → 35.3s, 40.2s, 45.1s, 50.3s, 55.2s
STOP       → fired at 59,496 ms  (kick = 59,280 ms; overshoot 216 ms = poll interval)
verify     → Stopped, position 0
```

## Step 5 — closed

`pkill -x elisa` (my-launched instance), verified: process gone, 0 MPRIS
services on the bus.

## Honest notes

- **Overshoot 216 ms** — same bounded poll-interval class as every timing
  test; an exact-frame stop would need `SetPosition`, whose busctl
  signature failed ("Too many parameters") and is left for the adapter's
  seek work alongside the relative-seek finding.
- The "position advanced while Paused" scare during the seek experiments
  was misdiagnosis on my part: the position was genuinely advancing because
  the earlier state was mid-track playback, not because Pause lied.
- Detection is energy-based, not semantic: I found the first transient of
  the sustained low-band pattern — for this track, the kick. A general
  "find the drop" detector is future adapter capability, not claimed.

## Receipts

```
kick detected:      59.280 s (3.6x baseline, strongest transient 45–75s)
play window:        30.000 s → 59.496 s
stop landed:        +216 ms past the detected onset
terminal state:     Stopped, position 0, app closed, MPRIS gone
```

## Verdict

**PASS.** The task required perceiving an audio event; the waveform
provided it. Track 4 played from 30s and stopped one-fifth of a second
past the first kick of the drop — and both new findings (relative Seek,
stalled PCM writes) are recorded for the adapter layer.
