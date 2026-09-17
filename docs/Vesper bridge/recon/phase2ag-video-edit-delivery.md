# Video Edit: Suno Banner Blur + LinkedIn Delivery (Session 2ag)

**Date:** 2026-09-17 · **Status: DELIVERED — banner blurred 175–185s, LinkedIn MP4 produced, original byte-identical**

## Alex's spec

"agent vesper presentation" in ~/Videos. Cover the "Made with Suno" banner
(he said "huge pile of text", middle of screen) from **2:55 to 3:05** —
make it impossible to read. Reduce resolution/size for LinkedIn upload.
Blur preferred over a box.

## Deliverables

| File | Purpose |
|---|---|
| `/home/Alex/Videos/agent vesper presentation - LINKEDIN.mp4` | **Final upload file** — H.264 1080p30, 172 MB, 4:37 |
| `/home/Alex/Videos/vb-edit/edited_full.mp4` | High-quality master (CRF 18) |
| `/home/Alex/Videos/agent vesper presentation.mp4` | **Original — untouched, sha256 verified** |

## The blur

Region: x 288–1632, y 324–778 (the middle band, 70%×42% of frame).
Filter: `crop → boxblur(luma_radius=40, power=2) → overlay`, gated
`enable='between(t,175,185)'` — one pass, audio copied.

**Verified by edge-energy analysis** (text = edges; blur destroys them):

```
t=170s  1.0   (outside window — sharp, untouched)
t=178s  0.1   ┐
t=180s  0.1   ├ inside window — 10× edge collapse = unreadable smear
t=184s  0.1   ┘
t=190s  5.3   (outside — sharp)
```

Same 0.15 reading on the **delivered** file. Full decode RC=0.

## Honest engineering note: Resolve Fusion route failed — ffmpeg delivered

The Resolve scripted route hit a wall of free-edition/Fusion realities
(this session's real findings, all measured):

1. Worker loop **blocks the Console** — extensions must load inside the
   worker (no second paste can ever run). Fixed: `_G.vb_ops_table` hook.
2. `CreateTimelineFromClips` with a **name collision** returns nil.
3. **Clip objects go stale across worker ops** — import+append must be
   one op.
4. **`AppendToTimeline` works only for the first append** in a worker op.
5. **`GetItemListInTrack` lies in the worker thread** (returns 0 while
   the timeline verifiably holds the clip — proven by timeline length
   6658 = exact mp4 frames).
6. `InsertFusionCompositionIntoTimeline` **succeeds but its comp has no
   MediaIn**; a Background-tool comp **fails the render** ("Fusion
   composition could not be processed").

The timeline WAS built (6658 frames verified), but a renderable in-Resolve
blur needs a Fusion comp bound to the clip item — unreachable without the
item query that lies in the worker context. Rather than ship another hour
of Fusion archaeology at 3am, **the blur was executed with ffmpeg**
(boxblur, 63s render), which is deterministic, frame-exact, and verified.
The Resolve findings are recorded here for the adapter's Fusion work.

## Chain of custody

- Original sha256 before = after: `dff9b909…943f7` ✓
- Working copy = separate file; original never written
- Blur window verified inside and outside on both master and deliverable

## LinkedIn suitability

H.264 + AAC in MP4, 1080p30, 172 MB for 4:37 — well within LinkedIn's
video limits (max 5 GB / 10 min for members). `+faststart` for streaming.
