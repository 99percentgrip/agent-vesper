# Install and manage Vesper

[Documentation](README.md) · [Using Vesper](using-vesper.md) · [Zed setup](zed.md)

## Before you install

Release packages support Linux x86_64 and ARM64, macOS Intel and Apple Silicon, and Windows x86_64. Linux packages target GNU/Linux; Alpine/musl and Windows ARM64 are not native release targets.

For the prebuilt app you need:

- A terminal and internet access to download the release.
- On Linux/macOS: `curl`, `tar`, and either `sha256sum` or `shasum`.
- On Windows: PowerShell with `Invoke-WebRequest`, `Expand-Archive`, and `Get-FileHash`.
- Access to a supported provider: Z.ai, OpenAI, or a running LM Studio server.

You do not need a Rust compiler for the prebuilt app. Your project still needs its own development tools. Verification that executes Cargo tests requires a Rust toolchain.

## Install

### Linux and macOS

```sh
curl -fsSL https://raw.githubusercontent.com/99percentgrip/agent-vesper/main/scripts/install.sh | sh
```

If you prefer to inspect the script before running it:

```sh
curl -fsSL https://raw.githubusercontent.com/99percentgrip/agent-vesper/main/scripts/install.sh -o vesper-install.sh
# Read vesper-install.sh, then:
sh vesper-install.sh
```

A minimal Debian/Ubuntu system can install the download tools with:

```sh
sudo apt-get update
sudo apt-get install curl ca-certificates tar coreutils
```

Fedora:

```sh
sudo dnf install curl ca-certificates tar coreutils
```

### Windows PowerShell

```powershell
irm https://raw.githubusercontent.com/99percentgrip/agent-vesper/main/scripts/install.ps1 | iex
```

To inspect it first:

```powershell
Invoke-WebRequest https://raw.githubusercontent.com/99percentgrip/agent-vesper/main/scripts/install.ps1 -OutFile vesper-install.ps1
# Read vesper-install.ps1, then:
./vesper-install.ps1
```

Use your organization's PowerShell execution policy if script execution is restricted.

### Verify and launch

Open a new terminal if necessary, then run:

```sh
agent-vesper-acp --version
agent-vesper-tui --version
cd path/to/your-project
agent-vesper-tui
```

Both commands should report the installed release version. Complete the authentication screen and use `/settings` → Providers to change providers. Follow any restart prompt. [OpenAI authentication](openai-provider.md) has separate instructions for API keys and subscription sign-in.

The package includes the terminal app, ACP server, sandbox supervisor, web-fetch helper, browser-driver image, and seed skills. The installer verifies the archive checksum and attempts to import the driver. A missing container engine does not prevent ordinary coding use; set it up later from Settings.

## Optional dependencies

### Local models

Install [LM Studio](https://lmstudio.ai/docs/app), download and load a model suitable for coding and tool use, and start its local server. In Vesper's provider settings, select LM Studio and configure the server address and model. Model download, hardware requirements, and supported capabilities depend on the model you choose.

### Web tools and container workers

Starting with v0.23.0, open **Settings → Web tools → Set up features / repair**.
Setup needs the complete browser bundle; compiling the executable alone does
not include the browser archive. Release packaging supplies it automatically.
Confirm setup and approve any operating-system authorization prompt. Vesper reuses
an available local Docker/Podman engine, or offers Podman installation. It then
imports the bundled image and checks contained browser execution and cleanup.
Save your web choices separately and restart the host when prompted.

Automatic package plans cover Debian/Ubuntu and Fedora (with a graphical `pkexec`
authorization agent), official Podman packages on macOS, and Windows x86_64.
macOS/Windows need a Linux VM; a new Vesper machine uses 2 CPUs, 2 GiB RAM and
up to 20 GiB disk. Downloads can exceed 1 GiB. Windows may require OS virtualization
approval and a restart; reopen Vesper and repeat setup afterward. Existing machines
are never reset. A previously verified Vesper machine can start again when needed.

The guided flow is a **preview**. Its real Linux contained-browser check has
passed; clean-machine package installation and macOS/Windows acceptance remain
pending. See the [acceptance record](foundation/dependency-setup-execution.md).
Older builds use **Set up / repair driver** after installing and starting
[Docker](https://docs.docker.com/get-started/get-docker/) or
[Podman](https://podman.io/docs/installation) through the official OS instructions.

Esc requests a stop after the current setup transaction. Installed packages/imports
remain available for Retry; unsaved web toggles stay unsaved. Core coding remains
available when setup is declined or blocked by device policy. Voice, local models,
embeddings and project build tools have separate requirements.

Web access and browser interaction are opt-in. See [web tools](web-tools.md) for permissions, settings, and supported operations. Multi-worker execution additionally needs configured embeddings and an available permitted sandbox backend; see [using workers](using-vesper.md#reasoning-and-workers).

### Voice input

Voice input is optional and supported in the Linux/macOS terminal app. It needs a microphone and a recording command: `arecord` on Linux or `afrecord` on macOS. On Debian/Ubuntu install `alsa-utils`; on Fedora install `alsa-utils` with `dnf`.

Starting with v0.23.0, click the red **● Push to talk** control (or press **F5**)
to start recording. Click **■ Stop** or press F5 again to transcribe. Recording
continues until Stop, with elapsed time shown; available disk and recorder limits
still apply. Transcription shows progress and keeps the composer editable.
Press F5 while preparing/transcribing to cancel. Failed or cancelled transcription
retains private audio for **Retry voice** or **Discard** (Delete); it is removed
on success, discard or normal app exit. Completed dictation is appended to the
composer for review and is never sent automatically.

If no suitable transcription environment exists, first use attempts to install `faster-whisper` into Vesper's voice environment and download the chosen model. This needs network access and can take time. The POSIX installer bundles `uv` when its download succeeds; a system `uv` or Python virtual-environment setup is the fallback.

If recording is unavailable, confirm the recording command is on PATH and the terminal has microphone access. Voice setup is separate from ordinary text-based coding.

## Update or choose a version

From the welcome screen, choose **Check for updates**. If a newer release is available, choose **Install update** to run the checksum-verifying installer. Linux/macOS show progress in Vesper; Windows opens an installer console after Vesper closes so its executables can be replaced. Reopen Vesper and restart any editor agent process afterward. Declining the offer changes nothing.

You can also rerun the same installer to update to the latest release, then restart Vesper and any editor agent process. Upgrades replace application payloads while preserving co-located user state and existing seed-skill edits.

To select a specific release, download the installer as shown above, then use an actual version from [Releases](https://github.com/99percentgrip/agent-vesper/releases):

```sh
AGENT_VESPER_VERSION=0.23.0 sh vesper-install.sh
```

```powershell
./vesper-install.ps1 -Version 0.23.0
```

The version above is an example, not an instruction to downgrade a newer installation.

## Install locations

| Item | Default location |
|---|---|
| Linux/macOS launchers | `~/.local/bin/agent-vesper-tui` and `~/.local/bin/agent-vesper-acp` |
| Linux/macOS bundle | `$XDG_DATA_HOME/agent-vesper`, normally `~/.local/share/agent-vesper` |
| Windows launchers | `%LOCALAPPDATA%\Programs\AgentVesper` |
| Windows bundle | `agent-vesper-acp.bundle` inside the launcher directory |
| Seeded skill library | `~/.agent-vesper/memory` |
| Project memory | `.agent-vesper/cognition` inside the project |

Linux/macOS installer overrides: `AGENT_VESPER_INSTALL_DIR`, `AGENT_VESPER_BUNDLE_DIR`, and `AGENT_VESPER_SHELL_PROFILE`. Windows accepts `-InstallDir` or `AGENT_VESPER_INSTALL_DIR`. Use the same custom paths when uninstalling.

## Troubleshooting

| Symptom | What to check |
|---|---|
| Command not found | Reopen the terminal. Check the launcher directory above is on PATH. A desktop editor may need an absolute executable path. |
| Wrong version starts | Run `command -v agent-vesper-tui` on Linux/macOS or `Get-Command agent-vesper-tui` in PowerShell. Check for an older installation earlier on PATH. |
| Checksum verification fails | Stop and download again from the official release. Do not bypass the checksum. |
| Web driver unavailable | Start Docker/Podman, confirm `info` succeeds, then use Settings → Web tools → Set up / repair driver. |
| Provider rejects a request | Check authentication, model entitlement, account limits, and the provider selected in Settings. |
| LM Studio cannot connect | Confirm its server is running and the address in Vesper matches it. |

## Uninstall

**Back up first.** The uninstaller removes the launchers and the entire application bundle. On Linux/macOS, that bundle shares the default data directory with global cognition and voice state; those files are removed too. Copy any data you want to retain outside the bundle before uninstalling. Custom storage paths can change what is inside it.

Linux/macOS:

```sh
curl -fsSL https://raw.githubusercontent.com/99percentgrip/agent-vesper/main/scripts/uninstall.sh | sh
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/99percentgrip/agent-vesper/main/scripts/uninstall.ps1 | iex
```

Provider credentials are preserved. Project directories and the seeded `~/.agent-vesper/memory` library outside the bundle are not removed. The scripts do not remove imported container images or externally installed dependencies. Remove the custom agent entry from your editor separately. Inspect [the uninstall scripts](../scripts/) if you use custom paths.
