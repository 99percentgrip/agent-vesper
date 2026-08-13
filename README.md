<div align="center">

# Agent Vesper

### A Rust-native AI agent with persistent cognitive memory — provider-neutral by design.

[![CI](https://github.com/99percentgrip/agent-vesper/actions/workflows/ci.yml/badge.svg)](https://github.com/99percentgrip/agent-vesper/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/99percentgrip/agent-vesper)](https://github.com/99percentgrip/agent-vesper/releases)
[![MSRV](https://img.shields.io/badge/MSRV-1.88-orange.svg)](https://blog.rust-lang.org/2025/06/23/Rust-1.88.0.html)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Crates](https://img.shields.io/badge/workspace-22%20packages-FF6B35.svg)](#architecture)

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
<tr><td><b>🧩 Reasoning Orchestrator</b></td><td>Strategy-driven orchestration (VRO): profile-driven routing across <code>Direct</code>, <code>GenerateVerifyRepair</code>, <code>ParallelCandidatesConsensus</code>, <code>ParallelCandidatesJudge</code>, <code>ToolGroundedReact</code>, <code>BoundedTreeSearch</code>, and <code>ProposerCriticAdjudicator</code>. VRO-4 ships <b>parallel candidate branches</b> with strict isolation, deterministic ordering, budget-capped fan-out, and either quorum-based consensus (§11.4) or position-bias-shuffled judge arbitration (§11.5). VRO-5 ships a <b>tool-grounded ReAct loop</b> (§11.6) with Read-Before-Write policy enforcement, structured failure observations, and a production LM Studio <code>ReactAgent</code> adapter. VRO-6 ships <b>bounded tree search</b> (§11.7) — depth/branching-limited expansion with aggressive verifier-based pruning and early-stop on the first passing leaf — and the <b>Proposer / Critic / Adjudicator</b> workflow (§11.8) with strict three-role separation: fan-out proposals → per-candidate objective critiques → criteria-anchored adjudication (not persuasive prose). VRO-7 ships <b>Verified Workflow Learning</b> (§11.9): successful complex-strategy turns are summarized into sanitized, generalized <code>ProceduralMemory</code> recipes by a <code>SecretScrubber</code>-guarded <code>WorkflowExtractor</code> (AWS keys, JWTs, bearer tokens, IPs, and high-entropy strings are redacted to <code>[REDACTED:&lt;KIND&gt;]</code> placeholders before any byte is persisted), then saved to cognitive memory through a pluggable <code>ProceduralMemorySink</code>. Learning is non-blocking — extraction and sink errors surface as <code>unresolved_risks</code> notes and never affect the underlying turn outcome. VRO-8 ships <b>UX & Diagnostics</b>: the Reasoning Panel surfaces the chosen strategy, mode, budget, and a prominent <b>⚠ RISK ESCALATION</b> warning when the profiler escalates a task; a manual <code>/reasoning set mode=&lt;auto|fast|balanced|deep|maximum|off&gt;</code> slash command overrides the profiler for the duration of the session; a <b>✓ LEARNED</b> notice appears in the panel when VRO-7 extracts a workflow. VRO-9 closes the final four PRD gaps: <b>race-aware branch cancellation</b> (PRD §10.6 — verified-success predicate aborts pending siblings via <code>JoinHandle::abort</code>), <b>cross-model candidate racing</b> (PRD §10.6 — <code>MultiModelCandidateGenerator</code> round-robins across heterogeneous providers), <b>strict budget enforcement</b> of all three ceilings (PRD §10.4 — <code>max_model_calls</code> + <code>max_total_output_tokens</code> + <code>max_wall_time_ms</code> all trigger <code>BudgetExceeded</code>), and <b>live HTTP integration tests</b> against a real LM Studio endpoint (PRD §22.2, <code>#[ignore]</code>-gated).</td></tr>
</table>

## Install

### macOS / Linux

```sh
curl -fsSL https://github.com/99percentgrip/agent-vesper/raw/main/scripts/install.sh | sh
```

Or pin a version:

```sh
AGENT_VESPER_VERSION=0.20.26 sh scripts/install.sh
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
| `/reasoning set mode=<auto\|fast\|balanced\|deep\|maximum\|off>` | Override the TaskProfiler's auto-selected reasoning mode for the session (VRO-8) |
| `/reasoning clear` | Return to profiler defaults (alias for `set mode=auto`) |
| `/model <name>` | Switch the active model |
| `/plan <coding\|standard\|bigmodel>` | Select API plan |
| `/compact [N]` | Compact older context |
| `/export [md\|json]` | Export the current session |
| `/help` | Show the full command reference |

**87 commands total** — type `/help` in the TUI for the complete list.

## Reasoning Orchestrator (VRO)

The Vesper Reasoning Orchestrator (VRO) is the multi-strategy reasoning layer that sits above the raw provider dispatch. Every prompt is profiled by a deterministic **TaskProfiler** (no LLM call), routed to one of ten **ReasoningStrategies**, and bounded by a **ReasoningBudget** preset. The TUI surfaces every step of this decision through the **Reasoning Panel** header so you always know what's happening and why.

### The Six Reasoning Modes (PRD §8.1)

| Mode | Behavior | Default budget |
|---|---|---|
| `auto` (default) | The profiler profiles and selects — the recommended starting point | varies by strategy |
| `fast` | Single pass; strict latency ceiling (30s wall clock, 1 model call) | minimal: 1/0/30000 |
| `balanced` | Decomposition + one verify/repair cycle when needed | 4/1/2 (PRD §24) |
| `deep` | Multiple candidates, stronger verification, bounded search | 10/2/3/3 (PRD §24) |
| `maximum` | Highest configured test-time budget for hard / high-value work | 16/3/4 |
| `off` | Bypass VRO entirely — use the provider's direct response path | (none) |

**Budget tuple**: `(max_model_calls, max_repairs, max_parallel_branches, max_search_depth)` — `fast`/`balanced` omit unpinned fields per PRD §24.

### The Ten Reasoning Strategies (PRD §10.3)

The profiler maps each prompt onto one of:

| Strategy | When it fires | PRD reference |
|---|---|---|
| `direct` | Trivial / low-risk tasks (chat, simple Q&A) | §11.1 |
| `plan_then_answer` | Decomposition helps but execution is unnecessary | §11.2 |
| `plan_execute_verify` | Plan, execute, then verify | §10.3 |
| `generate_verify_repair` | Generate → verify → targeted repair loop | §11.3 |
| `parallel_candidates_consensus` | Fan-out parallel candidates + quorum vote | §11.4 |
| `parallel_candidates_judge` | Parallel candidates adjudicated by a verifier | §11.5 |
| `tool_grounded_react` | Environment-grounded ReAct loop with live tool calls | §11.6 |
| `bounded_tree_search` | Depth/breadth-limited search with verifier pruning | §11.7 |
| `proposer_critic_adjudicator` | Three-role workflow: propose → critique → judge | §11.8 |
| `workflow_replay_with_verification` | Reuse a learned workflow + re-verify | §10.3 |

### The Reasoning Panel Header (VRO-8)

Every VRO turn now opens with a structured diagnostic header at the top of the Reasoning Panel, rendered through the same markdown pipeline as the streamed thinking:

```
Strategy: bounded_tree_search | Mode: deep (override) | Risk: high ⚠ RISK ESCALATION
        | Depth: 3 | Branches: 3 | Models: 10 | Repairs: 2
──────────────────────────────────────────────────────────────────────
(streamed chain-of-thought continues below…)
```

- **`Strategy`** — the exact variant chosen (snake_case, matches the wire format)
- **`Mode`** — the active reasoning mode; `*(override)*` marks when the user forced it via `/reasoning set mode=…`
- **`Risk`** — the profiler's consequence assessment (`low`/`medium`/`high`)
- **⚠ RISK ESCALATION** — prominently surfaced when the profiler escalated a task to `high` risk (e.g. mutating shell commands, deletions)
- **Depth / Branches / Models / Repairs** — the budget envelope from the active preset

### `/reasoning` — Manual Override (VRO-8)

Override the profiler for the duration of the session:

```text
/reasoning set mode=deep      # force Deep until cleared
/reasoning set mode=fast      # force Fast
/reasoning set mode=auto      # return to profiler defaults
/reasoning clear              # alias for set mode=auto
/reasoning                    # show the current override
```

The override **persists for the entire conversation** (mirrors how superpower overrides work). The next prompt will run under the forced mode regardless of what the profiler would recommend — `/reasoning set mode=off` will bypass VRO entirely for every subsequent turn.

### `/thinking` vs `/reasoning` — they are different things

| Surface | Scope | Affects | Example |
|---|---|---|---|
| `/thinking <level>` | Provider superpower | The model's chain-of-thought depth (GLM: `disabled`/`enabled`/`high`/`max`) | `/thinking max` |
| `/reasoning set mode=<X>` | VRO orchestrator | Which strategy + budget the orchestrator uses (PRD §8.1) | `/reasoning set mode=deep` |

`/reasoning <level>` (e.g. `/reasoning high`) still resolves to the GLM `thinking` superpower for backward compatibility — the VRO-8 behavior is gated by the `set mode=` / `clear` / no-argument patterns.

### ✓ LEARNED — Verified Workflow Learning (VRO-7, PRD §11.9)

After a successful complex-strategy turn (ReAct, GVR, parallel candidates, tree search, or PCA), the orchestrator summarizes the trajectory into a sanitized, generalized `ProceduralMemory` recipe and pushes a **✓ LEARNED** notice through the Reasoning Panel:

```
✓ LEARNED Workflow extracted (3 step(s), strategy=`tool_grounded_react`)
        and saved to cognitive memory.
```

The extraction runs through a `SecretScrubber` first — AWS keys, JWTs, bearer tokens, IPs, and high-entropy strings are redacted to `[REDACTED:<KIND>]` placeholders **before** any byte is persisted. Extraction errors never affect the underlying turn; they surface as `unresolved_risks` notes. `PrivacyMode::Private` requests are rejected entirely (no procedure is built or persisted).

### VRO-9 — Final Optimization (PRD §10.4 / §10.6 / §20 / §22.2)

VRO-9 closes the four remaining PRD gaps surfaced by the Vesper Reasoning Orchestrator audit:

- **Race-aware branch cancellation (PRD §10.6 "Branch cancellation" + "Early stopping").** The `CandidateExecutor` now exposes `fan_out_with_early_stop(generator, prompt, requested, budget, predicate)`. Branches are polled with `tokio::select!` over their `JoinHandle`s; the moment one branch completes and the predicate fires (e.g. a verifier-backed success signal), every still-pending sibling is `JoinHandle::abort`-ed and its model call is cancelled. The plain `fan_out` delegates with `|_| false`, so existing callers see no behavior change.
- **Cross-model candidate racing (PRD §10.6 "Cross-model candidates").** A new `MultiModelCandidateGenerator` round-robins each call across a pool of heterogeneous providers (`Vec<Box<dyn CandidateGenerator>>`) — e.g. one branch to LM Studio, the next to a remote API. PRD §10.6: "Candidate diversity must not be simulated merely by asking for 'three alternatives' in one completion."
- **Strict budget enforcement (PRD §10.4 "Budget Manager").** The Generate-Verify-Repair loop now enforces **all three** ceilings: `max_model_calls`, `max_total_output_tokens`, and `max_wall_time_ms`. A breach of any ceiling returns `OutcomeStatus::BudgetExceeded` with the breached-budget name in the `unresolved_risks` note. The Phase R3 calibrated budget presets (PRD §20 — derived from observed coding-agent output: `max_model_calls × ~6 KiB` per call) replaced the VRO-1 placeholder magic numbers.
- **Live HTTP integration tests (PRD §22.2 "Real LM Studio process").** `crates/vesper-agent/tests/live_react_integration.rs` exercises the Tool-Grounded ReAct loop against a real LM Studio endpoint at `localhost:1234` via `reqwest` (declared as a dev-only dependency so `src/` stays architecture-clean). All four tests are `#[ignore]`-gated and skip cleanly when the endpoint is offline; run them locally with `cargo test -p vesper-agent --test live_react_integration -- --ignored`.

### How VRO interacts with the live tool surface

- The `tool_grounded_react` strategy (VRO-5) routes through a real LM Studio `ReactAgent` bundle when configured — `Actions` and `Observations` stream live into the Reasoning Panel as `▶ ACTION` / `↳ OBSERVATION` / `✗ ERROR` / `✓ FINISH` lines, alongside the diagnostic header above.
- The other nine strategies execute in the background; only the LEARNED notice and the final answer land in the panel.
- VRO is **off by default** (PRD §21 — zero behavior regression when disabled). It activates only when `[reasoning] enabled = true` is set in the runtime config OR a `/reasoning set mode=<X>` override is in force.

## Configuration

All configurable from the TUI — no restart needed:

| Option | Values | Description |
|---|---|---|
| **Model** | GLM-5.2, GLM-5-Turbo, GLM-4.7, GLM-4.6 (+ vision models) | Model list syncs to the selected API plan |
| **Reasoning Depth** | Off, Enabled, High, Max | Session-scoped GLM thinking depth (provider superpower; `/thinking`) |
| **Reasoning Mode (VRO)** | Auto, Fast, Balanced, Deep, Maximum, Off | VRO orchestrator mode override (`/reasoning set mode=…`); see [Reasoning Orchestrator](#reasoning-orchestrator-vro) |
| **API Plan** | Coding, Standard, BigModel (CN) | Switch endpoints |
| **Permissions** | Ask, Read Only, Bypass | Gate destructive tools |
| **Generation** | Balanced, Precise, Exploratory | Temperature / sampling strategy |

### Cognitive Memory Configuration

| Env Var | Default | Description |
|---|---|---|
| `AGENT_VESPER_COGNITION_ROOT` | `.agent-vesper/cognition/` | SQLite database location |
| `AGENT_VESPER_COGNITION_USER_ID` | `local` | Scope identifier for memory partitioning |
| `AGENT_VESPER_COGNITION_MODEL` | `glm-4.6` | Extraction LLM model |
| `AGENT_VESPER_COGNITION_EMBEDDING_API` | (local hash) | Set to `bigmodel` for BigModel neural embeddings (requires BigModel CN JWT auth) |
| `AGENT_VESPER_COGNITION_EMBEDDING_MODEL` | `text-embedding-nomic-embed-text-v1.5` | LM Studio embedding model name (used when LM Studio is the active provider) |
| `LMSTUDIO_API_KEY` | (none) | Optional bearer token for a metered LM Studio embedding endpoint |

### Multi-Provider Embedding Strategy

The cognitive memory engine picks its embedder from the **active chat provider**, not the project default:

| Active provider | Embedder | Behavior |
|---|---|---|
| **LM Studio** + settings | `LmStudioEmbedder` | **Real neural embeddings** via LM Studio's `/v1/embeddings` endpoint. True semantic cosine: "do you remember me" ↔ "user's name is Alex" matches because the underlying text-embedding model captures meaning. |
| ZAI + `AGENT_VESPER_COGNITION_EMBEDDING_API=bigmodel` | `BigModelEmbeddingAdapter` | BigModel CN neural embeddings (requires JWT auth) |
| ZAI (default) or no provider | `LocalHashEmbedder` | Zero-network bag-of-words fallback. Cosine only fires on lexical overlap. |

**Automatic migration**: when the active embedder changes (e.g. switching providers), the engine compares the new model name against the `embedding_model` row in `cognition_meta` and re-embeds every memory + entity in chunks of 256 if they differ. The startup log shows `"cognition: re-embedded N memories and M entities to model \"...\" (768-d)."` confirming the migration ran.

### Provider-Independent Embedding Layer (ADR 0016, v0.20.14 + v0.20.15)

**The fundamental cross-provider gap is eliminated.** Write `.agent-vesper/cognition/embedding.json` to decouple the embedding source from the active chat provider entirely:

```json
{
  "source": "lmstudio",
  "endpoint": "http://localhost:1234/v1/embeddings",
  "model": "text-embedding-nomic-embed-text-v1.5",
  "dimension": 768
}
```

With this file in place, switching chat providers (ZAI ↔ LM Studio ↔ future X) **does not change the embedder** — cosine similarity cannot silently break, and no migration is ever needed mid-session. When the file is absent (or `source` is unset), the engine falls back to the provider-routed behavior described above (zero migration cost for existing installs).

**Live search mode** (`SearchMode::Hybrid` vs `SearchMode::BM25Only`) backs an atomic, never-silent recall contract: if the embedder fails mid-search, the engine atomically degrades to **BM25-only keyword recall** instead of returning an error or an empty result. The user sees keyword matches even when the embedding endpoint is completely down. The next successful embed call auto-upgrades back to hybrid mode.

| `SearchMode` | Embedder | Semantic | BM25 | Entity boost | `Err` on embedder failure? |
|---|---|---|---|---|---|
| `Hybrid` | Called | ✅ | ✅ | ✅ | ❌ (auto-degrades) |
| `BM25Only` | Skipped | ❌ | ✅ | ❌ | ❌ (never) |

**v0.20.15 follow-ups (three v0.20.14 deferrals closed):**
1. **BigModel source path resolves correctly** — `source: "bigmodel"` constructs `BigModelEmbeddingAdapter` (JWT auth resolved per call from the ZAI credential) instead of falling back to `LocalHashEmbedder`.
2. **Background-thread startup probe** — TUI loads instantly. The engine starts in `BM25Only`; a background `std::thread::spawn` issues a one-shot `embed()` call and flips to `Hybrid` on success. If a memory search runs before the probe completes, `search()` honors `BM25Only` and returns keyword-only results — graceful fallback, no UI stall.
3. **`/embedding` slash command (UX parity)** — registered in the Vesper-native surface:
   - `/embedding` → render live config + active search mode + model name
   - `/embedding set source=lmstudio endpoint=... model=... api_key=... dimension=...` → parse, validate, persist `embedding.json`, hot-reload with another background probe
   - `/embedding clear` → delete `embedding.json` (revert on next restart)

See [ADR 0016](docs/adr/0016-provider-independent-embedding-layer.md) for the full design.

### Push-to-Talk Voice Configuration

Voice works out-of-the-box on Linux and macOS — press **F5** to record, **F5** to transcribe. On first use with no existing `faster-whisper` install, the binary auto-creates a harness-owned venv via the installer-bundled `uv` and pip-installs `faster-whisper` (one-time, needs network). All optional — discovery finds existing venvs first.

| Env Var | Default | Description |
|---|---|---|
| `VESPER_PYTHON_PATH` | (auto) | Force a specific Python executable for the voice sidecar (highest precedence) |
| `GLM_VENV_PATH` | (auto) | Point at an existing virtualenv root |
| `AGENT_VESPER_VOICE_VENV` | `$XDG_DATA_HOME/agent-vesper/voice-venv` | Override the harness-owned voice venv location |
| `GLM_ACP_WHISPER_MODEL` | `base` | faster-whisper model size (`tiny`/`base`/`small`/`medium`/`large`) |

## Architecture

Agent Vesper is a **22-package Rust workspace** with strict dependency boundaries enforced by `cargo xtask architecture`:

```
apps/
├── agent-vesper-acp/     ACP protocol-v1 stdio server (for Zed/editors)
├── agent-vesper-tui/     Interactive terminal UI (ratatui + crossterm)

crates/
├── vesper-cognition/     cognitive memory engine (SQLite + FTS5 + entity graph + neural embeddings)
├── vesper-memory/        Durable memory graph, skills, profile, awareness ledger
├── vesper-agent/         Multi-turn agent loop + Vesper Reasoning Orchestrator (VRO)
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
cargo xtask verify          # fmt + clippy -D warnings + 866 tests + architecture
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
