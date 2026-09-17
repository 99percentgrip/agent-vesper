# Vesper Bridge — Compatibility manifest (living document)

All statuses are measured facts at the time of writing. **Not certified**
is the default for unresolved rows; a blocked lane is never reported as
passed.

## Host environment (this machine)

| Item | Value | Evidence |
|---|---|---|
| OS | Fedora 44 (KDE Plasma), kernel 7.2.4-200.fc44 | `uname -a`, `/etc/os-release` |
| Compositor | `kwin_wayland` + XWayland `:0` (Plasma session) | `ps aux` (Phase 0 report) |
| Session | `XDG_SESSION_TYPE=wayland`, `XDG_CURRENT_DESKTOP=KDE` | env probe |
| Rust | stable 1.95.0 active; 1.88.0 installed for MSRV gate | `rustup toolchain list` |
| ffmpeg/ffprobe | present (`/usr/bin`) | `command -v` |
| Python 3 | present (system) | `command -v` |

## Application lanes

| Lane | Status | Blocker / manual action |
|---|---|---|
| DaVinci Resolve (Phase 3) | **INSTALLED — free 21.1.0.0017_lite at `/opt/resolve`; VERTICAL SLICE VERIFIED (2s)** | External scripting Studio-gated (port 1144 never listens; vendor README). Route: Console-initiated fixed Lua bootstrap, one human paste per launch; markers + print as channels. **Slice PASS (2s):** import → timeline (2245 frames) → render → output `VB Slice Timeline.mkv` **frame-exact verified (2245/2245, full-decode RC=0, 93.542s)**; original sha256 unchanged. **Free-edition measurements:** no H.264/H.265 encoders (mp4=APV only; MKV+ProRes chosen); `ImportMedia` dict form broken, string form works; `CustomName` in RenderSettings breaks `AddRenderJob`; Scripts-menu absent. `resolve --version` blocks forever. Evidence: `recon/phase2r-resolve-live-probes.md`, `recon/phase2s-resolve-vertical-slice.md`. |
| MPRIS media players (Elisa-class) | **MEASURED BEHAVIORS (2y/2z) — adapter guards active** | Elisa 24.x/KDE, live-measured on Alex's machine: (1) `Previous` while **paused** restarts the current track instead of navigating — adapter refuses with guidance rather than mutating; (2) **rapid `Next`/`Previous` inversion crashes the player's MPRIS handler** (observed: service vanished, process dead) — adapter enforces a 1200 ms inversion cooldown; (3) navigation settles only on **trackid change**, never a call ack. Product path 22 ms (2x). Evidence: `recon/phase2y-three-track-choreography.md`, `recon/phase2z-adapter-behavior-guards.md`. |
| Generic desktop via Cua Driver (Phase 4) | **INSTALLED; portal probe APPROVED+PASS at OS level; Cua 0.28.1 capture still BLOCKED on KDE Wayland** | Driver 0.28.1 digest-verified (`a068b6e4…` exact), installed `~/.local/opt/cua-driver-rs-0.28.1/`, telemetry disabled, daemon serving. Read-only tools PASS here: screen size (2880×1800@1.0), process/window tree. **Portal verified 2026-09-15 (Alex approved the dialog):** xdg Screenshot portal returned 4 real 2880×1800 RGBA captures — the KDE portal and Alex's grant work end to end. **Cua itself still refuses capture on this session:** `get_desktop_state` → X11 `Match`/GetImage; 0.28.1 has no KWin-native path (wayland-helper is GNOME-Shell-only; upstream README states KDE needs a KWin activation adapter "not yet provided"; portal reachability is insufficient because RemoteDesktop/libei input is focus-global). **Certifiable lanes:** an X11 session (`XDG_SESSION_TYPE=x11`), a GNOME session, or the upstream KWin adapter. Input tools listed, NOT exercised. |
| Browser applications (second workflow) | **READY via existing route** | No new driver; `vesper-web`/browser tooling is the intended path (AT-36 candidate). |
| Games (Phase 6) | DEFERRED | Out of scope until media/application releases land. |

## Driver inventory (pinned)

| Driver | Version | Digest | Lane | Notes |
|---|---|---|---|---|
| Cua Driver | 0.28.1 (tag `cua-driver-rs-v0.28.1`, **prerelease**) | `a068b6e477893b77ced74bceccf7db7483cf140e8d54150ce5849b6252b90bcf` (linux-x86_64 tar) — **verified at install** | X11/XWayland only | **Installed** 2026-09-15 at `~/.local/opt/cua-driver-rs-0.28.1/`, symlink `~/.local/bin/cua-driver`; telemetry disabled (`telemetry disable`, persisted); provenance record `recon/cua-driver-provenance.md`. KWin-native refuses by design. |

## Protocol revisions

| Surface | Revision in use | Notes |
|---|---|---|
| Vesper MCP client | `2025-06-18` legacy initialize flow | `crates/vesper-mcp/src/mcp.rs:21`; compatible with Cua's retained legacy path |
| MCP target revision | `2026-07-28` | Not implemented in Vesper; adoption is AD-07 work behind tests |

## Bridge implementation status

| Unit | Status |
|---|---|
| `vesper-bridge` pure core | LANDED (Phase 1 complete: 46 tests + 6 fake-driver scenarios; MSRV 1.88 clean) |
| Harness `bridge` feature (default-off) | LANDED (Phase 2/2b: 131 tests on, 116 off; settings fail-closed) |
| TUI/ACP command surface | LANDED (advertisement parity; `/bridge` read-only host answers) |
| Real adapter | **NONE** — no driver, no application control exists in production |
| Native enrollment | BLOCKED (host binary predates ceiling repair; live reviewer window) |

## Verification evidence locations

- Phase 0: `recon/phase0-report.md` (+ digest-pinned upstream sources)
- Phase 1: `recon/phase1-report.md`, `phase1-fake-driver-report.md`
- Phase 2: `recon/phase2-report.md`, `phase2b-report.md`
- Threat model/AT plan: `recon/threat-model-and-test-plan.md`
- Enrollment mechanics: `PRD-ENROLLMENT-NOTE.md`
