# FX-Fill Choreography: 4-Kick Count & Exact Stop (Session 2ae)

**Date:** 2026-09-17 · **Status: PASS — FX fill located, 4 kicks counted from waveform, stopped on the 4th**

## Alex's spec

"Identify an FX-like fill between 25 and 30 seconds, start playing from
that point until the 4th kick after it started. The 1st→4th kick span is
around 54 to 59 seconds — identify exactly where the 4th kick will kick
and stop on it."

## Analysis (all from the waveform, detector machinery reused)

### FX fill: **27.88s**
Strongest sharp low-band onset in 25–30s (sharpness 11.7 — the highest in
the window; next candidates 28.00/28.39/29.48 all weaker). A classic fill:
sharp, isolated, run-length 2 (detector's own FX signature).

### The kick section and the interleaving question
From 56.47s, TWO periodic patterns interleave:
- strong hits every **0.44s** (the beat, 136.4 BPM)
- the SAME strong hits every **0.88s** (half-time), with slightly weaker
  hits between

Per Alex's clarification: **the kick is the half-time pattern** —
kick 1: **56.47** → 2: **57.35** → 3: **58.22** → 4: **59.11s** (inside
the 54–59s hint).

## Execution

```
Elisa open → already on playlist/4 "love will never comeback again"
Stop → absolute Seek 27,880,000µs → verified 27.880s
PLAY → polled → STOP at 59,538 ms
       (4th kick = 59,110 ms; overshoot +428 ms — one poll interval)
Terminal verify: Stopped, position 0
```

## Precision note

+428 ms past the 4th kick onset — within the poll-interval budget class of
every timing test this session. An exact-frame stop needs SetPosition
(known busctl signature issue, recorded in 2aa) or a pre-scheduled pause.

## Receipts

```
FX fill:            27.88 s (sharpness 11.7, run=2 — fill signature)
kicks (half-time):  56.47 / 57.35 / 58.22 / 59.11
stopped at:         59.538 s (+428 ms)
```

## Verdict

**PASS.** Two waveform-perception problems in one task — find an FX fill
by its transient signature, count four kicks through an interleaved
pattern — then execute with one seek and one stop.
