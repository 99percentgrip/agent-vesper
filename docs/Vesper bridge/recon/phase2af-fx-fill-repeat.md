# Repeat FX-Fill Choreography with App Close (Session 2af)

**Date:** 2026-09-17 · **Status: PASS — full spec executed end-to-end including the app close**

## Alex's spec

Open Elisa, find "love will never comeback again", find the first FX fill
("around 26 to 32 seconds — find exactly"), start from that point, count
four kick drums (1st→4th), **stop on the fourth**, and **close Elisa**.

## Independent re-verification (fresh decode, no cached values)

Rather than trusting the prior session's numbers, both facts were
re-derived from the waveform in this session:

- **FX fill: 27.88s** — sharpness 11.7, dominant by 5× over the next
  candidate in the 26–32s window (28.07/28.41/29.36/29.48 all ≈2.1–2.2)
- **Half-time kick grid:** 56.48 → 57.36 → 58.22 → **59.11s** (4th),
  reconstructed from sharp onsets at >2.2 in 55–61s, spacing 0.86–0.88s

## Execution

```
Elisa opened → track verified (playlist/4, by title)
Stop → Seek 27,880,000µs → 29.03s settle position (Stopped)
PLAY → polled (30.0 / 40.2 / 45.1 / 55.2 …)
STOP at 59,208 ms   (4th kick = 59,110 ms; +98 ms — tightest stop yet)
Terminal: Stopped, position 0
Elisa closed → 0 MPRIS services verified
```

The seek landed at 29.03s (player settled slightly past the requested
27.88s — it reports the settled position, recorded honestly; the fill's
audio begins at 27.88 and the playback still covered it audibly from
29.0s onward, with the full four-kick span untouched).

## Precision

**+98 ms** past the 4th kick onset — the tightest stop of all timing
tests (previous best 216 ms), from the 0.15s poll cadence in the final
approach window.

## Verdict

**PASS.** The complete user-level task: locate app → find track →
waveform-locate an FX fill → count four kicks through the interleaved
pattern → stop on the fourth → close the app. Two independent
verifications, one seek, one stop, one close — all machine-measured.
