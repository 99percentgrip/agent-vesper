<div align="center">

# Agent Vesper

**An AI coding agent for your terminal and editor. Built in Rust.**

[![Release](https://img.shields.io/github/v/release/99percentgrip/agent-vesper)](https://github.com/99percentgrip/agent-vesper/releases/latest)
[![CI](https://github.com/99percentgrip/agent-vesper/actions/workflows/ci.yml/badge.svg)](https://github.com/99percentgrip/agent-vesper/actions/workflows/ci.yml)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)

[Get started](#install) · [Capabilities](#what-you-can-do) · [Documentation](docs/README.md) · [Releases](https://github.com/99percentgrip/agent-vesper/releases)

</div>

Agent Vesper reads your codebase, edits files, runs commands and tests, and remembers project context across conversations. Work in its native terminal interface or connect it to an editor through the Agent Client Protocol (ACP).

Choose **Z.ai, OpenAI, or local models through LM Studio**. Vesper runs on your machine; prompts and relevant context go to the model provider you select. Local model inference requires a running LM Studio server.

## What you can do

| Capability | What it means for your work |
|---|---|
| **Work on real code** | Explore repositories, make changes, run tests, and review results with tool-permission controls. |
| **Plan and track tasks** | Review a plan and follow progress in the terminal or your editor. Unfinished plans trigger bounded continuation. |
| **Keep project knowledge** | Save preferences and project facts, search memory, and control what belongs to one project or follows you across projects. |
| **Check completion against requirements** | Enroll a requirements document and require current verification evidence before the harness reports completion. Missing or failing checks stay visible. |
| **Review visual work** | Use VesperLens to review local HTML artifacts in a browser and return annotations or answers to planning questions. |
| **Use the web when needed** | Enable contained fetching, scraping, crawling, and browser interaction from Settings. Requires Docker or Podman. |
| **Extend your workflow** | Use bundled skills, connect MCP tools, and enable reasoning or multi-worker workflows when a task needs them. |

[Explore the user guide →](docs/using-vesper.md)

## Install

Prebuilt packages include **both the terminal app and the ACP editor server**. You do not need Rust or Python for ordinary coding tasks.

**Supported:** Linux x86_64 / ARM64, macOS Intel / Apple Silicon, and Windows x86_64.

### macOS and Linux

Run in your terminal:

```sh
curl -fsSL https://raw.githubusercontent.com/99percentgrip/agent-vesper/main/scripts/install.sh | sh
```

### Windows

Run in PowerShell:

```powershell
irm https://raw.githubusercontent.com/99percentgrip/agent-vesper/main/scripts/install.ps1 | iex
```

The installer downloads the latest release and verifies its SHA-256 checksum. Reopen your terminal if the commands are not yet on PATH.

[Inspect the installers](scripts/) · [Manual download](https://github.com/99percentgrip/agent-vesper/releases/latest) · [Installation and troubleshooting](docs/installation.md)

## First run

From the project you want to work on:

```sh
cd path/to/your-project
agent-vesper-tui
```

Complete the authentication screen. Use **`/settings` → Providers** to select or change your provider, and restart when prompted.

| Provider | What you need |
|---|---|
| **Z.ai** | A Z.ai API key and access to the selected model/plan. |
| **OpenAI** | An API key or an eligible ChatGPT subscription sign-in. [OpenAI setup](docs/openai-provider.md). |
| **LM Studio** | A running LM Studio server with a loaded model. Configure its address and model in Vesper's provider settings. |

Try a concrete first task:

> Read this repository and explain how to run its tests. Do not change any files yet.

Then ask for a change, review tool approvals, and inspect the result. Use `/help` for commands and `/usage` for the active model, context estimate, and available account usage information.

### Install in Zed

Use the included `agent-vesper-acp` server as a custom external agent. Follow the [Zed setup guide](docs/zed.md) for configuration and persistent chat history.

## Dependencies

Start with the app and your chosen provider. Add dependencies only for the features you use:

- **Project tooling:** Git, compilers, package managers, and test runners needed by your repository.
- **Web tools and container workers:** Docker or Podman, running with Linux containers. The browser-driver image is included in Vesper's package.
- **Voice input:** Optional Linux/macOS microphone and transcription setup.

[Dependency installation and feature setup →](docs/installation.md#optional-dependencies)

## Update or uninstall

**Update:** rerun the installer, then restart Vesper. Upgrades preserve co-located user state and your existing skill edits.

**Uninstall:** back up data stored inside the application bundle first, including global memory at its default Linux/macOS location. The uninstaller removes that directory; provider credentials are preserved. [Paths and backup details](docs/installation.md#uninstall).

macOS / Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/99percentgrip/agent-vesper/main/scripts/uninstall.sh | sh
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/99percentgrip/agent-vesper/main/scripts/uninstall.ps1 | iex
```

## Documentation

| Start here | Learn more |
|---|---|
| [Install, update, dependencies, uninstall](docs/installation.md) | [Web tools and browser setup](docs/web-tools.md) |
| [Using Vesper: memory, commands, verification](docs/using-vesper.md) | [OpenAI authentication and usage](docs/openai-provider.md) |
| [Connect to Zed](docs/zed.md) | [Architecture and engineering documentation](docs/README.md#for-contributors) |

[Browse all documentation →](docs/README.md)

## Contribute

Found a bug or have a feature request? [Open an issue](https://github.com/99percentgrip/agent-vesper/issues). Code and documentation contributions are welcome—start with [Contributing](CONTRIBUTING.md).

If Vesper is useful to you, a GitHub star helps others discover it.

[Apache-2.0 licensed](LICENSE).
