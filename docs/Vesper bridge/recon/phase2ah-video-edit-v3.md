# Video Edit v3 — Full-Height Blur Correction (Session 2ah, final)

**Date: 2026-09-17 · Status: DELIVERED (v3) — blur now covers the full window height; Alex's screenshot defect resolved**

## Alex's rejection of v2 (correct)

His screenshot at 3:05 showed the "made with suno" text and metadata rows
**still readable below my blur rectangle**. My v2 region covered only
y 0.35–0.82; the actual Elisa window text spans from y≈0.04 (Title/Artist
rows at the top of the frame) down to y≈0.92 (file path row at the bottom).

## Root-cause measurement (pixel-level, this session)

Row-profile scan at t=178s across the panel zone (x 0.33–0.75):

- **Dark-text rows from y-frac 0.052 to 0.137** (Title/Artist/playlist rows
  at the top of the frame — 20+ dark-pixel rows)
- Sustained dark band 0.45–0.89 (the metadata rows inside my v2 blur)
- Text spike again at 0.89–0.93 (file path row)

**Conclusion: the panel spans y 0.04 → 0.93 of the frame.** My v2 box
missed the top 0.31 and the bottom 0.11.

## v3 fix

- Region: **x 480–1536, y 43–1004** (full panel height, 1056×961)
- Time: **174–187s** (panel confirmed visible 175→186; margins)
- Blur: boxblur radius 60, power 3 (unchanged — strong enough)
- One pass, audio copied, original untouched

## Verification (delivered file)

| t | Full-region edge energy | Reading |
|---|---|---|
| 170s (outside) | 1.11 | sharp |
| 175s (inside) | **0.13** | blurred |
| 180s (inside) | **0.24** | blurred |
| 185s (inside) | **0.12** | blurred |
| 187s (inside) | **0.04** | blurred |
| 190s (outside) | 3.32 | sharp |

Full decode RC=0 · 170.4 MB · H.264 1080p30 + AAC · original sha256
`dff9b909…` byte-identical.

## Delivered file (replaced)

`/home/Alex/Videos/agent vesper presentation - LINKEDIN.mp4`
Master: `/home/Alex/Videos/vb-edit/edited_v3.mp4`

## Process findings recorded

- **The worker loop blocks the Console** — pastes queue and never run;
  restarts were required between worker-code changes (five restarts this
  session). Extensions must load inside the worker at startup.
- **`GetItemListInTrack` is unusable in the worker thread** (returns 0
  items on a provably-populated timeline) — timeline verification must
  use frame-count arithmetic instead.
- **`CreateTimelineFromClips` with a stale/empty same-name timeline
  returns a shell**; unique names are mandatory.
- ffmpeg file-output stalls on this box; pipe-out via Python is the
  reliable route for both decode and frame extraction.
- The blur region must be measured from the video frames at the target
  time — not from a still screenshot taken at a different window
  geometry (the v2 miss).

## Verdict

**PASS.** The full Elisa window — top metadata rows through bottom file
path — is blurred for the entire panel-visibility window, verified by
row-profile measurement before and after, on the delivered file.
