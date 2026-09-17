# Video Edit v4 — Tight Text-Box Blur (Session 2ah, corrected)

**Date: 2026-09-17 · Status: DELIVERED (v4) — blur box sized to the text block only; outside content sharp**

## Alex's rejection of v3

"Too big!" — the v3 full-height box swallowed half the screen. The
metadata text actually occupies a compact block right-of-center.

## v4

- **Box**: x 460–1573, y 378–831 (1113×453) — sized to the text block
  only (Title → Duration rows), measured from Alex's screenshot geometry
- **Time**: 174–187s (unchanged, Alex-confirmed window)
- **Blur**: boxblur radius 60, power 3 (unchanged)

## Verification (delivered file)

| Probe | Edge energy | Reading |
|---|---|---|
| Inside box @176/180/185s | 0.11 / 0.12 / 0.11 | fully blurred |
| Outside-left @176–185s | 1.41–1.58 | sharp — only the box is blurred |
| Outside-right @176–185s | 3.13–5.72 | sharp |
| Full decode | RC=0 | |
| Original sha256 | `dff9b909…` MATCH | |

## Delivered file (replaced)

`/home/Alex/Videos/agent vesper presentation - LINKEDIN.mp4` — 172.4 MB,
H.264 1080p30 + AAC, faststart. Master: `vb-edit/edited_v4.mp4`.

## Verdict

**PASS.** The blur is now exactly the size of the text block — the rest
of the video remains sharp.
