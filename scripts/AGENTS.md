# Installation scripts

## Purpose

Own the cross-platform installers and uninstallers that download a compiled
`agent-vesper-acp` release archive, verify its SHA-256, install or remove the
binary on the user's PATH, and produce the same first-run UX as the original
Python `native-glm-acp` installer.

## Ownership

- `install.sh` — POSIX installer (Linux + macOS). Downloads the
  `agent-vesper-acp-<platform>-<arch>.tar.gz` release, verifies SHA-256,
  installs a launcher under `$XDG_BIN_HOME` (default `~/.local/bin`), and
  updates the shell profile PATH.
- `install.ps1` — Windows installer. Downloads the
  `agent-vesper-acp-windows-x86_64.zip` release, verifies SHA-256, installs a
  `.cmd` launcher under `%LOCALAPPDATA%\Programs\AgentVesper`, and adds that
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
- The installed launcher invokes the bundled `agent-vesper-acp` binary
  verbatim — it adds no behavior of its own.
- Credentials are NOT stored by the installer; Agent Vesper resolves Z.ai
  credentials from the `ZAI_API_KEY` environment variable (matching its
  no-filesystem-I/O composition contract). The installer prints the
  `ZAI_API_KEY` setup hint instead of running a `--setup` credential store.
- Uninstallers never remove provider credentials. Agent Vesper's environment
  credential is outside the installer-owned artifact set.
- The installers call `agent-vesper-acp --version` to confirm success, so the
  binary MUST implement that flag.

## Verification

- Shellcheck-clean `install.sh` (POSIX `sh`, no bashisms).
- Shellcheck-clean `uninstall.sh` (POSIX `sh`, no bashisms).
- `install.ps1` runs under Windows PowerShell 5.1+ and PowerShell 7.
- `uninstall.ps1` runs under Windows PowerShell 5.1+ and PowerShell 7.
- After install, `agent-vesper-acp --version` prints the workspace version
  and the binary is reachable on `PATH`.

## Child DOX Index

No children.
