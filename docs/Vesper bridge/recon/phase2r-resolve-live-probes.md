# Phase 3 — Resolve Free 21.1 Live Probes (Session 2r)

**Date:** 2026-09-16 · **Lane:** Phase 3 route discovery + channel proof · **Status: PASS — marker channel proven live**

## Objective

Establish, from live evidence on Alex's installed free edition, which scripting
route Bridge can use, and prove a complete request/response channel — without
inventing capabilities or trusting stale third-party reports.

## Environment

| Fact | Evidence |
|---|---|
| Install path | `/opt/resolve` (bin, `ResolvePython`, `Developer/Scripting`) |
| Version | `21.1.0.0017_lite` (binary strings) — **free edition**; Alex confirmed "Free/regular Resolve" |
| Bundled Python | 3.14.4, `import DaVinciResolveScript` works out-of-box (no env vars) |
| Vendor README | `/opt/resolve/Developer/Scripting/README.md` — dated 31 Aug 2026 |
| Current project | "test" (Alex's own), timeline "vb_channel" created by probe |

## Probe results (all bounded, hard `timeout` on every call)

| # | Probe | Result | Verdict |
|---|---|---|---|
| 1 | `resolve --version` | **Blocks forever** (initializes GUI) — root cause of the earlier 19,000s hang; zombie killed | Documented hazard; never call unbounded |
| 2 | `ResolvePython` import + `scriptapp` (app down) | import OK; connect → `None` **fast** | No hang risk on module use |
| 3 | App running, poll `scriptapp` 30×5s | `None` always; **port 1144 never listens** | **External scripting = Studio-gated in free 21.1** (vendor README: "Studio… listens on 1144") |
| 4 | `fuscript -p` / `-P` / Lua `scriptapp` | Empty / `None` | Gate confirmed; not a network issue |
| 5 | Console `print(resolve.GetVersionString())` | **"21.1 Build 17"** | **Internal scripting WORKS in free** |
| 6 | Lua `dofile` v3 probe | **`VB3\|21.1.0.17\|DaVinci Resolve\|New Project 2\|0\|true\|-`** | Full API reachable inside |
| 7 | Console Lua `io.*` | `attempt to index global 'io'` | Console Lua is **sandboxed** (no file I/O) |
| 8 | `proj:SetComments` | Not a method in 21.1 free | Minor; marker channel replaces it |
| 9 | Scripts **menu** route (`Workspace → Scripts → Utility`) | **Not present in free** — no such menu item on Alex's install; binary contains no `Workspace…Scripts` menu string; no default scripts shipped under `/opt/resolve/Fusion/Scripts` | Scripts-menu route **unavailable in free**; Console is the only in-app entry |
| 10 | KWin scripting (independent window observation) | `resolve\|DaVinci Resolve - New Project 2` via `loadScript`+`run`+journalctl | Window observation channel works (read-only, DBus) |
| 11 | **Marker channel (v4)** | **`VB4\|marker_set=true\|tl=vb_channel\|proj=test` + `readback=VB4 cap 21.1.0.17 free`**; Alex confirmed the blue `vb_hello` marker is **visible in the Edit page** | **Request/response channel PROVEN: write → read-back → human-visible effect** |

## Channel design (evidence-derived)

**Request/response = Timeline markers with `customData`.**

- Inside the app, Console Lua can: `tl:AddMarker(frame, color, name, note, duration, customData)`
  and `tl:GetMarkerCustomData(frame)` — **proven live** (probe 11).
- The host (Bridge, outside the app) observes results through:
  a) **rollinglog**: Console Lua **errors** land in `/opt/resolve/logs/rollinglog.txt`
     (probe 7's error is there verbatim) — usable as a completion/error signal;
  b) **KWin scripting** for window-state observation (probe 10);
  c) **marker visibility** is human-checkable and API-readable inside.
- Console Lua `print` output does **not** land in rollinglog (only errors do) —
  so the print channel is human-visible only; error-channel is machine-readable.
  Final design: **markers = state, rollinglog ERROR lines = async error signal,
  human paste = admission**.

## Honest limitations

- One human paste per Resolve launch (admission) — real UX cost, recorded.
- `print` output is not machine-readable from outside; only ERROR lines are.
- Probe 6's `New Project 2` was the state at that moment; probe 11 ran in
  Alex's project "test" — no mutation of his real media; a new empty timeline
  `vb_channel` was created inside "test" **with his on-screen confirmation**.
- No render/import mutation tested yet — next gated step.

## The 19,000-second hang — root cause

`resolve --version` initializes the Qt GUI stack instead of printing. My
earlier unbounded call sat in that state until Alex cancelled it. Every probe
since runs under `timeout`; the hazard is documented in the manifest.

## Files

- This report: `docs/Vesper bridge/recon/phase2r-resolve-live-probes.md`
- Probe scripts (tracked): `recon/probes/vb_bootstrap.lua`, `recon/probes/vb_v4.lua`
- Installed probes: `/home/Alex/.local/share/DaVinciResolve/Fusion/Scripts/vb_bootstrap.lua`
- Menu probe (unused by final design, kept as evidence): `…/Scripts/Utility/vb_menu_probe.py`

## Next permitted steps

1. Fixed command registry inside the bootstrap (typed ops: list_projects,
   create_disposable_project, import_fixture, append_to_timeline, add_marker,
   render_draft) — each mapped to Bridge authority + verification.
2. First mutation under Bridge authority: create disposable project, import
   the fixed media fixture, render draft, **verify the output file exists and
   decodes** — the PRD's verification standard.
3. Compatibility manifest: record Studio-gate proof + Console/marker route.
