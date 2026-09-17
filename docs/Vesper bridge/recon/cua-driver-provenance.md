# Vesper Bridge — Cua Driver provenance record (Phase 4 prep; NOT INSTALLED)

## Status

**BLOCKED — not installed, not executed.** This record pins the executable
provenance required by VB-PRD-001 §9.3/§9.5 so that the *moment* Alex
authorizes the sidecar, installation can proceed against verified
identities. Nothing here constitutes driver operation or capability
evidence.

## Pinned release

| Field | Value |
|---|---|
| Product | Cua Driver (trycua/cua, `libs/cua-driver`) |
| Release tag | `cua-driver-rs-v0.28.1` |
| Published | 2026-09-12T08:45:39Z |
| Publisher | `cua-release-bot[bot]` |
| Upstream flag | **prerelease: true** — treat as candidate-only; a later stable promotion requires re-pinning |
| Release metadata | `recon/cua-release.json` (sha `ee418bee…`) |

## Target artifact (this machine's lane: Linux x86_64, XWayland lane only)

| Field | Value |
|---|---|
| Asset | `cua-driver-rs-0.28.1-linux-x86_64.tar.gz` |
| Size (from release API) | 30,563,145 bytes |
| **SHA-256 (upstream checksums.txt)** | `a068b6e477893b77ced74bceccf7db7483cf140e8d54150ce5849b6252b90bcf` |
| Checksums file | `recon/cua-0.28.1-checksums.txt` (pinned copy) |

Verification procedure (to run only after authorization): download the
asset, `sha256sum` it, compare against the pinned digest above, and only
then extract into a Bridge-owned directory. No ambient-PATH resolution
after initial configuration (§9.3).

## Lane decision (from Phase 0, unchanged)

This machine runs Fedora 44 KDE / `kwin_wayland` + XWayland. Cua's own
matrix (`recon/cua-platform.mdx` line 84): KDE/KWin is **Experimental** —
the KWin helper exposes read-only window identity/state, "not activation
or input"; raw target-addressed input refuses. Therefore:

- **Certifiable lane here: X11/XWayland** (apps presenting real X11
  windows) — "Supported with limits" per the matrix.
- KWin-native surfaces: refuse; a refusal there is a designed pass (AT-32
  portion).
- No nested-compositor demonstration may be cited as stock-Wayland
  capability.

## Required permissions at run time (to request explicitly)

- stdio child process supervision by the harness (no daemon by default).
- `bounded`-equivalent permission mode (manifest-admitted tools only);
  never `--dangerously-bypass-approvals`.
- Portal consent surfaces (ScreenCast/RemoteDesktop) stay user-facing;
  Bridge surfaces them, never bypasses (S20/S21).
- Screenshot/observation egress to the model provider is a **separate**
  grant from tool permission.

## Open items blocking Phase 4

1. Alex's explicit authorization to install the pinned sidecar (manual
   action; this task does not install software).
2. Selection of the first X11/XWayland test application for lane
   certification.
3. Portal ScreenCast v5 metadata availability probe on this Fedora 44
   install (serial/object-identity support).
