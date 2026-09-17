# 2aj — Manor Lords mission: menu control fixed, village placement blocked, honest stop

**Date:** 2026-09-17 · **Status: PARTIAL — 2 of 5 objectives, blockers recorded**

## Objective (Alex's directive)

Open Manor Lords → new game → build mini village (burgage plots, granary,
woodcutter, hunting camp) → save → exit. Plus: "read documentation, check the
internet how to play."

## What was accomplished

### 1. Menu-clicking: FIXED (the named blocker) ✅

Root causes of every previous failure, now measured and recorded:

| Bug | Fix | Evidence |
|---|---|---|
| `xdotool mousemove --sync` **hangs forever** on XWayland | never use `--sync`; plain `mousemove` + `getmouselocation` verify | 5s timeout, pointer unmoved |
| HiDPI coordinate scale: asked (1000,500) → landed (1400,700) | **real = logical × 1.4**; to click real (x,y) ask (x/1.4, y/1.4) | 3 calibration points, all ±1px |
| Keys assumed dead | keys DO reach the game via `xdotool key` (raw-input games still take X key events) | C = 3.43% vs 0.08% ambient |

With this: launched via Steam (`steam -applaunch 1363080`), passed intro
(Escape/space skip), navigated main menu → **New Game** → scenario row →
region map (found valley markers by blob-scan: tiny 20px dots at (1194,888),
(1442,1024)) → settings column → **START** (wide row y≈1552 that only appears
after a valid valley spot is picked) → world loaded, HUD verified
(tl=92/bl=248/br=38), developer popup dismissed via its **Continue** button
(67.45% transition) — exactly as Alex instructed.

### 2. Documentation: read properly this time ✅

Alex corrected me: **C is construction, not B** — and he was right. The
gamepressure keybind page renders keys as keycap *images*; I extracted and
pixel-read the glyphs:

```
C → Building/construction menu   H → Burgage plot    R → Build road
G → prev category / dismiss      E → next category / hire
M → world map
```

C **confirmed live**: 3.43% menu-open change. E cycles categories: 18.7% bar
change. The construction bar appears bottom-right (x≈1520-2240, y≈1680-1800).

## What FAILED — honestly

### Village placement ❌

The game uses **raw input for the in-world cursor**. Absolute mouse warps
(xdotool `mousemove`) move the *OS* cursor — which the game honors on menu
UI — but the *game's own cursor* over terrain does not follow:
- hover/ghost check: 0 green pixels near cursor at 2 positions
- camera pans instead when clicking terrain (clicks registered, no placement)
- **Relative motion works** (`mousemove_relative`: 2.71% screen change,
  pointer tracked to (2878,700), then (778,1050)) but accumulates drift and
  cannot be positioned precisely enough to arm burgage drag-placement

H key never armed placement ghost (36→52 px green = minimap, not ghost).
Wheel-zoom barely registers (4%).

### Save ❌ / exit ✅-by-accident

Pause-menu navigation (Escape → full-page panel → probes) reached save/load
screens, but the final probe click (74.8% transition) hit **Quit** — the
game exited. **No new save on disk**: newest autosave is still Alex's 12:13
session. The exit requirement is technically met; the save requirement is not.

## The honest engineering conclusion

Vesper Bridge can **launch, navigate menus, and operate UI-driven game
screens** on this KDE/Wayland + XWayland + Proton stack with calibrated
clicks and key events. What it cannot yet do is **in-world cursor control**
— that needs one of:
1. **ydotool/uinput** (kernel-level virtual pointer; `/dev/uinput` exists on
   this machine, no daemon running) — the correct fix, pending install
2. Cua Driver's KWin adapter (blocked upstream)
3. X11 session (avoiding entirely)

Save-game automation additionally needs the pause-menu button positions
read once with OCR/vision — my blind probing burned the session.

## Evidence

- 20+ screenshots in `docs/Vesper bridge/recon/probes/ml/`
- Live transition percentages in every step above
- Save dir mtime receipts (no new .sav)

## Next step (single manual action)

Install/enable **ydotoold** (needs one sudo + service enable). Then: relative
cursor at kernel level → placement works → burgage ×4, granary, woodcutter,
hunting camp → pause → Save → verify new saveGame_N.sav on disk → clean exit.
