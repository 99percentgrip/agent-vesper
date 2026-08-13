# Installation scripts

## Purpose

Own the cross-platform installers and uninstallers that download one verified
release archive, install or remove both `agent-vesper-acp` and
`agent-vesper-tui` on the user's PATH, and produce the same first-run UX as the
original Python `native-glm-acp` installer. Also owns the ACP Registry PR
submission payload used to sync the published `agent-vesper` registry entry
with each release.

## Ownership

- `install.sh` — POSIX installer (Linux + macOS). Downloads the
  `agent-vesper-acp-<platform>-<arch>.tar.gz` release, verifies SHA-256,
  installs both launchers under `$XDG_BIN_HOME` (default `~/.local/bin`),
  updates the shell profile PATH, and bundles the standalone `uv` binary
  (from `astral-sh/uv` latest release) into the bundle dir so the
  push-to-talk voice backend can auto-bootstrap a `faster-whisper` venv via
  the bundled `uv` with no external venv toolchain. (The bootstrap's
  `python3 -m venv` fallback still needs system `python3`+`python3-venv`, but
  is only reached if the bundled `uv` is absent.) A failed `uv` download is
  non-fatal.
- `install.ps1` — Windows installer. Downloads the
  `agent-vesper-acp-windows-x86_64.zip` release, verifies SHA-256, installs a
  `.cmd` launchers under `%LOCALAPPDATA%\Programs\AgentVesper`, and adds that
  directory to the user PATH.
- `uninstall.sh` — POSIX uninstaller. Removes only the launcher, bundle, and
  exact shell-profile PATH marker owned by `install.sh`.
- `uninstall.ps1` — Windows uninstaller. Removes only the launcher, bundle,
  and exact user PATH entry owned by `install.ps1`.
- `acp_pr_439.md` — VRO-9 manual submission payload for the public ACP
  Registry PR #439 (`agentclientprotocol/registry`). Data-only scaffolding:
  contains the manifest JSON (mirroring `registry/agent.json`), the PR title,
  the PR body, and the asset-URL audit table. Not executed; the local
  installer (`install.sh` / `install.ps1`) consumes `registry/agent.json`
  from the repo root, NOT this file. Bumped to v0.20.26 by the VRO-9 release
  to sync the previously-stale `v0.1.0` URLs and reflect VRO capabilities.

## Local Contracts

- The installers download from `github.com/99percentgrip/agent-vesper`
  releases for the matching tag (`v<version>`, or `latest`).
- SHA-256 verification is mandatory; a missing/mismatched checksum fails the
  install.
- Installed launchers invoke their bundled binary verbatim and add no behavior.
- Credentials are not stored by the installer. First-run guidance leads with
  the TUI's Agent Vesper Authentication screen and also documents `agent-vesper-acp --setup` and
  the optional `ZAI_API_KEY` environment override.
- Uninstallers never remove provider credentials. OS-keyring entries and the
  private-vault fallback are outside the installer-owned artifact set.
- The installers call both binaries with `--version` to confirm success.

## Verification

- Shellcheck-clean `install.sh` (POSIX `sh`, no bashisms).
- Shellcheck-clean `uninstall.sh` (POSIX `sh`, no bashisms).
- `install.ps1` runs under Windows PowerShell 5.1+ and PowerShell 7.
- `uninstall.ps1` runs under Windows PowerShell 5.1+ and PowerShell 7.
- After install, both `--version` commands print the workspace version and both
  binaries are reachable on `PATH`.

## Child DOX Index

No children.
