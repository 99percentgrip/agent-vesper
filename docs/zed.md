# Use Vesper in Zed

[Documentation](README.md) · [Installation](installation.md) · [Using Vesper](using-vesper.md)

Vesper's `agent-vesper-acp` executable connects to editors using ACP. Install Vesper first and complete provider authentication in the terminal app.

## Add the custom agent

Until [the registry submission](https://github.com/agentclientprotocol/registry/pull/539) is merged, use a custom agent entry. In Zed, open Agent Settings → External Agents → Add Agent → Add Custom Agent. Zed opens the settings file; merge this entry into your existing `agent_servers` object. This follows [Zed's custom-agent setup](https://zed.dev/docs/ai/external-agents#custom-agents).

```json
{
  "agent_servers": {
    "agent-vesper": {
      "type": "custom",
      "command": "agent-vesper-acp",
      "args": [],
      "env": {
        "AGENT_VESPER_ENABLE_SESSION_READS": "1",
        "AGENT_VESPER_ENABLE_SESSION_WRITES": "1"
      }
    }
  }
}
```

Select Agent Vesper when creating a thread in the Agent Panel. If the editor cannot find the command, use an absolute launcher path from the [installation guide](installation.md#install-locations). Reopen the editor after an installation or update if its process has an old PATH.

On Windows, you can point `command` directly at the installed executable, for example `C:\Users\YourName\AppData\Local\Programs\AgentVesper\agent-vesper-acp.bundle\agent-vesper-acp.exe`. Escape backslashes as `\\` inside JSON strings.

## Choose your provider

Authenticate in Vesper first; credentials configured for Zed's own models are not automatically Vesper credentials. The agent's provider/model controls select the active adapter. Available options depend on that provider.

For an explicit OpenAI launch, change `args` to:

```json
["--provider", "openai"]
```

For LM Studio, use `["--provider", "lmstudio"]` and configure its running server in Vesper's native provider settings. For Z.ai, the default launch uses the selected stored credential, or a `ZAI_API_KEY` environment variable supplied to the process. Avoid putting a real key into a settings file you share or commit.

[OpenAI authentication instructions](openai-provider.md#acp--editor-usage) cover subscription sign-in and API-key setup.

## Keep chat history

The example enables both flags:

- `AGENT_VESPER_ENABLE_SESSION_WRITES=1` saves completed turns.
- `AGENT_VESPER_ENABLE_SESSION_READS=1` lets later agent processes list, load, and resume saved sessions.

Both are required for durable editor chats. On Linux, the default session store is `$XDG_DATA_HOME/agent-vesper/sessions`, normally `~/.local/share/agent-vesper/sessions`. Workspace checkpoints are separate and remain opt-in; they are not needed for chat persistence.

## Everyday use

Type `/` for Vesper's advertised commands. `/usage` reports the active model and available account information. Plans and tool progress appear through the editor's native agent interface. Host-specific terminal controls do not become editor UI panels.

To enroll a requirements document from ACP, use `/acceptance start <PRD>` or persist activation with `/settings acceptance on <PRD>`. Web controls use `/web`; see [web tools](web-tools.md).

Vesper owns its native MCP configuration. Do not assume a server configured only in Zed is automatically available to Vesper.

## Troubleshooting

If startup fails, first run `agent-vesper-acp --version` from a terminal. Check the executable path, selected provider, and authentication. In Zed, `dev: open acp logs` exposes protocol diagnostics; see [Zed's debugging guidance](https://zed.dev/docs/ai/external-agents#debugging). Remove secrets and private content before sharing logs.
