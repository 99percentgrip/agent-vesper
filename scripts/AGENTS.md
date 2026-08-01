# Installation scripts

## Purpose

Own the cross-platform installers and uninstallers that download one verified
release archive, install or remove both `agent-vesper-acp` and
`agent-vesper-tui` on the user's PATH, and produce the same first-run UX as the
original Python `native-glm-acp` installer.

## Ownership

- `install.sh` — POSIX installer (Linux + macOS). Downloads the
  `agent-vesper-acp-<platform>-<arch>.tar.gz` release, verifies SHA-256,
  installs both launchers under `$XDG_BIN_HOME` (default `~/.local/bin`), and
  updates the shell profile PATH.
- `install.ps1` — Windows installer. Downloads the
  `agent-vesper-acp-windows-x86_64.zip` release, verifies SHA-256, installs a
  `.cmd` launchers under `%LOCALAPPDATA%\Programs\AgentVesper`, and adds that
  directory to the user PATH.
- `uninstall.sh` — POSIX uninstaller. Removes only the launcher, bundle, and
  exact shell-profile PATH marker owned by `install.sh`.
- `uninstall.ps1` — Windows uninstaller. Removes only the launcher, bundle,
  and exact user PATH entry owned by `install.ps1`.

## Local Contracts

- The installers download from `github.com/99percentgrip/agent-vesper`
  releases for the matching tag (`v<version>`, or `latest`).
- SHA-256 verification is mandatory; a missing/mismatched checksum fails the
  install.
- Installed launchers invoke their bundled binary verbatim and add no behavior.
- Credentials are not stored by the installer. It documents both
  `ZAI_API_KEY` and the explicit `agent-vesper-acp --setup` private credential
  store; uninstall preserves either source.
- Uninstallers never remove provider credentials. Agent Vesper's environment
  credential is outside the installer-owned artifact set.
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
