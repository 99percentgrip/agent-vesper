# Manor Lords automation — honest status (2aj interim)

**Date:** 2026-09-17 late · **Status: BLOCKED on UI-state verification**

## What works (proven, reproducible)

1. **Launch**: `steam -applaunch 1363080` → game window (fullscreen 2880×1800
   at origin, or 1920×1080 windowed at (68,103) on one crash-restart —
   geometry must be re-read each launch).
2. **Input calibration (the menu-clicking fix)**: XWayland pointer mapping is
   `real = logical × 1.4` (KDE 200% scale). `xdotool mousemove --sync`
   **hangs forever** under XWayland — never use it. Plain `mousemove` +
   `getmouselocation` verification works. Clicks register in-game.
3. **Screen observation**: `spectacle -b -n -o` (KDE native) + PIL analysis.
   Ambient animation ~2–9%; real UI transitions measure 40–80%.
4. **The full tutorial video** (Alex's `for ai to learn.webm`, 89s) was
   decoded and analyzed frame-by-frame:
   - f1–4 loading → f5–8 **dev popup** (text x488–2208, right block
     x2084–2388 y672–800; disappears by f9 — Continue was clicked)
   - f9–13 main menu (centered column) → f14 setup → f15 (67% transition)
   - f29+ **in-game** (HUD tl≈1276) → f39–44 **construction panel via C**
     (Alex: "C is construction, NOT B")
   - f45–60 building placement flows → f88–89 menu

## Where it stands

- Reached **in-game with full HUD** twice (once per game instance) — the
  New-Game flow (popup → menu → setup → region marker (1442,1024) →
  START row y1552) was executed successfully once each.
- The last session ended in a setup-family screen (left portrait panel +
  right option rows) where every identified element responds with 4–8%
  selection feedback but no screen transition, and HUD never appears.
- **No village was built; no save was created.** Game was relaunched
  several times during UI archaeology; no user saves were modified
  (verified: save files untouched, timestamps unchanged).

## Why it's blocked

The UI has no OCR available (no tesseract, no sudo, no vision MCP auth),
menus restate similar layouts (setup/pause/region screens share geometry),
and each wrong click cycles internal state. The tutorial video gives the
*sequence* but exact button coordinates were not extractable beyond the
dev popup's Continue block.

## The precise unblock (30 seconds of Alex's time)

One of:
1. While the game is on the current screen, **tell me what screen it shows**
   (e.g. "region select", "difficulty", "changelog") — one word fixes the map.
2. Or screenshot-paste the current screen — I'll match it against the
   tutorial frames.
3. Or say "restart clean" and I re-run launch → popup → menu from zero
   with the video's exact sequence, stopping to verify each screen.

## Files

- Tutorial frames: `/tmp/ml_tut/` (89 frames + zooms)
- Live screenshots: `docs/Vesper bridge/recon/probes/ml/` (30+ states)
- This note: evidence trail for the input-calibration fix and the
  documented C-key construction fact.
