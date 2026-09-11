# Documentation

[← Agent Vesper](../README.md)

Choose a guide by what you want to do. Installation and everyday use come first; architecture, specifications, and historical evidence are separate below.

## For users

| Guide | Covers |
|---|---|
| [Installation](installation.md) | Supported systems, prerequisites, dependency setup, upgrades, paths, troubleshooting, and uninstalling. |
| [Using Vesper](using-vesper.md) | First tasks, settings, memory, skills, reasoning, requirement verification, and useful commands. |
| [Zed integration](zed.md) | Connect the ACP server, select a provider, and keep editor chat history. |
| [OpenAI](openai-provider.md) | API-key and ChatGPT subscription authentication, model controls, usage, and account limits. |
| [Web tools](web-tools.md) | Enable fetching and browser interaction, repair the bundled driver, and understand supported operations. |

For changes between versions, read the [release notes](https://github.com/99percentgrip/agent-vesper/releases). For a problem, [open an issue](https://github.com/99percentgrip/agent-vesper/issues) with your operating system, Vesper version, and steps to reproduce it. Remove credentials and private content from logs.

## For contributors

| Reference | Purpose |
|---|---|
| [Contributing](../CONTRIBUTING.md) | Work boundaries, verification, and pull requests. |
| [Repository instructions](../AGENTS.md) | Project contracts and the documentation ownership hierarchy. |
| [Architecture](architecture.md) | Runtime design and component boundaries. |
| [Workspace map](workspace-map.md) | Crates and application composition. |
| [Dependencies](dependencies.md) | Engineering dependency inventory; user prerequisites are in the installation guide. |
| [Current implementation status](migration-status.md) | Implemented scope, evidence, and remaining limitations. |
| [Architecture decisions](adr/) | Accepted design decisions and their verification obligations. |

## Specifications and engineering evidence

These describe requirements, design decisions, and past implementation work. A specification or historical completion report is not a promise that every described feature is available in your installed version. Use the current status and release notes above.

- [Reasoning orchestration](agent-vesper-reasoning-orchestrator-prd.md)
- [Provider capabilities](provider-capability-gating-prd.md)
- [Web tools](web-oracle-extraction-prd.md)
- [Multi-worker orchestration](swarm-oracle-extraction-prd.md)
- [Completion assurance decision](adr/0028-native-implementation-acceptance.md) and [implementation evidence](foundation/completion-assurance-execution.md)
- [Foundation evidence index](foundation/evidence-index.md)

Earlier migration records remain in [Stage 1](stage1/), [Stage 2](stage2/), [Stage 3](stage3/), [Stage 4](stage4/), and [Stage 5](stage5/). They are historical engineering references, not getting-started guides.
