# Using Vesper

[Documentation](README.md) · [Installation](installation.md) · [Zed setup](zed.md)

## Start with a concrete task

Launch `agent-vesper-tui` from your project directory. After authentication, the
welcome screen shows your selected provider, model, and installed version. Use
↑/↓ and Enter, or click a row, to **Start coding**, open **Settings**, or **Check
for updates**. Settings uses a centered menu; select a category and then a value
with Enter. Applying a value keeps you in Settings. Esc goes back from values to
Settings, then to the welcome screen (or your conversation if opened while coding). The update check contacts GitHub only when selected; it does not
install anything. Press R for the release page or Esc to quit. Resuming with
`--resume <SESSION_ID>` goes directly to your conversation.

Ask Vesper to inspect the repository before making a change:

```text
Read the README and test configuration. Explain how this project is structured
and which tests cover authentication. Do not modify files yet.
```

Then give it a scoped implementation task:

```text
Add validation for empty project names. Follow the existing conventions,
add a regression test, and run the relevant tests. Report anything unverified.
```

The conversation shows tool activity and results. Review the plan for multi-step work, respond to permission prompts, and inspect the resulting diff. Automatic continuation is bounded; an interrupted or exhausted run can leave work unfinished.

## Settings and providers

The selected visual theme also colors the welcome screen, Settings, and Web tools.
Changing it in Settings → Visual theme applies immediately and is retained for the
next launch, including when you open another project. Web tools uses the same centered menu, with
draft toggles and explicit Save settings / Cancel actions.

Use `/settings` for provider selection and optional feature setup. Provider controls come from the selected adapter: model, reasoning, and account options can differ. Save changes explicitly and restart when the screen asks you to.

- **Z.ai:** authenticate with a key for your selected endpoint/plan.
- **OpenAI:** choose API-key or eligible ChatGPT subscription authentication. See [OpenAI setup](openai-provider.md).
- **LM Studio:** run its server, load a model, and configure the address and model through provider settings.

`/usage` reports the current model, reasoning, permissions, estimated context, and provider-reported account windows when available. Missing account information is shown as unavailable. Using a local harness does not make cloud-provider requests local; the selected provider receives the context needed for those requests.

## Useful commands

Type `/` to explore available commands or `/help` for the installed version's reference. Some terminal-only controls differ from editor controls.

| Command | Use it to |
|---|---|
| `/settings` | Configure providers and optional features. |
| `/usage` | Inspect model, context, and available account usage. |
| `/remember [--global\|--project] <text>` | Save a preference or project fact. |
| `/recall <query>` | Find relevant saved memories. |
| `/memories [query]` | Inspect memories and their IDs. |
| `/forget [--global\|--project] <id>` | Delete a saved memory. |
| `/promote <id>` / `/demote <id>` | Move a memory between project and global scope. |
| `/skills` | Browse available skills. |
| `/skill <name\|bundle:name> [task]` | Start an explicit skill workflow. |
| `/compact [focus]` | Summarize older working context with an optional focus. |
| `/acceptance start <PRD>` | Enroll a requirements document for completion verification. |
| `/acceptance status` | See remaining verification gaps. |
| `/mcp` / `/plugins` | Inspect and manage native MCP or signed-plugin integrations. |

Provider model and reasoning selections are best made through their native controls; available values depend on the adapter and your account.

## Memory you can inspect

Vesper separates project knowledge from global preferences. `/remember` chooses a scope and reports it; explicit flags let you choose:

```text
/remember --global I prefer concise explanations and conventional commits.
/remember --project Run npm test before changing the API handlers.
/recall test conventions
/memories
```

Use the displayed IDs to forget or move records. A move returns the destination ID. Project memory normally lives in `.agent-vesper/cognition`; global memory uses Vesper's platform data directory. [Installation paths and uninstall behavior](installation.md#install-locations) explain what to retain when moving machines or removing the app.

Recall can use semantic embeddings and keyword search. With no suitable embedding service, the local fallback relies on lexical similarity; do not expect the same semantic recall as a neural embedding model. `/embedding` shows the active configuration. Embedding configuration is independent of the chat provider when explicitly set, and changing the embedding model can require re-embedding stored memories.

## Verify work against requirements

For a feature with a written requirements document, enable **Settings → Implementation acceptance**, choose the document, and Save. Or start one objective:

```text
/acceptance start docs/my-feature-prd.md
```

Vesper keeps the requirements separate from the agent's plan, reviews proposed checks, executes admitted Rust tests, and generates an evidence-based completion report. Missing, failing, stale, or inconclusive checks remain visible. An agent's claim that it is done does not itself satisfy the gate.

Use `/acceptance status` to inspect gaps and `/acceptance resume` to continue. `/acceptance revise <PRD>` changes scope explicitly while keeping prior scope in lineage. `/acceptance export <new-file>` saves an audit bundle; `/acceptance resume <audit-file>` restores scope with fresh verification. `/acceptance stop` stops enforcement without declaring success.

This applies to enrolled objectives. The initial collector runs exact Rust tests and needs a Rust toolchain and available dependencies. Independent review can miss semantic gaps; a verified report is scoped evidence, not a guarantee of defect-free software. [Design and verification boundaries](adr/0028-native-implementation-acceptance.md).

## Reasoning and workers

Reasoning orchestration can spend additional model calls on decomposition, alternative solutions, verification, or repair. To select a session mode:

```text
/reasoning set mode=balanced
```

Modes are `auto`, `fast`, `balanced`, `deep`, `maximum`, and `off`. `/reasoning clear` removes the override. These are Vesper workflow modes; `/thinking` controls the selected model's own reasoning options. More involved workflows can consume more time and provider allowance.

Multi-worker orchestration is opt-in through native Swarm settings and `/swarm` controls. It requires configured embeddings and an available permitted sandbox backend. The release includes the worker-capable binaries; enabling the feature does not grant tool or network permission. Use the settings/status controls to resolve missing prerequisites before starting work.

## Web and visual review

Enable web access through **Settings → Web tools** after installing Docker or Podman. Fetching, rendering, and browser interaction have separate choices. [Web setup](web-tools.md) covers the bundled driver and supported operations.

For local HTML work, VesperLens can open a browser review with annotations and feedback. You can request a review explicitly, for example: “Open this HTML page for review so I can mark changes.” Planning interviews can also collect structured answers. `/interview-limit` controls the number of planning questions. A local HTML review is separate from enabling internet browser tools.

## Skills and integrations

The installer seeds a shared skill library. Project skills can override shared skills with the same name. Upgrades add new seeds while preserving existing edits and recorded deletions.

Native MCP tools and signed declarative plugins extend workflows subject to permission checks. Configure MCP integrations in Vesper; do not assume editor-provided MCP servers are automatically executed by the harness. See `/mcp`, `/plugins`, and the installed command reference.

## Sessions and checkpoints

The terminal app supports saved-session resume with `agent-vesper-tui --resume <SESSION_ID>`. Editor chat persistence needs both session read and write flags; see [Zed setup](zed.md#keep-chat-history).

Checkpoints and lineage are a separate feature. In ACP they are off by default and require an explicit checkpoint root or `AGENT_VESPER_ENABLE_CHECKPOINTS=1`. Ordinary editor chats should not create a project checkpoint directory merely by connecting.
