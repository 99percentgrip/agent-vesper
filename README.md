# Agent Vesper

Agent Vesper is an active migration of the completed Native GLM ACP harness into
a Rust-native, provider-neutral agent architecture. The frozen Python project at
commit `bf4d4287e2e3320aa3f09015f678e6169d520045` is the behavioral reference.

This repository contains the Rust-native Agent Vesper ACP harness, the Z.ai GLM
provider, the session/runtime engine, the agent/tool loop, and the interactive
TUI. The frozen Python project remains the behavioral reference for the native
GLM command surface. Optional browser, web, vision, and live ACP permission
integrations are implemented as explicit runtime capabilities rather than
claimed as part of the provider-neutral core.

All seven Stage 4.1 process-level blockers pass locally. The session writer,
bounded search, memory, checkpoints, MCP/plugin, observability, agent-loop,
permission, and TUI command surfaces are covered by the local verification
gate. SQLite remains intentionally excluded. The five-target host matrix is
validated by GitHub Actions rather than local execution.

Stage 11b adds the `agent-vesper-tui` interactive frontend: a pure 4-phase
Plan Mode state machine (NORMAL → PLANNING → REVIEW → EXECUTING), a
provider-superpowers discovery layer, and a `ratatui`/`crossterm` event loop.
ADR 0009 reconciles the GLM reasoning surface with the Python oracle into a
single session-scoped `/thinking` dial (`{disabled, enabled, high, max}`) and
threads a session reasoning override through the runtime into the GLM wire
`reasoning_effort`. `/effort` is retired. Model-driven planning and bounded
tool execution are available through the native harness capabilities.

## Install

Agent Vesper ships two binaries: `agent-vesper-acp` (the ACP-protocol-v1 stdio
server an editor drives) and `agent-vesper-tui` (the interactive terminal UI).
The `registry/agent.json` manifest and the `scripts/install.sh` /
`scripts/install.ps1` installers mirror the original Python `native-glm-acp`
distribution so the compiled Rust binary registers as an ACP agent exactly
like the source of truth.

### macOS / Linux

```sh
curl -fsSL https://github.com/99percentgrip/agent-vesper/raw/main/scripts/install.sh | sh
```

Or pin a version:

```sh
AGENT_VESPER_VERSION=0.2.0 sh scripts/install.sh
```

The installers install the ACP server used by Zed. The TUI is currently built
from source with `cargo build --release -p agent-vesper-tui`.

Then set your Z.ai credential (Agent Vesper resolves it from the environment;
it keeps no on-disk credential store):

```sh
export ZAI_API_KEY="<your Z.ai key>"   # https://z.ai/
agent-vesper-acp --version             # verify
```

To uninstall the ACP binary, bundle, and PATH entry created by the installer:

```sh
curl -fsSL https://github.com/99percentgrip/agent-vesper/raw/main/scripts/uninstall.sh | sh
```

The uninstaller preserves `ZAI_API_KEY` and any other provider-owned
credentials. Custom locations can be selected with the same
`AGENT_VESPER_INSTALL_DIR`, `AGENT_VESPER_BUNDLE_DIR`, and
`AGENT_VESPER_SHELL_PROFILE` variables used by the installer.

### Windows (PowerShell)

```powershell
irm https://github.com/99percentgrip/agent-vesper/raw/main/scripts/install.ps1 | iex
```

Uninstall with:

```powershell
irm https://github.com/99percentgrip/agent-vesper/raw/main/scripts/uninstall.ps1 | iex
```

Both installers and uninstallers are reversible for their own artifacts, like
the Native GLM ACP distribution. Agent Vesper does not currently install the
TUI as a release artifact, and it intentionally does not offer a credential
purge flag because credentials are environment-owned rather than installer-
owned.

### Install in Zed

Once `agent-vesper-acp` is on `PATH` and `ZAI_API_KEY` is exported, add it as
a custom agent in Zed's `settings.json`:

```json
{
  "agent_servers": {
    "agent-vesper": {
      "command": "agent-vesper-acp",
      "env": { "ZAI_API_KEY": "<your Z.ai key>" }
    }
  }
}
```

Restart Zed, then open the Agent Panel and select **Agent Vesper**. The binary
speaks ACP protocol v1 over stdio; no child process or filesystem I/O is
performed by the server itself.

> The ACP Registry entry (`registry/agent.json`) is the publish path for
> one-click discovery. Pushing a matching `v<version>` tag runs the release
> workflow, builds the five target archives, emits SHA-256 files, and publishes
> the GitHub Release assets consumed by the manifest and installers.

## Local verification

The pinned development toolchain is installed automatically by Rustup.

```bash
cargo xtask verify
cargo xtask msrv
cargo xtask fixtures coverage --stage 2
cargo xtask fixtures coverage --stage 3
cargo xtask fixtures coverage --stage 4
cargo xtask fixtures coverage --stage 5
cargo xtask contracts verify
cargo xtask provider glm verify
cargo xtask runtime verify
cargo xtask acp verify
cargo xtask sessions verify
```

The first command runs formatting, Clippy, tests, fixture validation/index
verification, and architecture rules. The MSRV command requires the Rust 1.88.0
toolchain. See [migration status](docs/migration-status.md), [architecture](docs/architecture.md),
and [contributing](CONTRIBUTING.md).

No runtime performance claim is made at this stage.
