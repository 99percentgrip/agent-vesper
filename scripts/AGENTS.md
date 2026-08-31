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
- `publish_to_acp_registry.sh` — VRO-11.2 publishing helper for the public
  ACP Registry (`agentclientprotocol/registry`). Clones the registry to a
  temporary directory, writes `registry/agent.json` from this repo into the
  `agent-vesper/` directory, reuses the branch tracked by the open PR, and
  updates that Pull Request in place. CONTINUOUS-UPDATE CONTRACT
  (maintainer-mandated 2026-08-18):
  there is exactly ONE long-lived open `agent-vesper` PR. Never close it to
  open a new one; never comment that it is superseded. For every version
  bump: PUT the updated `agent.json` to the SAME branch the open PR tracks
  (force-push if rebased), then edit the PR title/body in place via
  `gh api repos/agentclientprotocol/registry/pulls/<N> -X PATCH`. The local installers
  (`install.sh` / `install.ps1`) consume `registry/agent.json` from this
  repo, NOT this script. Replaces the deleted `acp_pr_439.md` payload
  (VRO-11.2: that file was stale — it claimed to be the payload for PR
  #439, but PR #439 is the `native-glm-acp` Python project's PR and is
  intentionally left untouched; `agent-vesper` is a separate agent entry).

## Local Contracts

- The installers download from `github.com/99percentgrip/agent-vesper`
  releases for the matching tag (`v<version>`, or `latest`).
- SHA-256 verification is mandatory; a missing/mismatched checksum fails the
  install.
- Installed launchers invoke their bundled binary verbatim and add no behavior.
- Until the public ACP Registry PR is merged, installer completion guidance
  directs Zed users to register the installed launcher with `type: custom`;
  it must not imply that the agent is already discoverable from the registry.
- Credentials are not stored by the installer. First-run guidance leads with
  the TUI's Agent Vesper Authentication screen and also documents `agent-vesper-acp --setup` and
  the optional `ZAI_API_KEY` environment override.
- Uninstallers never remove provider credentials. OS-keyring entries and the
  private-vault fallback are outside the installer-owned artifact set.
- The installers call both binaries with `--version` to confirm success.
- The installers seed the curated skill library (from the bundled
  `skills/` archive directory) into `~/.agent-vesper/memory/`
  non-destructively: existing files win, slugs listed in
  `.seed-manifest` are never resurrected, and new seed skills from later
  releases are added on upgrade. `AGENT_VESPER_MEMORY_ROOT` overrides the
  destination. Uninstallers never touch `~/.agent-vesper/` (user data).

## Verification

- Shellcheck-clean `install.sh` (POSIX `sh`, no bashisms).
- Shellcheck-clean `uninstall.sh` (POSIX `sh`, no bashisms).
- `install.ps1` runs under Windows PowerShell 5.1+ and PowerShell 7.
- `uninstall.ps1` runs under Windows PowerShell 5.1+ and PowerShell 7.
- After install, both `--version` commands print the workspace version and both
  binaries are reachable on `PATH`.
- Installer completion output identifies custom Zed registration as the
  pre-registry path.

## Child DOX Index

No children.
