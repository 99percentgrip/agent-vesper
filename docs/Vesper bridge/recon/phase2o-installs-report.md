# Vesper Bridge — Increment 14: installs executed under Alex's authorization

## Objective

Alex authorized the installs ("if need to install install please if
missing mcp then install"). Executed: repaired Vesper binaries, the Cua
driver (digest-verified), and a first real lane measurement. Enrollment
rechecked against the live tool surface.

## What was done

1. **Repaired binaries installed.** Built
   `agent-vesper-tui`/`agent-vesper-acp` (release) from the repaired
   workspace and installed both to `~/.local/share/agent-vesper/`. The
   installed TUI bytes now contain `PRD requires 1–512 source
   paragraphs` (verified by byte inspection; the old binary truncated
   at the 256-era message). The running TUI process (pid 353218) was
   left untouched — the new binary takes effect on next launch.
2. **Cua Driver 0.28.1 installed.** Downloaded the pinned
   linux-x86_64 tarball; **SHA-256 matched the pinned digest exactly**
   (`a068b6e477893b77ced74bceccf7db7483cf140e8d54150ce5849b6252b90bcf`).
   Installed to `~/.local/opt/cua-driver-rs-0.28.1/` with
   `~/.local/bin/cua-driver`; **telemetry disabled** (persisted) before
   any other call; daemon serving on its socket.
3. **First real lane measurements** (this machine, KDE Wayland +
   XWayland):
   - `doctor`: binary/install/display OK; `XDG_SESSION_TYPE=wayland`,
     XWayland `:0` connected, AT-SPI bus reachable.
   - `get_screen_size`: 2880×1800 @1.0 — PASS.
   - `get_accessibility_tree`: process + window list — PASS.
   - `get_desktop_state` (full capture): **BLOCKED** — X11 `Match`
     error on GetImage. Root cause matches the Phase 0 finding: the
     stock-Wayland session has no portal capture path in 0.28.1 and
     XWayland GetImage cannot read the Wayland desktop. Manual action
     recorded: KWin ScreenCast portal (v5) probe, or run the X11 lane
     inside a real X session.
   - Input tools listed but **NOT exercised** — driving real windows
     needs its own go-ahead.
4. **Enrollment rechecked:** no `acceptance_enroll` exists on this
   session's tool surface (MCP registry auth-unavailable). The **new
   installed host binary** carries the 512 ceiling, so the next session
   launched from it can enroll the PRD natively — that is the fix for
   item 3, and it needed this restart to take effect.

## Files changed

- `docs/Vesper bridge/recon/compatibility-manifest.md` — Cua lane row
  (installed + measured facts + capture blocker), driver inventory row.
- This report.

## Evidence

| Item | Command | Result |
|---|---|---|
| Cua digest | `sha256sum` on download | exact match to pin |
| Cua health | `cua-driver doctor` | ok (binary, install, display, AT-SPI) |
| Telemetry | `telemetry disable` then `doctor` | disabled (persisted) |
| Screen size | `call get_screen_size` | 2880×1800@1.0 PASS |
| Process/window tree | `call get_accessibility_tree` | PASS |
| Full capture | `call get_desktop_state` | BLOCKED (X11 Match on GetImage; Wayland session) |
| Vesper binaries | installed; byte-verified ceiling string | DONE |

## Honest status

- Resolve remains the one truly external blocker (paid Studio install,
  licensed external scripting). Free edition is not a baseline: 21.1
  moved Python scripting to Studio.
- Cua lane: install and read-only observation verified; capture needs
  the portal or an X session; input needs explicit authorization.
- Enrollment: fixed at the binary level; requires a fresh host session.

## Next permitted steps

1. Restart the TUI/ACP host → enroll VB-PRD-001 natively.
2. KWin ScreenCast portal probe (or X session) → capture lane.
3. Explicit authorization → first input-tool certification on a
   disposable window.
