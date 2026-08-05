<div align="center">

# Agent Vesper

### A Rust-native AI agent with persistent cognitive memory — provider-neutral by design.

[![CI](https://github.com/99percentgrip/agent-vesper/actions/workflows/ci.yml/badge.svg)](https://github.com/99percentgrip/agent-vesper/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/99percentgrip/agent-vesper)](https://github.com/99percentgrip/agent-vesper/releases)
[![MSRV](https://img.shields.io/badge/MSRV-1.88-orange.svg)](https://blog.rust-lang.org/2025/06/23/Rust-1.88.0.html)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Crates](https://img.shields.io/badge/workspace-19%20crates-FF6B35.svg)](#architecture)

**[Install](#install)** · **[Features](#features)** · **[Quick Start](#quick-start)** · **[Commands](#slash-commands)** · **[Architecture](#architecture)**

</div>

---

Agent Vesper is a **pure-Rust** AI agent harness that **remembers you across conversations**. It provides a provider-neutral runtime where any LLM provider can plug in — today it ships with the Z.ai GLM adapter, tomorrow any provider can join with zero TUI or runtime changes. The agent learns your preferences, tracks your projects, and recalls relevant context before every reply — all running locally with zero external services.

Built as a native port of the [Native GLM ACP](https://github.com/99percentgrip/Native-GLM-ACP) Python oracle with an advanced cognitive memory engine.

## Features

<table>
<tr><td><b>🧠 Cognitive Memory</b></td><td>The agent extracts facts from every conversation, stores them in a local SQLite database, and <b>silently recalls relevant memories before each reply</b>. You never repeat yourself. Type <code>/remember</code> to add a fact manually, <code>/recall</code> to search, <code>/forget</code> to delete.</td></tr>
<tr><td><b>📝 Rich Markdown TUI</b></td><td>Full-screen terminal UI with streaming reasoning traces, syntax-highlighted code blocks, full-width user-turn role banners, an interactive centered tool-permission modal (<code>Tab</code> / arrows to switch, <code>Enter</code> to confirm), mouse-wheel + PageUp/PageDown/Home/End scrolling with a visible scrollbar, and a live slash-command palette. Built on <code>ratatui</code> + <code>crossterm</code>.</td></tr>
<tr><td><b>🎯 Plan Mode</b></td><td>A pure 4-phase state machine (<code>NORMAL → PLANNING → REVIEW → EXECUTING</code>) that lets the model author a plan, you review it, then it executes with bounded tool calls.</td></tr>
<tr><td><b>🔧 87 Slash Commands</b></td><td>The complete Python oracle command surface — memory, skills, checkpoints, MCP, plugins, goals, awareness, sessions, export, CI status, and more.</td></tr>
<tr><td><b>🔐 Provider-Neutral Auth</b></td><td>Credentials route through the provider layer — never hardcoded. OS keyring with owner-only Unix vault fallback. <code>/auth</code> force-rotates without restart.</td></tr>
<tr><td><b>📦 Ed25519-Signed Plugins</b></td><td>Declarative plugin packages (permissions only — no executable code). Unsigned loading is structurally erased from <code>--release</code> builds via <code>#[cfg(debug_assertions)]</code>.</td></tr>
<tr><td><b>🔄 Session Lineage</b></td><td>Workspace snapshots, rollback, session branching, and a bounded cron/export/clipboard/CI surface — all RAII-safe with strict <code>Drop</code> file-handle discipline.</td></tr>
<tr><td><b>⚡ Hybrid Retrieval</b></td><td>Multi-signal scoring: <code>(semantic + BM25 + entity_boost) / max_possible</code>. Snowball lemmatization, FTS5 keyword search, entity-graph boosting with hyper-connection penalty.</td></tr>
<tr><td><b>📊 Priority + Heat Tracking</b></td><td>Every memory gets a type (<code>persona</code>/<code>episodic</code>/<code>instruction</code>), priority (0-100), and scene label. Frequently-recalled memories accumulate heat and float to the top.</td></tr>
<tr><td><b>🛡️ Secret-Safe</b></td><td>All error messages are sanitized. No file contents, API keys, paths, or memory text leak through <code>CognitionError</code>. <code>#![forbid(unsafe_code)]</code> enforced workspace-wide.</td></tr>
<tr><td><b>🎙️ Push-to-Talk Voice</b></td><td>Press <code>F5</code> to record from the microphone, <code>F5</code> again to transcribe speech-to-text straight into the composer (Linux + macOS). Auto-discovers any existing <code>faster-whisper</code> venv, or <b>self-bootstraps</b> a harness-owned one via the installer-bundled <code>uv</code> on first use — no separate Python setup. A long-lived sidecar loads the Whisper model once per session for instant subsequent transcriptions.</td></tr>
</table>

## Install

### macOS / Linux

```sh
curl -fsSL https://github.com/99percentgrip/agent-vesper/raw/main/scripts/install.sh | sh
```

Or pin a version:

```sh
AGENT_VESPER_VERSION=0.14.0 sh scripts/install.sh
```

### Windows (PowerShell)

```powershell
irm https://github.com/99percentgrip/agent-vesper/raw/main/scripts/install.ps1 | iex
```

### Set your credential

```sh
export ZAI_API_KEY="<your-key>"   # Get one at https://z.ai/
agent-vesper-tui                   # Launch the TUI
```

### Install in Zed

Add to Zed's `settings.json`:

```json
{
  "agent_servers": {
    "agent-vesper": {
      "command": "agent-vesper-acp",
      "env": { "ZAI_API_KEY": "<your-key>" }
    }
  }
}
```

Restart Zed → Agent Panel → **Agent Vesper**.

### Uninstall

```sh
# macOS / Linux
curl -fsSL https://github.com/99percentgrip/agent-vesper/raw/main/scripts/uninstall.sh | sh

# Windows
irm https://github.com/99percentgrip/agent-vesper/raw/main/scripts/uninstall.ps1 | iex
```

Credentials are never touched by the uninstaller.

## Quick Start

```sh
# 1. Launch the TUI
agent-vesper-tui

# 2. Teach it who you are
/remember I'm Alex, a Rust developer working on Agent Vesper

# 3. Teach it your preferences
/remember I prefer conventional commits and dislike unwrapped unwrap() calls

# 4. Ask a question — it already knows you
What's the best way to handle errors in my codebase?

# 5. Search its memory
/recall error handling

# 6. Check what it knows about you
/recall preferences
```

The agent silently recalls relevant memories before every reply. You don't need to repeat context — it already knows.

## Slash Commands

| Command | Description |
|---|---|
| `/remember <text>` | Add a fact to the cognitive memory store |
| `/recall <query>` | Search the cognitive memory store |
| `/forget <id>` | Delete a cognitive memory by ID |
| `/memory [query]` | List project-local memory entries |
| `/goal <text>` | Set a persistent goal |
| `/skills` | List learned skills |
| `/profile` | Show approved user-profile preferences |
| `/awareness` | Show live epistemic state |
| `/checkpoint [label]` | Capture a workspace snapshot |
| `/rollback <id>` | Restore a checkpoint |
| `/sessions` | Search past sessions |
| `/mcp [list\|add\|remove\|tools]` | Manage MCP servers |
| `/plugins [list\|verify\|load\|trust]` | Manage signed plugins |
| `/thinking <level>` | Set reasoning depth (disabled/enabled/high/max) |
| `/model <name>` | Switch the active model |
| `/plan <coding\|standard\|bigmodel>` | Select API plan |
| `/compact [N]` | Compact older context |
| `/export [md\|json]` | Export the current session |
| `/help` | Show the full command reference |

**87 commands total** — type `/help` in the TUI for the complete list.

## Configuration

All configurable from the TUI — no restart needed:

| Option | Values | Description |
|---|---|---|
| **Model** | GLM-5.2, GLM-5-Turbo, GLM-4.7, GLM-4.6 (+ vision models) | Model list syncs to the selected API plan |
| **Reasoning** | Off, Enabled, High, Max | Session-scoped reasoning depth |
| **API Plan** | Coding, Standard, BigModel (CN) | Switch endpoints |
| **Permissions** | Ask, Read Only, Bypass | Gate destructive tools |
| **Generation** | Balanced, Precise, Exploratory | Temperature / sampling strategy |

### Cognitive Memory Configuration

| Env Var | Default | Description |
|---|---|---|
| `AGENT_VESPER_COGNITION_ROOT` | `.agent-vesper/cognition/` | SQLite database location |
| `AGENT_VESPER_COGNITION_USER_ID` | `local` | Scope identifier for memory partitioning |
| `AGENT_VESPER_COGNITION_MODEL` | `glm-4.6` | Extraction LLM model |
| `AGENT_VESPER_COGNITION_EMBEDDING_API` | (local hash) | Set to `bigmodel` for neural embeddings (requires BigModel CN JWT auth) |

### Push-to-Talk Voice Configuration

Voice works out-of-the-box on Linux and macOS — press **F5** to record, **F5** to transcribe. On first use with no existing `faster-whisper` install, the binary auto-creates a harness-owned venv via the installer-bundled `uv` and pip-installs `faster-whisper` (one-time, needs network). All optional — discovery finds existing venvs first.

| Env Var | Default | Description |
|---|---|---|
| `VESPER_PYTHON_PATH` | (auto) | Force a specific Python executable for the voice sidecar (highest precedence) |
| `GLM_VENV_PATH` | (auto) | Point at an existing virtualenv root |
| `AGENT_VESPER_VOICE_VENV` | `$XDG_DATA_HOME/agent-vesper/voice-venv` | Override the harness-owned voice venv location |
| `GLM_ACP_WHISPER_MODEL` | `base` | faster-whisper model size (`tiny`/`base`/`small`/`medium`/`large`) |

## Architecture

Agent Vesper is a **19-crate Rust workspace** with strict dependency boundaries enforced by `cargo xtask architecture`:

```
apps/
├── agent-vesper-acp/     ACP protocol-v1 stdio server (for Zed/editors)
├── agent-vesper-tui/     Interactive terminal UI (ratatui + crossterm)

crates/
├── vesper-cognition/     cognitive memory engine (SQLite + FTS5 + entity graph)
├── vesper-memory/        Durable memory graph, skills, profile, awareness ledger
├── vesper-agent/         Multi-turn tool-executing agent loop (Tier C)
├── vesper-runtime/       Provider-neutral session actors + reasoning dial
├── vesper-provider-glm/  Z.ai GLM provider adapter (auth, catalog, SSE, retry)
├── vesper-mcp/           MCP stdio client + Ed25519-signed plugin loader
├── vesper-checkpoints/   Workspace snapshots, rollback, session lineage
├── vesper-sessions/      Transactional session writer + read-only discovery
├── vesper-acp/           Official-SDK ACP protocol adapter
├── vesper-domain/        Provider-neutral values and events
├── vesper-security/      Secret-safe primitives, path confinement
├── vesper-auth/          OS credential manager + Unix vault fallback
├── vesper-config/        Platform paths, profiles, typed configuration
├── vesper-policy/        Pure permission and policy decisions
├── vesper-harness/       Shared hosted Python-oracle tool services
├── vesper-observability/ Secret-safe trajectory recording
├── vesper-provider-synthetic/  Deterministic reference provider
└── vesper-testkit/       Synthetic read-store / no-write helpers
```

**Key principles:**
- `#![forbid(unsafe_code)]` enforced workspace-wide
- MSRV 1.88 (tested in CI)
- No external services required (SQLite embedded, local hash embedder by default)
- Provider-neutral trait ports — adding a 2nd provider needs zero TUI edits

## Cognitive Memory Pipeline

The memory engine implements a single-pass ADD-only extraction pipeline:

```
User says something
  → LLM extracts atomic facts (single call, JSON output)
    → Facts get type (persona/episodic/instruction) + priority (0-100) + scene label
      → MD5 hash dedup drops exact duplicates
        → Entities extracted + linked to memories (semantic dedup ≥ 0.95)
          → Stored in SQLite with FTS5 BM25 index
            → On next prompt: hybrid search recalls top-5 relevant memories
              → Silently injected into the user message before the provider call
```

**Optional enhancements:**
- Conflict detection (store/skip classification via second LLM call)
- RRF fusion (Reciprocal Rank Fusion as alternative to additive scoring)
- Both disabled by default — enable via `CognitiveConfig`

## Local Verification

```sh
cargo xtask verify          # fmt + clippy -D warnings + 655 tests + architecture
cargo xtask architecture    # dependency-boundary validation (22 packages)
cargo xtask msrv            # Rust 1.88 compatibility check
```

See [migration status](docs/migration-status.md), [architecture](docs/architecture.md), and [ADRs](docs/adr/).

## License

Apache-2.0

---

<div align="center">

**[Install](#install)** · **[Features](#features)** · **[Commands](#slash-commands)** · **[Architecture](#architecture)**

Agent Vesper · [99percentgrip](https://github.com/99percentgrip) · Apache-2.0

</div>
