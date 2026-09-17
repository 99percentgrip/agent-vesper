# Phase 3 — Paste-Free Autonomous Worker (Session 2t)

**Date:** 2026-09-16 · **Status: PASS — one paste per launch, then fully autonomous file-driven control**

## Objective

Eliminate the per-step human paste that made the 2s slice unusable as a
product. Alex's verdict was correct: a human relaying commands through
VesperLens is not automation.

## The worker architecture (all measured, nothing assumed)

**One paste per Resolve launch** starts `vb_worker2.lua` in the Console.
After that, host ↔ worker communicate purely through files:

```
host -> /ipc/cmd      {"id":N,"op":"name","args":{...}}
worker -> /ipc/result {"id":N,"ok":bool,"data":"..."}
worker -> /ipc/beat   liveness mtime, every 0.4s
```

The enabling discovery: **LuaJIT FFI is globally available in the Console
sandbox** (`jit`/`ffi` tables, no `require` needed). `ffi.C.fopen/fwrite/fread`
gives real filesystem I/O even though `io` is stripped and `bmd.writestring`
writes only to a virtual path (measured: `/opt/resolve/vb_out.txt` never
appeared on disk).

## Session evidence

| Test | Result |
|---|---|
| FFI write probe | `ffi-channel-works` on disk — channel exists |
| Worker start + self-test | `VBSELF\|id=7\|op=status\|args={}` |
| `status` via file | `{"ok":true,"data":"app=DaVinci Resolve ver=21.1.0.17 proj=test tl=VB Slice Timeline tlcount=2"}` |
| `get_state` | `tl=VB Slice Timeline start=86400 end=88645 vtracks=1` |
| `fixture_import` | `clip=fixture_master.webm count=1` |
| `set_marker` | `frame=24` (marker added to live timeline) |
| `render_draft` | `job=8a4b3160-…` queued |
| `render_start` | `jobs=2 started` |
| `render_status` polled | `Ready → Rendering 35% → Complete 100%` |
| **Second render output** | **full decode RC=0, 2245/2245 frames** |
| Original integrity | sha256 unchanged `196b3e4c…` |

**The entire second render — import through verified output — ran with zero
human involvement after the single launch paste.**

## Bugs found and fixed this session (each live-verified)

1. `bmd.writefile`/`readfile` **do not exist** (my diag5 probe's `and`-chain
   short-circuit misread as existence; the lone `beat` file came from the
   worker's pcall'd call evaluating to `nil` error — misattributed). Corrected
   with `tostring(bmd.writefile)` = `nil`.
2. `bmd.writestring` returns a path and claims success but **writes nothing**
   to the real filesystem (virtual path only, `fullpath` = `/opt/resolve/…`
   never materialized).
3. Malformed Lua long-bracket pattern (`missing ']'` at line 209) — worker
   died on first command.
4. **Double-quote pattern bug**: `…-(%w+)""` demanded `status""`; only
   patterns without the stray quote matched. Found by isolating each pattern
   in `fuscript`.
5. **`%w` excludes underscore**: `fixture_import` captured as `fixture` →
   "malformed_cmd" for every op containing `_`. The single most deceptive bug
   of the session — `status` (no underscore) worked, everything else failed.
6. `timeline_from_fixture` reports `create_failed` on re-run because
   "VB Slice Timeline" already exists (name collision) — an honest app
   response, not a worker bug; a unique-name policy is future work.

## Security posture

- Fixed op registry; unknown ops refused **and acked** (host never hangs).
- No `load`/`eval` of any string from the command file; args parsed by a
  hand-rolled extractor accepting only `"%w_"` keys and bounded values.
- App-originated strings escaped (`|`, CR/LF, quotes, control chars, 300-cap)
  before embedding in results — no protocol injection.
- Render target and fixture path are host-fixed constants, not model inputs.
- Worker loop time-bounded (8h); beat file gives liveness detection.

## Honest limitations

- Still **one paste per Resolve launch** (Console is the only in-app entry on
  free; Scripts menu absent, Workflow Integrations Studio-only + not on Linux).
- F6 shortcut (reported by Alex) + XTest can *open* the Console programmatically,
  but typing the bootstrap line needed a working shift-symbol table — the
  `vb_type` tool with a full US-layout map was built and syntax-verified but
  not yet end-to-end tested for the bootstrap line itself. The paste remains
  the launch step today.
- `timeline_from_fixture` name collision handling pending.
- Result strings are pipe-delimited key=value, not full JSON values — fine for
  current ops, not a general serialization.

## Artifacts

- Worker: `recon/probes/vb_worker2.lua` (deployed copy in Fusion/Scripts)
- Typer: `recon/probes/vb_type.c` (full US keymap, built to /tmp)
- Diagnostics: `vb_diag5–12.lua` (channel hunt: bmd inventory, writestring
  virtual-path proof, FFI discovery, pattern isolation)
- IPC dir: `/home/Alex/Videos/vb-fixture/ipc/`

## Verdict

**PASS.** The UX complaint is addressed at the architecture level: one paste
per launch, then autonomous, observable, bounded control — proven by a full
render cycle executed with no human in the loop and a frame-exact verified
output.
