# Phase 3 — Resolve Vertical Slice: LIVE VERIFIED (Session 2s)

**Date:** 2026-09-16 · **Status: PASS — full slice executed on Alex's free 21.1 install, output file independently verified**

## Objective

Execute the PRD's first useful Resolve flow on the real installed application —
inspect/import → working timeline → draft render → **actual output validation** —
with the original media provably untouched.

## The slice, leg by leg (all live evidence)

| Leg | Command | Live result |
|---|---|---|
| Registry load | `dofile(...vb_bridge.lua)` | `VBOK\|ready\|ops=status,fixture_import,timeline_from_fixture,render_draft,render_start,render_status` |
| App state | `vb_run("status")` | `VBOK\|status\|app=DaVinci Resolve\|ver=21.1.0.17\|proj=test\|tl=vb_channel\|tlcount=1` |
| Import | `vb_run("fixture_import")` | `VBOK\|fixture_import\|clip=fixture_master.webm\|path=…\|count=1` |
| Timeline | `vb_run("timeline_from_fixture")` | `VBOK\|timeline_from_fixture\|tl=VB Slice Timeline\|dur_frames=2245` |
| Render queue | `vb_run("render_draft")` | job `8bc781c4-ae21-4535-adbd-82f47d7994ba` queued (via diag4 step sequence) |
| Render start | `vb_run("render_start")` | `VBOK\|render_start\|started=true\|jobs=1` |
| Render status | `vb_run("render_status")` | **`status=Complete\|pct=100\|err=-`** |
| Output file | host-side | **`/home/Alex/Videos/vb-fixture/out/VB Slice Timeline.mkv`, 1,597,118,449 bytes** |
| Decode check | `ffmpeg -xerror -f null -` | **RC=0 — every frame decodes** |
| Frame count | `ffprobe -count_frames` | **2245/2245 — exact match to timeline duration** |
| Duration | ffprobe | **93.542s vs source 93.545s** |
| Original integrity | sha256 ×3 | **identical: `196b3e4c…` — original untouched** |

Output format: **ProRes 422 HQ in MKV, 1920×1080@24, FLAC audio** (timeline
resized 2560×1440@60 → 1080p24 by the project default — recorded, not hidden).

## Defects found and fixed mid-slice (the evidence-driven method working)

1. **`ImportMedia` dict form returns `nil` on free 21.1** — the documented
   `ImportClipInfo` shape does not work; the **legacy string-list form does**
   (`diag`: `string_form_count=1`). Registry switched to the proven form.
2. **`GetRenderFormats` map is name→extension with UPPERCASE keys** (`MP4="mp4"`)
   — my first lookup used the extension as key.
3. **Free edition has NO H.264/H.265 encoders.** Measured: mp4 = `APVYUV422_10`
   only; mov/mkv = ProRes/DNx/FFV1. `H264_NVH` etc. exist in the binary but are
   **Studio-gated** (consistent with the StudioOnly prompt handler string).
   Chose **MKV + ProRes 422 HQ** — free, professional, fully verifiable.
4. **`CustomName` in `SetRenderSettings` breaks `AddRenderJob`** on free 21.1
   (`diag3` isolated it; `diag4` proved the pair works without it).
5. **`mov:ProRes422HQ=false` but `mkv:ProRes422HQ=true`** — same codec,
   different container acceptance; measured, not assumed.

## Adversarial-content posture (PRD §8)

- Every op is a **typed command** in a fixed registry — no model text is ever
  `eval`'d; unknown ops return the allowed-op list.
- All app-originated strings (clip names, project names, job ids, error text)
  pass through `esc()` (strips `\|`, CR/LF, quotes, control chars; 200-char cap)
  before any output channel — **no delimiter injection into the CSV protocol**.
- Render target dir is **host-fixed** (`vb-fixture/out`), not model-controlled.
- Media metadata was treated as untrusted throughout; nothing from the file
  was executed or concatenated into a command.

## Honest limitations

- The slice ran in Alex's existing `test` project (his explicit choice of
  fixture, on-screen confirmations at each step); a dedicated disposable
  project (`CreateProject`) is the cleaner future default — recorded.
- Render used the project's default 1080p24 timeline settings; explicit
  `SetRenderResolutions`/timeline settings control is future work.
- Console paste = admission, per ADR 0030's free-edition route. One paste per
  launch remains the honest UX cost.
- Output is a **technical** success. Human editorial approval of the content
  is a separate verdict Alex owns.

## Artifacts

- Registry: `recon/probes/vb_bridge.lua` (6 typed ops, syntax-verified outside app)
- Diagnostics: `recon/probes/vb_diag.lua` … `vb_diag4.lua` (each pinned a
  specific hypothesis with a measured answer)
- Output: `/home/Alex/Videos/vb-fixture/out/VB Slice Timeline.mkv` (1.49 GiB)
- Fixture: `/home/Alex/Videos/vb-fixture/fixture_master.webm` (copy, sha-verified)
- Original: `/home/Alex/Videos/Vesperpresentatio.webm` — **byte-identical throughout**

## Verdict

**PASS.** The PRD's first useful Resolve flow — import → timeline → render →
verified output — executed end-to-end on the real free 21.1 install, through
typed Bridge commands, with frame-exact output validation and proof the
original media was never touched. Phase 3's vertical slice is real.
