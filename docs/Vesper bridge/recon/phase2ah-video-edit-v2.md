# Video Edit v2 — Full-Panel Blur, All Visible Times (Session 2ah)

**Date:** 2026-09-17 · **Status: DELIVERED (v2) — full panel square blurred for every visible moment, LinkedIn file replaced, original untouched**

## Alex's rejection of v1 (both points correct)

1. **Region too small** — my band covered the middle; the Elisa metadata
   panel extends well below it (Channels / Bit rate / Sample rate /
   Duration rows stayed readable).
2. **Coverage gaps** — I blurred only 175–185s without checking whether
   the panel appears elsewhere in the video.

## What changed

**Region (from Alex's own screenshot):** the full metadata panel card —
x 480–1574, y 378–885 (1094×507 px, with margins) — heavy boxblur
(radius 60, power 3).

**Time coverage:** asked Alex directly whether the panel appears only at
2:55–3:05 or elsewhere. **Confirmed: only that window.** A full-video
pixel scan supported it (the 172–188s stretch is the only stretch whose
region signature matches the panel card; other high-energy moments are
ordinary scene changes elsewhere on screen). Blurred **172–188s** —
2s safety margin each side.

## Verification (delivered file)

| Check | Result |
|---|---|
| Full decode | RC=0, no errors |
| Duration / streams | 277.44s, H.264 1080p30 + AAC — unchanged |
| Panel edge @170s (outside) | 0.81 — sharp |
| Panel edge @180s (inside) | **0.13 — pure blur** |
| Panel edge @190s (outside) | 3.60 — sharp |
| Corner probes @180s | flat (min≈max at all 4 corners + center) — no readable structure anywhere in the square |
| Original sha256 | `dff9b909…` — byte-identical |
| Size | 172.2 MB MP4, faststart — LinkedIn-ready |

## Delivered file (replaced)

`/home/Alex/Videos/agent vesper presentation - LINKEDIN.mp4`

Master with the wider blur: `/home/Alex/Videos/vb-edit/edited_v2.mp4`.
Frames extracted for verification: `vb-edit/verify/` and
`.vb-frames/` (workspace).

## Findings recorded (2ag → 2ah)

- ffmpeg **file output stalls** on this box (pipe-out works) — all
  extraction/writing routed through Python subprocess stdout.
- Resolve scripted Fusion-composition route is **unusable in free 21.1
  worker context** (six measured limitations, `phase2ag` report).
- ffmpeg `boxblur` + masked overlay is the deterministic, verified route.

## Verdict

**PASS.** Full panel, all visible moments, verified unreadable, original
intact.
