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
<tr><td><b>🧠 Cognitive Memory</b></td><td>The agent extracts facts from every conversation and <b>silently recalls relevant memories before each reply</b>. <code>/remember</code> smart-routes stable identity/preferences globally and repository facts to the current project, always confirms the chosen scope, and accepts <code>--global</code> or <code>--project</code> overrides. Use <code>/memories</code> to audit both stores and <code>/promote</code>/<code>/demote</code> to correct scope later.</td></tr>
<tr><td><b>📝 Rich Markdown TUI</b></td><td>Single-column terminal UI (Claude Code / Codex style): the Conversation panel takes the full body height and streams <b>thinking inline</b> (dim italic <code>🧠 Thinking · strategy · mode · risk</code> block, F2 to toggle), <b>live tool telemetry</b> with <code>⏺</code> action / <code>⎿</code> result glyphs, and the answer in one feed — plus syntax-highlighted code blocks, full-width user-turn role banners, an interactive centered tool-permission modal, mouse-wheel + PageUp/PageDown/Home/End scrolling with a visible scrollbar, a live slash-command palette, and <b>bracketed-paste mode</b> (multi-line clipboard content arrives as a single contiguous insertion — no premature submits on embedded newlines). On wide terminals the Session + TODO + Last-run sidebar rail collapses with <code>F11</code> for a chat-only full-width view — the collapse is a pure render-time overlay, so a second F11 restores your exact panel layout. Built on <code>ratatui</code> + <code>crossterm</code>.</td></tr>
<tr><td><b>🎯 Plan Mode</b></td><td>A pure 4-phase state machine (<code>NORMAL → PLANNING → REVIEW → EXECUTING</code>) that lets the model author a plan, you review it, then it executes with bounded tool calls.</td></tr>
<tr><td><b>🔧 95 Slash Commands</b></td><td>The complete Python oracle command surface plus Vesper-native controls — scoped cognitive memory, VesperLens interview limits, embeddings, authentication, provider settings, and more.</td></tr>
<tr><td><b>🔐 Provider-Neutral Auth</b></td><td>Credentials route through the provider layer — never hardcoded. OS keyring with owner-only Unix vault fallback. <code>/auth</code> force-rotates without restart.</td></tr>
<tr><td><b>📦 Ed25519-Signed Plugins</b></td><td>Declarative plugin packages (permissions only — no executable code). Unsigned loading is structurally erased from <code>--release</code> builds via <code>#[cfg(debug_assertions)]</code>.</td></tr>
<tr><td><b>🔄 Session Lineage</b></td><td>Workspace snapshots, rollback, session branching, and a bounded cron/export/clipboard/CI surface — all RAII-safe with strict <code>Drop</code> file-handle discipline.</td></tr>
<tr><td><b>⚡ Hybrid Retrieval</b></td><td>Multi-signal scoring: <code>(semantic + BM25 + entity_boost) / max_possible</code>. Snowball lemmatization, FTS5 keyword search, entity-graph boosting with hyper-connection penalty.</td></tr>
<tr><td><b>📊 Priority + Heat Tracking</b></td><td>Every memory gets a type (<code>persona</code>/<code>episodic</code>/<code>instruction</code>), priority (0-100), and scene label. Frequently-recalled memories accumulate heat and float to the top.</td></tr>
<tr><td><b>🛡️ Secret-Safe</b></td><td>All error messages are sanitized. No file contents, API keys, paths, or memory text leak through <code>CognitionError</code>. <code>#![forbid(unsafe_code)]</code> enforced workspace-wide.</td></tr>
<tr><td><b>🎙️ Push-to-Talk Voice</b></td><td>Press <code>F5</code> to record from the microphone, <code>F5</code> again to transcribe speech-to-text straight into the composer (Linux + macOS). Auto-discovers any existing <code>faster-whisper</code> venv, or <b>self-bootstraps</b> a harness-owned one via the installer-bundled <code>uv</code> on first use — no separate Python setup. A long-lived sidecar loads the Whisper model once per session for instant subsequent transcriptions.</td></tr>
<tr><td><b>🧩 Reasoning Orchestrator</b></td><td>Strategy-driven orchestration (VRO): profile-driven routing across <code>Direct</code>, <code>GenerateVerifyRepair</code>, <code>ParallelCandidatesConsensus</code>, <code>ParallelCandidatesJudge</code>, <code>ToolGroundedReact</code>, <code>BoundedTreeSearch</code>, and <code>ProposerCriticAdjudicator</code>. VRO-4 ships <b>parallel candidate branches</b> with strict isolation, deterministic ordering, budget-capped fan-out, and either quorum-based consensus (§11.4) or position-bias-shuffled judge arbitration (§11.5). VRO-5 ships a <b>tool-grounded ReAct loop</b> (§11.6) with Read-Before-Write policy enforcement, structured failure observations, and a production LM Studio <code>ReactAgent</code> adapter. VRO-6 ships <b>bounded tree search</b> (§11.7) — depth/branching-limited expansion with aggressive verifier-based pruning and early-stop on the first passing leaf — and the <b>Proposer / Critic / Adjudicator</b> workflow (§11.8) with strict three-role separation: fan-out proposals → per-candidate objective critiques → criteria-anchored adjudication (not persuasive prose). VRO-7 ships <b>Verified Workflow Learning</b> (§11.9): successful complex-strategy turns are summarized into sanitized, generalized <code>ProceduralMemory</code> recipes by a <code>SecretScrubber</code>-guarded <code>WorkflowExtractor</code> (AWS keys, JWTs, bearer tokens, IPs, and high-entropy strings are redacted to <code>[REDACTED:&lt;KIND&gt;]</code> placeholders before any byte is persisted), then saved to cognitive memory through a pluggable <code>ProceduralMemorySink</code>. Learning is non-blocking — extraction and sink errors surface as <code>unresolved_risks</code> notes and never affect the underlying turn outcome. VRO-8 ships <b>UX & Diagnostics</b>: the Reasoning Panel surfaces the chosen strategy, mode, budget, and a prominent <b>⚠ RISK ESCALATION</b> warning when the profiler escalates a task; a manual <code>/reasoning set mode=&lt;auto|fast|balanced|deep|maximum|off&gt;</code> slash command overrides the profiler for the duration of the session; a <b>✓ LEARNED</b> notice appears in the panel when VRO-7 extracts a workflow. VRO-9 closes the final four PRD gaps: <b>race-aware branch cancellation</b> (PRD §10.6 — verified-success predicate aborts pending siblings via <code>JoinHandle::abort</code>), <b>cross-model candidate racing</b> (PRD §10.6 — <code>MultiModelCandidateGenerator</code> round-robins across heterogeneous providers), <b>strict budget enforcement</b> of all three ceilings (PRD §10.4 — <code>max_model_calls</code> + <code>max_total_output_tokens</code> + <code>max_wall_time_ms</code> all trigger <code>BudgetExceeded</code>), and <b>live HTTP integration tests</b> against a real LM Studio endpoint (PRD §22.2, <code>#[ignore]</code>-gated). VRO-10 closes the final PRD gaps in streaming, planner fields, branch diversification, repair heuristics, typed evidence, ACP events, rate accounting, and soak coverage. VRO-11 (ADRs 0017 and 0018) ships <b>VesperLens</b>: a native loopback browser for interactive HTML review and structured planning interviews, with automatic browser handoff, explicit <code>request_human_review</code>/<code>request_human_input</code> tools, and no new web-framework dependency.</td></tr>
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
      "env": {
        "ZAI_API_KEY": "<your-key>",
        "AGENT_VESPER_ENABLE_SESSION_READS": "1",
        "AGENT_VESPER_ENABLE_SESSION_WRITES": "1"
      }
    }
  }
}
```

Restart Zed → Agent Panel → **Agent Vesper**.

Both session flags are required for durable editor chats: writes transactionally
save completed turns, while reads let a new ACP process list, load, and resume
them after Zed restarts. On Linux the default store is
`$XDG_DATA_HOME/agent-vesper/sessions/` (normally
`~/.local/share/agent-vesper/sessions/`). Checkpoints remain a separate,
explicit opt-in and are not required for chat persistence.

The chat footer carries the full multi-provider selector surface (advertised via ACP `sessionConfigOptions` on `session/new`/`load`/`resume`/`set`): **Provider** (TUI `/provider` parity — lists every registered adapter with live credential status; Z.ai GLM + LM Studio are both registered in every boot; switching swaps the acting provider for the next turn, GLM keeps its overrides across round trips, and unauthenticated GLM descriptions tell the user to run `--setup`) plus the controls of the **acting provider only** (dynamic capability gating, `docs/provider-capability-gating-prd.md`): when Z.ai GLM acts — **Model** (Mixture-of-Agents picker first, then the plan's models), **Reasoning** (Off/Standard, Deep·High/Deep·Max only on deep-reasoning models), **API Plan**, **Generation Style**, **Auxiliary Model**, **Mixture of Agents**; when LM Studio acts — ONLY a truthful **Model** picker fed by the server's live `/api/v1/models` catalog (verified LM Studio schema: live model ids, advertised context sizes; the pinned settings model always present as the offline fallback). **Permissions** (ask/bypass/read) is always advertised. GLM-only selections made while LM Studio acts are rejected fail-closed (never a silent cross-provider route). Every selection is validated against the provider's own surface (invented values are rejected fail-closed) and takes effect on the very next turn; the **token counter** follows the acting provider — GLM's frozen per-model context windows, the LM Studio model's advertised `max_context_length` (conservative floor when unadvertised, never GLM's 1M) — through live `usage_update` notifications.

In Zed, typing `/` in the composer surfaces the **28-command harness catalog** (advertised via ACP `available_commands_update` on every new/load/resume): report commands (`/help`, `/status`, `/memory`, `/skills`, `/profile`, `/awareness`, `/metacognition`, `/deliberation`, `/goal`, `/subgoal`, `/curator`, `/sessions`, `/lineage`, `/version`) execute in-process with no model call, `/max-iterations [N]` tunes the per-turn tool cap for the session, and unknown `/` text gets the oracle's bounded fallback. Slash turns never reach the provider and are never persisted. Every host-owned command is wired with full TUI parity: `/checkpoint`/`/rollback`/`/undo`/`/export`/`/sessions`/`/lineage`/`/ci`/`/plugins`/`/mcp` run against the same durable checkpoint/MCP stores the TUI uses, `/compact`/`/clear-history`/`/clear-plan` mutate the engine's own conversation state, `/usage` queries the live provider quota endpoint, and `/diff`/`/release` drive a real agent workflow turn. `/skills` reads the project layer **plus the cross-project global layer** (`~/.agent-vesper/memory`), so globally learned skills appear exactly like in the TUI.

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

# 2. Teach it who you are (smart-routed globally with a visible confirmation)
/remember I'm Alex, a Rust developer working on Agent Vesper

# 3. Teach it your preferences (force either scope when needed)
/remember I prefer conventional commits and dislike unwrapped unwrap() calls
/remember --project The mock server runs on port 8321

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
| `/remember [--global\|--project] <text>` | Smart-route a fact and visibly confirm its global or project scope (`--local` aliases `--project`) |
| `/recall [--global\|--project] <query>` | Search both cognitive stores, or restrict the scope |
| `/forget [--global\|--project] <id>` | Delete a cognitive memory by full or displayed short ID |
| `/memories [query]` | Audit global and project cognitive memories with scope labels and IDs |
| `/promote <id>` | Move a project memory into global memory |
| `/demote <id>` | Move a global memory into the current project |
| `/memory [query]` | List project-local memory entries |
| `/goal <text>` | Set a persistent goal |
| `/skills` | List learned skills (frontmatter descriptions; project + global library) |
| `/read_skill` surface | Hosted `read_skill` takes `section`/`offset`/`limit` for large skills |
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

The Vesper Reasoning Orchestrator (VRO) is the multi-strategy reasoning layer that sits above the raw provider dispatch. Every prompt is profiled by a deterministic **TaskProfiler** (no LLM call), routed to one of ten **ReasoningStrategies**, and bounded by a **ReasoningBudget** preset. The TUI surfaces every step of this decision through the **inline thinking header** that streams in the Conversation panel so you always know what's happening and why.

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

### The Inline Thinking Header (VRO-8, restyled VRO-11.5)

Every VRO turn now opens with a compact diagnostic label at the top of the inline thinking block that streams in the Conversation panel (the standalone bottom Reasoning panel was removed in VRO-11.5 — thinking, tool telemetry, and the answer all render in one Claude Code-style column):

```
🧠 Thinking · Validating result · bounded_tree_search · deep (override) · risk: high ⚠ RISK ESCALATION
        (dimmed chain-of-thought streams below, bounded to the newest lines…)
```

- **Phase** — the live orchestrator phase (PRD §8.2 vocabulary) when one applies
- **Strategy** — the exact variant chosen (snake_case, matches the wire format)
- **Mode** — the active reasoning mode; `(override)` marks when the user forced it via `/reasoning set mode=…`
- **Risk** — the profiler's consequence assessment (`low`/`medium`/`high`), with a prominent **⚠ RISK ESCALATION** marker when the profiler escalated a task to `high` risk (e.g. mutating shell commands, deletions)
- The thinking block renders **dim + italic**, collapses away when the turn completes, and is toggleable with **F2**

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

After a successful complex-strategy turn (ReAct, GVR, parallel candidates, tree search, or PCA), the orchestrator summarizes the trajectory into a sanitized, generalized `ProceduralMemory` recipe and pushes a **✓ LEARNED** notice into the conversation feed:

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

### VRO-10 — The 100% PRD Annihilation Sweep (PRD §8.2 / §10.4 / §10.5 / §10.6 / §10.9 / §14.3 / §16 / §22.4)

VRO-10 closes the final 8 PRD gaps (3 PARTIAL + 5 DEFERRED) identified in the VRO-9 self-audit:

- **§8.2 Phase-Level Streaming Strings (PARTIAL).** The Reasoning Panel now renders a live **`Phase:` `<label>`** segment at the top of the diagnostic header, derived from the profiled strategy via `phase_label_for_strategy(...)`. The phase labels come from the PRD §8.2 vocabulary verbatim (Understanding request / Building plan / Exploring alternatives / Running tools / Validating result / Finalizing answer); `Direct` turns omit the phase line entirely.
- **§10.5 Planner Fields (PARTIAL).** `WorkflowPlanStep` gains the five previously-missing planner fields — `expected_output_schema`, `failure_policy: StepFailurePolicy`, `max_attempts`, `parallel_allowed`, `requires_user_approval`. Each carries conservative serde defaults so legacy plans deserialize unchanged.
- **§10.6 Candidate-Specific Branch Prompts (PARTIAL).** New `BranchDiversification` enum + `fan_out_diverse(...)` executor method. Each parallel branch receives a distinct prompt prefix from a 4-variant stance ladder (conservative → balanced → creative → highly creative); the existing `fan_out` / `fan_out_with_early_stop` are unchanged (zero-breakage).
- **§10.9 Repair Controller Heuristics (DEFERRED).** New `RepairController` + `classify_finding(&VerificationFinding) -> RepairHeuristic`. Each failed finding is classified (JSON parse / schema mismatch / file-not-found / compilation / test failure / constraint violation) and a class-specific correction hint is injected into the next Generate's corrections vector. A repeated-attempt guard escalates to `Failed` when the same finding recurs, preventing unbounded review loops.
- **§14.3 Placeholder Aliases (DEFERRED).** The VRO-2.1 free-form `String` aliases `Assumption`, `EvidenceRef`, `ContextRef` are promoted to **strict newtypes** carrying kind tags + structured fields (`AssumptionStatus`, `EvidenceKind`, `ContextKind`). `From<&str>` / `From<String>` / `AsRef<str>` impls keep every existing call site compiling.
- **§16 ACP Event Vocabulary (DEFERRED).** New `vesper-acp/vro_events.rs` module defines the 13-event PRD §16 vocabulary (`reasoning.profiled`, `reasoning.strategy_selected`, …, `reasoning.completed`) and translates each event into an ACP `AgentMessageChunk` notification so upstream clients (Zed, IDEs) receive standardized orchestrator state changes. PRD §16: "Where ACP has no dedicated event, VRO events should be translated into existing session update or status mechanisms."
- **§10.4 Provider Rate-Limit Accounting (DEFERRED).** New `RateLimitTracker` (atomic-backed, `Arc`-shared between provider adapter and orchestrator) catches HTTP 429s and halts the GVR loop with the new `OutcomeStatus::RateLimitExceeded` outcome instead of crashing on a network error. The default `untracked()` tracker preserves byte-identical VRO-9 behavior when no provider has been wired.
- **§22.4 Soak Tests (DEFERRED).** New `crates/vesper-agent/tests/soak_test.rs` with five `#[ignore]`-gated soak tests looping the orchestrator through 50+ back-to-back synthetic requests, proving memory safety, thread-leak prevention, repair-controller signature boundedness, rate-limit-tracker atomic-counter integrity, and cross-turn state non-corruption.

### VRO-11 — VesperLens Native Human-in-the-Loop Oracle (ADR 0017)

VRO-11 adds the first **native loopback HTTP oracle** to `vesper-agent`. ADR 0020 now places trusted review chrome around a sandboxed artifact iframe, binds a raw `tokio::net::TcpListener` on `127.0.0.1:0`, and awaits authenticated structured feedback before the explicit tool resumes.

- **Zero new external dependencies.** Built on raw `tokio::net::TcpListener` (no `axum` / `actix-web` / `hyper`). Only the `net` + `io-util` *features* are added to the existing workspace `tokio` pin — no version bump, no new crate.
- **Native Rust only.** No external web runtime or framework. Trusted chrome and the sandbox SDK are owned Rust-string assets.
- **Loopback-only, ephemeral ports.** Binds strictly to `127.0.0.1:0`; the OS assigns the open port. No wildcard bind, no public interface.
- **Pure-function HTTP parser.** `try_parse_request(bytes)` is a pure function with no network — the parser is unit-testable by feeding exact byte arrays (PRD §4: "mock the TCP byte streams").
- **Isolated and authenticated.** The artifact iframe has no same-origin authority. Only trusted chrome can POST feedback, using a UUID session route plus exact Host/Origin, JSON content type, and token-header checks.
- **Precise JSON contract.** Feedback includes stable annotation IDs, editable comments/replacement HTML, element or text-range targets, notes, planning answers, and explicit session ending. All returned strings remain untrusted user input.
- **Complete local artifacts.** Workspace-confined `.html`/`.htm` files are size-bounded and serve canonically confined sibling assets; repeated file rounds reuse the live session and revision polling reloads changed artifacts without discarding browser drafts.

### VRO-11.2 — Planner Seam + Context Injection + Registry Launch

VRO-11.2 originally introduced an orchestrator seam. ADR 0020 supersedes the unused orchestrator field/methods; only the explicit TUI tools own review now:

- **`LensReviewPort` trait** (`crates/vesper-agent/src/vro/lens_integration.rs`) — abstracts the lens so the orchestrator stays pure. The composition boundary (TUI binary) supplies a concrete impl wrapping `VesperLens::review_artifact`. Includes `NoOpLensReviewPort` (returns `LensFeedback::default()` immediately, no I/O).
- **No dormant final-output interception.** `VroOrchestrator::with_lens_port` and `maybe_review_html_artifact` were removed because they had no production caller and contradicted explicit invocation.
- **`feedback_as_context_message(&feedback)`** — token-frugal context-injection renderer (verdict + notes + numbered annotations). The host injects this as a `role: Tool` message so the next model turn can apply the human's corrections (PRD §4).
- **Registry launch.** New `scripts/publish_to_acp_registry.sh` opens (or updates) a **brand-new** `agent-vesper` PR against `agentclientprotocol/registry`. The legacy `scripts/acp_pr_439.md` is deleted — PR #439 belongs to the `native-glm-acp` Python project and is intentionally left untouched.

### VRO-11.3 — TUI & UX Hotfix (Bracketed Paste, Live Telemetry, Autocomplete Disconnect, File-Save Interceptor)

VRO-11.3 is a surgical hotfix closing four TUI/UX gaps exposed by the dashboard-generation test. All four are presentation/interception patches — the core ReAct loop is unchanged:

1. **Bracketed Paste Mode (input shattering fix).** `enter_raw_mode` / `leave_raw_mode` queue `EnableBracketedPaste` / `DisableBracketedPaste` alongside the existing mouse-capture commands. The main event loop handles `Event::Paste(text)` as a **single contiguous insertion** at the composer cursor — multi-line clipboard content is no longer shattered into individual `Char` / `Enter` events that would submit on the first embedded `\n`. The user submits with a deliberate bare `Enter` after the paste, exactly like the oracle composer.
2. **Live Tool Telemetry (blind-execution fix).** `TrajectoryCapturingInvoker` now broadcasts `⏳ *Executing* \`<tool>\`...` to the Reasoning Panel the **instant** the agent requests a tool, BEFORE the tool runs — mirroring Codex / Claude Code's "the agent is acting" affordance. The matching `↳ OBSERVATION` / `✗ ERROR` line streams second when the tool returns. No more staring at a frozen panel during a slow `write_file`.
3. **Autocomplete Disconnect.** `/reasoning` is disconnected from the legacy `/thinking` alias in the palette UI. Typing `/reasoning ` now surfaces the VRO mode family (`set mode=auto|fast|balanced|deep|maximum|off` + `clear`) instead of leaking the GLM thinking-style levels (`disabled`/`enabled`/`high`/`max`). The backend backward-compat fall-through (`/reasoning <level>` → thinking superpower) is intentionally preserved — only the autocomplete surface changes.
4. **VesperLens file-save interceptor (VRO-11.3).** ~~`lens_integration.rs` gains `html_artifact_for_write_file(name, arguments)` and the `LensObservingInvoker<'a>` decorator. After every successful `write_file` to an `.html` path, the decorator routes the content through `LensReviewPort::review` and the React loop **halts** mid-turn for human review.~~ **Replaced by VRO-11.4** — see below.

**Verification:** `cargo xtask verify` green; workspace `cargo test --workspace --all-features` is **1028 pass / 0 fail / 10 ignored** (+19 new tests over the VRO-11.2 baseline of 1009).

### VRO-11.4 — Local Recon & UX Overhaul (Inline Telemetry + Explicit `request_human_review` Tool)

VRO-11.4 is an architectural course-correction driven by **architectural analysis** . The recon proved that VRO-11.3's implicit `LensObservingInvoker` was an anti-pattern — The analysis showed zero interception and relies entirely on the model explicitly invoking an explicit review command when it wants human review. VRO-11.4 aligns Vesper with this proven architecture:

1. **Inline Telemetry.** Tool execution logs are ripped out of the Reasoning sidebar and rendered **DIRECTLY in the main Conversation panel**. A new `TuiSession.live_trajectory` field collects per-turn tool telemetry from both the direct path (`ToolStarted` / `ToolFinished` → `> 🛠️ Executing: <name>...` / `> ✓ <name>`) and the ReAct trajectory stream (`> ⏳ *Executing* ...` / `> **▶ ACTION**` / `> *↳ OBSERVATION*`). The trajectory now reads top-to-bottom naturally with the assistant's text, matching Codex / Claude Code host-agent rendering.
2. **Explicit `request_human_review` tool.** The implicit `LensObservingInvoker` (VRO-11.3 directive 4) is **DELETED**. VesperLens review is now triggered by an **explicit tool** the model calls when it wants human review — matching the explicit-invocation pattern. The `TuiToolService` advertises `request_human_review(file_path)` when a lens port is configured. The tool reads the file, routes it through `LensReviewPort::review`, **blocks** until the human submits, and returns the verdict as the tool result.
3. **Historical orchestrator wiring.** VRO-11.4 wired both surfaces to close a silent bypass. ADR 0020 later removed the unused orchestrator seam; `TuiToolService` is now the sole explicit owner.
4. **`LensReviewPort` trait lifetime fix.** The `on_url` parameter is now tied to the `'a` lifetime of `&self` so concrete impls can call `on_url` from within the returned async block (needed because `VesperLens::review_artifact` calls `on_url` mid-async when the TCP listener binds).

**Verification:** `cargo xtask verify` green; workspace tests **1028 pass / 0 fail / 10 ignored** (11 deleted interceptor tests replaced by 11 explicit-tool + inline-telemetry tests).

### VRO-11.5 — Claude Code UI & System-Prompt Enforcement (Single-Column Layout, Inline Thinking, Tool Mandate)

VRO-11.5 closes the gap a 180-second **zero-tool turn** exposed: the model announced a plan, yielded its turn, and never called `write_file` / `request_human_review` — while the dashboard-style UI (bottom Reasoning panel, TODO panel, Activity strip) made it look like the harness was ignoring the user. The architecture was sound; the layout and the prompt were not.

1. **UI declutter — single conversation column.** The bottom **Reasoning panel**, the sidebar **TODO** panel, and the sidebar **Activity** strip are **removed**. The Conversation panel now takes the full body height (F4 working-tree view still overlays when opened); the sidebar keeps only Session + Run report. PageUp/PgDn/Home/End and the mouse wheel always scroll the conversation — the Tab panel-focus toggle is gone with the panel it focused.
2. **Inline thinking stream.** The provider's raw chain of thought (GLM `reasoning: max`, Qwen3/DeepSeek-R1 `reasoning_content`, and the LM Studio stream) renders **directly in the Conversation feed** as a dim italic block under a compact `🧠 Thinking · strategy · mode · risk` header (with live phase and ⚠ risk-escalation marker). Only the newest `14` reasoning lines stream (long thinks stay readable), and the block collapses away when the turn completes — exactly Claude Code's live-thinking behavior. **F2** toggles it.
3. **Tool telemetry parity (⏺ / ⎿).** Direct-path tool events render with Claude Code's glyphs: `> ⏺ write_file` when the tool starts, `> ⎿ ✓ write_file` when it finishes (✗ on failure) — dim, inline, one column.
4. **Tool-execution enforcement (system prompt).** Every shared-`AgentLoop` path requires real `write_file` execution and forbids plan-only yielding. ADR 0020 narrows browser review to requested or materially useful workspace-confined HTML; ordinary source code and deterministically verifiable HTML are not forced through VesperLens.

**Verification:** `cargo xtask verify` green; enforcement + inline-thinking tests added in `agent-vesper-tui` and `vesper-agent`.

### VRO-11.6 — Review UX Parity (Claude Code Telemetry, Clickable URL + Ctrl+O, Interactive Lens Overlay)

VRO-11.6 closes the three gaps the first live VRO-11.5 test exposed: telemetry that still looked like block-quoted noise, a review URL the terminal refused to make clickable, and a review overlay that annotated through a crude native prompt dialog.

1. **Exact Claude Code telemetry shape.** The `> ` quote prefix is gone. Tool events now render exactly like Claude Code: `⏺ write_file` flush-left for the action, `  ⎿ ✓ write_file` indented for the result (✗ on failure) — dim, quiet, one column. The ReAct path's `▶ ACTION` / `↳ OBSERVATION` / `✓ FINISH` markdown labels are retired for the same `⏺` / `⎿` shapes.
2. **The review URL is finally openable.** The `[VesperLens]` announcement now sends the **bare URL on its own line** (own-line + no wrapping is what terminal auto-linkification needs), rendered cyan + underlined as an obvious link. And for any terminal that still refuses: **Ctrl+O** opens the most recent review URL in the system browser (`xdg-open` / `open` / `start`), with the status line advertising it the moment a review goes pending. Fails loudly with a copyable URL if no opener exists.
3. **Oracle-style interactive overlay.** The VesperLens review page drops `window.prompt` entirely: **pick mode** outlines elements on hover, a click opens an **inline popover editor** anchored at the click, **text selections are quotable annotations** (the selection is embedded in the note), annotations live in a removable numbered list (✕ per item), Esc exits pick mode, Enter confirms the popover. Approve / Request changes submit the same `{action, annotations, notes}` contract as before — no backend change.

### VRO-11.7 — Clickability & TODO Restore (Historical; Superseded)

VRO-11.7 fixes what the live v0.20.36 test exposed: the review URL rendered **twice** (embedded in the message line and as the bare line) and **neither was clickable**, and the TODO list removal had overshot.

1. **Historical mouse-off experiment.** This release made mouse capture opt-in; VRO-11.9 supersedes that default because alternate-screen wheel events require mouse reporting.
2. **One URL, not two.** The VesperLens announcement is now a message line without any URL plus a **single bare-URL line** — the sole, linkifiable copy (cyan + underlined), with **Ctrl+O** still guaranteed to open it.
3. **Historical inline TODO.** This release put plan snapshots into chat; VRO-11.11 supersedes that behavior with the dedicated live TODO sidebar.
4. **Historical pick-first review.** This release booted directly into annotation capture; VRO-11.11 supersedes it with interaction-first review so artifact controls remain usable.

### VRO-11.8 — Rich Telemetry, Guaranteed TODO, Wipe-Proof Overlay

VRO-11.8 closes the three gaps from the live v0.20.38 test:

1. **Telemetry is rich, not bare names.** Tool events carry a secret-safe display digest: `⏺ write_file · dashboard.html` and `  ⎿ ✓ write_file · 43 lines`. Hints come only from whitelisted argument keys (paths, patterns, commands — never `content`/`body`/credential keys, 48-char cap); success notes are size digests only, so file bytes never reach the progress stream.
2. **The TODO list is guaranteed.** The tool-enforcement instruction mandates `update_plan` for multi-step work in every mode. VRO-11.11 keeps that guarantee but renders current task state in the dedicated sidebar instead of appending snapshots to chat.
3. **The overlay survives artifact JS.** Dashboards that rebuild `document.body` after load no longer kill the review UI: pick listeners attach at `document` level (capture phase) and an 800ms watchdog re-attaches the panel if the page removes it.

### VRO-11.9 — Wheel Restored + In-App Clickable Links + Silent Browser Spawn

VRO-11.9 fixes the two regressions the live v0.20.39 test exposed:

1. **Mouse wheel scrolls again.** VRO-11.7's capture-off default had a hidden cost: in the alternate screen, terminals deliver **no wheel events at all** to apps without mouse reporting — so the wheel went dead (only PageUp/Down worked). Mouse capture is back **ON by default**; users who prefer native terminal selection can still toggle it off in settings (Shift-drag also selects in most terminals while reporting is on).
2. **The link is clickable in-app.** Since the app owns the mouse again, it now owns linkification: clicking the cyan review-URL line opens the browser directly — the click is inverse-mapped through the exact same render pipeline that drew the line. No terminal support required. **Ctrl+O** remains the keyboard path.
3. **Ctrl+O no longer wrecks the screen.** The "errors" on Ctrl+O were the default browser's own stderr (harmless Chromium `atom_cache` / GCM logs) — but the spawned browser inherited the TUI's stdio and sprayed those lines over the interface. The opener now spawns with **null stdio**; browser noise can never touch the display again.

### VRO-11.10 — Review Overlay Survives Artifact CSP

The reviewed copy strips artifact-authored `<meta http-equiv="Content-Security-Policy">` tags before injecting the owned Lens script. The source file remains untouched; the browser review can no longer render with silently disabled controls because a generated CSP omitted inline scripts.

### VRO-11.11 — Automatic Interactive Review + Browser Planning Interview

VRO-11.11 closes the remaining gap with the reviewed Lavish workflow: opening a Lens session now creates an immediate, genuinely interactive human handoff instead of leaving the TUI stuck on `WORKING` while the user hunts for a URL.

1. **Automatic browser handoff.** `request_human_review` and `request_human_input` open the loopback review URL with the platform browser as soon as the listener binds. The bare URL, in-app click target, and Ctrl+O remain reliable recovery paths when desktop opening is unavailable.
2. **Artifacts work before annotation.** VesperLens starts in interaction mode, so dashboard buttons, forms, links, and other native controls remain usable. Annotation is an explicit toggle. The overlay exposes all three native verdicts (Approve / Send changes / Reject), preserves draft notes across panel rerenders, and treats non-2xx submissions as failures.
3. **Interactive planning questions.** The model-facing `request_human_input` tool accepts bounded questions with optional choices. `/interview-limit` reports or changes the session policy: `1`–`12` sets a hard maximum, `auto` lets the agent choose 1–12 questions from the PRD's unresolved decisions, and the default remains 4. VesperLens renders escaped free-text, radio, or checkbox controls, requires every answer before submission, and returns stable `question: value` pairs to the model as tool context so planning can continue without invented requirements.
4. **Terminal-native conversation and TODO state.** User turns use a compact cyan `›` marker, assistant markdown is unboxed, and tool/thinking lines stay dim; the old full-width/asymmetric chat bubbles are gone. Repeated `update_plan` snapshots no longer enter transcript history. Wide terminals show current task state in a dedicated, toggleable TODO sidebar between a compact Session summary and a compact Run/Last run panel; `/tasks` controls it.

### VRO-11.12 — Isolated, Authenticated, Resumable Review (ADR 0020)

1. **Trusted chrome, sandboxed artifact.** Verdict controls live outside a no-same-origin iframe; artifact JavaScript cannot directly approve, reject, or manipulate the review panel.
2. **Authenticated loopback session.** UUID routes, Host/Origin checks, JSON-only feedback, a custom token header, CSP, request bounds, and workspace/file confinement close the local trust-boundary gaps.
3. **Iterative artifact review.** Canonical file sessions queue feedback across cancelled waits, reuse their URL for later rounds, poll file revisions, preserve drafts in browser session storage, and serve confined sibling CSS/JS/images/fonts.
4. **Precise feedback and richer interviews.** Stable annotation IDs, exact text-range anchors, editable suggested HTML, clean highlight removal, optional questions, help text, recommendations, and Other answers are native typed contracts.
5. **Conditional review.** File creation remains mandatory; browser review is requested only for workspace-confined HTML when the user asks or visual/interaction choices materially need human inspection.
6. **Passive diagnostics.** Bounded overflow/clipping warnings never wake the agent and enter feedback only when the reviewer confirms them.
7. **Real-browser gate.** The artifact-review and interview Playwright scripts drive Chrome and fail on console, network, resource, sandbox, interaction, validation, annotation, or submission regressions.

### How VRO interacts with the live tool surface

- The `tool_grounded_react` strategy (VRO-5) routes through a real LM Studio `ReactAgent` bundle when configured — `Actions` and `Observations` stream live into the Conversation panel as `⏺ <tool>` / indented `⎿ <result>` lines, alongside the inline thinking header above.
- The other nine strategies execute in the background; only the LEARNED notice and the final answer land in the conversation feed.
- VRO is **off by default** (PRD §21 — zero behavior regression when disabled). It activates only when `[reasoning] enabled = true` is set in the runtime config OR a `/reasoning set mode=<X>` override is in force.

## Skill Library

Agent Vesper ships a curated cross-project skill library at
`~/.agent-vesper/memory/skills/` (93 skills across 14 category bundles —
software development, GitHub workflows, research, documents, creative and
more). **The installers seed it automatically**: a fresh install lands with
the full suite, upgrades add newly curated skills, and your own edits and
deletions are never overwritten (a seed manifest tracks what was offered).
The library is a **read layer**: every project sees it, project-local
skills shadow same-named global ones, and `learn_skill` always writes
project-locally. Skill bodies are bounded at 200 KB; `read_skill` can pull a
single `section` or line window instead of a whole skill; `list_skills`
surfaces each skill's frontmatter description. Override the library root
with `AGENT_VESPER_GLOBAL_MEMORY_ROOT`.

## Configuration

All configurable from the TUI — no restart needed. Every provider control is **advertisement-driven and capability-gated** (`docs/provider-capability-gating-prd.md`): rows appear only when the active provider advertises them, value lists come from the provider's own descriptors narrowed by its policy for the active model, and unsupported features (image input on non-vision models, Mixture of Agents without eligible advisers, thinking on models that report no reasoning options) are disabled or hidden with truthful reasons — never a provider-name check:

| Option | Values | Description |
|---|---|---|
| **Model** | GLM: GLM-5.3, GLM-5.2, GLM-5-Turbo, GLM-4.7 (+ vision models) · LM Studio: live `/api/v1/models` list | Model list comes from the active provider's catalog, filtered by plan (GLM) or reported availability (LM Studio) |
| **Reasoning Depth** | GLM: Off, Enabled, High, Max (deep levels on deep-reasoning models only) · LM Studio: only the model's reported reasoning options | Provider superpower (`/thinking`); value set is policy-filtered per active model |
| **Reasoning Mode (VRO)** | Auto, Fast, Balanced, Deep, Maximum, Off | VRO orchestrator mode override (`/reasoning set mode=…`); see [Reasoning Orchestrator](#reasoning-orchestrator-vro) |
| **VesperLens interview limit** | Auto or 1–12 (default 4) | Session-scoped maximum for `request_human_input`; `/interview-limit` reports or changes it |
| **API Plan** | Coding, Standard, BigModel (CN) | GLM endpoint plan (advertised by the GLM adapter; hidden for providers without plans) |
| **Permissions** | Ask, Read Only, Bypass | Gate destructive tools |
| **Generation** | Balanced, Precise, Exploratory | Temperature / sampling strategy (advertised by the provider) |
| **Auxiliary Model** | Main + eligible models | Bounded auxiliary work model; ineligible (vision / off-plan) values are policy-filtered out |
| **Mixture of Agents** | Off, Enabled | Enabled only when the active provider fields eligible adviser models (tool-capable, non-vision); single-model providers see Off only |

### Cognitive Memory Configuration

| Env Var | Default | Description |
|---|---|---|
| `AGENT_VESPER_COGNITION_ROOT` | `.agent-vesper/cognition/` | Current project's cognitive SQLite store (existing location retained for compatibility) |
| `AGENT_VESPER_GLOBAL_COGNITION_ROOT` | `$XDG_DATA_HOME/agent-vesper/cognition/` or `~/.local/share/agent-vesper/cognition/` | Cross-project cognitive SQLite store |
| `AGENT_VESPER_COGNITION_USER_ID` | `local` | Scope identifier for memory partitioning |
| `AGENT_VESPER_COGNITION_MODEL` | `glm-4.6` | Extraction LLM model |
| `AGENT_VESPER_COGNITION_EMBEDDING_API` | (local hash) | Set to `bigmodel` for BigModel neural embeddings (requires BigModel CN JWT auth) |
| `AGENT_VESPER_COGNITION_EMBEDDING_MODEL` | `text-embedding-nomic-embed-text-v1.5` | LM Studio embedding model name (used when LM Studio is the active provider) |
| `LMSTUDIO_API_KEY` | (none) | Optional bearer token for a metered LM Studio embedding endpoint |

Smart routing sends identity and stable preference phrases (for example, “my
name” or “I prefer”) to the global store, sends repository/runtime/build facts
to the project store, and conservatively keeps ambiguous facts project-local.
Every `/remember` prints the selected scope and routing reason. Explicit flags
override the heuristic, while `/promote` and `/demote` repair a decision without
deleting and retyping the memory. Automatic recall searches both stores.

#### Scoped memory lifecycle (v0.20.45)

Project memory keeps the existing `.agent-vesper/cognition/` database, so the
upgrade does not move or rewrite prior data. Global memory uses the platform
data directory and follows the user across projects on the same machine.
Transfers are copy-verified before source deletion and report the destination
ID because each store assigns its own memory ID:

```text
✓ Moved [d3b53280] from global to project memory as [40d30a6a]
```

Use the displayed destination ID for a later `/promote`, `/demote`, or
`/forget`. `/memories [query]` shows both stores and their current IDs.

### Embedding Strategy

The cognitive memory engine uses the provider-independent
`.agent-vesper/cognition/embedding.json` configuration when present. Only
installations without that file retain the legacy active-chat-provider routing:

| Active provider | Embedder | Behavior |
|---|---|---|
| **LM Studio** + settings | `LmStudioEmbedder` | **Real neural embeddings** via LM Studio's `/v1/embeddings` endpoint. True semantic cosine: "do you remember me" ↔ "user's name is Alex" matches because the underlying text-embedding model captures meaning. |
| ZAI + `AGENT_VESPER_COGNITION_EMBEDDING_API=bigmodel` | `BigModelEmbeddingAdapter` | BigModel CN neural embeddings (requires JWT auth) |
| ZAI (default) or no provider | `LocalHashEmbedder` | Zero-network bag-of-words fallback. Cosine only fires on lexical overlap. |

**Automatic migration**: when the configured embedder changes, the engine
compares the new model name against the `embedding_model` row in
`cognition_meta` and re-embeds every memory + entity in chunks of 256 if they
differ. The startup log reports the migrated counts and target model.

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
