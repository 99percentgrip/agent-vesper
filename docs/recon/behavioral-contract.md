# Behavioral Contract

Status: COMPLETE

## Scope

This specification describes observable Native GLM ACP behavior without prescribing Python structure. Evidence paths refer to `/home/alex/Projects/Native GLM-5.2 Provider`. Rust parity tests must target these state machines and event sequences.

## 1. ACP process and connection

### Inputs

JSON-RPC/ACP over stdin/stdout; initialization protocol version, client capabilities/info; terminal-auth requests; session and prompt requests; cancellation; permission outcomes.

### Initialization transition

`Created → Connected → Initialized`. `on_connect` stores the client connection (`agent.py:1839-1841`). `initialize` may start background MCP discovery and cron tasks, then returns:

- the client-provided protocol version;
- load-session support;
- prompt image and embedded-context support, no prompt audio;
- list/resume/close/fork/additional-directory session capabilities;
- implementation identity/version;
- terminal auth `zai-api-key-setup` with `--setup`.

Evidence: `agent.py:1842-1902`; tests: `tests/test_agent.py`.

Background startup must not delay initialization on slow custom MCP discovery (`agent.py:1849-1858`; `tests/test_jit_tools.py`). Shutdown must cancel/await background tasks, close provider clients, MCP, and diagnostics (`agent.py:1801-1837`).

### Errors and concurrency

Malformed JSON-RPC/ACP is SDK-owned. Session prompts and configuration changes for the same session serialize through one prompt lock (`agent.py:2182-2191`, `:2356-2365`). Different sessions may progress concurrently. Connection callbacks must never expose credentials.

## 2. Session state machine

### States

`Absent`, `Loaded/Idle`, `Prompting`, `Closing`, `Persisted-only`.

### Transitions

| Input | Preconditions | Transition and effects | Observable result |
|---|---|---|---|
| New | Valid cwd/additional roots | Generate UUID; create default Session; persist; advertise commands | Session ID, modes, seven config options (`agent.py:1909-1939`) |
| Load | Any requested ID | Close stale provider client; deserialize if present else create fresh; recompute usage; replay visible history then plan then commands | Modes/config; user/assistant chunks precede plan/commands (`agent.py:1941-2006`) |
| Resume | Same as load | Same restore/replay semantics | Resume response (`agent.py:2031-2086`) |
| List | Optional cwd | Read metadata, optionally exact-filter cwd, newest-first storage order | SessionInfo list (`agent.py:2008-2029`) |
| Fork | Parent loaded | Deep-copy messages/plan and behavioral state; copy provider/policy settings and totals; set parent/root lineage; create new clientless session; persist | New ID/modes/config; parent untouched (`agent.py:2104-2180`) |
| Close | Loaded or absent | Cancel provider; acquire prompt lock; persist; close client; remove runtime state | Empty close response; history remains searchable (`agent.py:2088-2102`) |

Corrupt/missing stored JSON produces a fresh load/resume, not process failure (`session_store.py:233-245`). Session IDs are filename-sanitized (`:71-78`). The exact top-level session fields emitted by `Session.to_dict` are schema-compatibility fixtures (`agent.py:555-602`); `from_dict` must tolerate omitted legacy fields (`:603-673`).

## 3. Prompt lifecycle and message identity

1. Acquire per-session lock.
2. Extract text/images (`agent.py:2367-2377`).
3. If a slash command with no images, echo one user chunk, execute locally, emit optional agent chunk, and return `end_turn` with the supplied `message_id` as `user_message_id` (`:2379-2389`).
4. Plan approval changes mode, persists, emits current-mode update, optionally loads `.agent/plan.md`, and emits confirmation before model work (`:2391-2415`).
5. Empty text/no-image returns without a provider call (`:2417+`).
6. Save accepted image payloads under workspace `.glm-acp-images`; text-only models receive file-path guidance; vision-capable requests preserve multimodal content (`agent.py:6628-6757`).
7. Append/echo user input, refresh task/project context, run the provider/tool loop, persist state, and return a stop reason.

Cancellation may arrive while awaiting HTTP or tools. The session’s provider client is marked and the active prompt task is cancelled (`agent.py:2540-2550`; `glm_client.py:154-163`). Cancellation is idempotent. Cancellation during a command must also terminate its process tree; this is a required Rust improvement fixture because Python cancellation and timeout paths are not identical in all branches.

## 4. Normalized event order

### Model stream

Within a coalesced SSE batch:

1. reasoning delta callback;
2. content delta callback;
3. before any tool delta, force-flush pending reasoning/content;
4. first complete tool name emits tool-start/pending;
5. after request completion, assemble tool arguments by index.

Evidence: `glm_client.py:641-801`; integration tests in `tests/test_stream_integration.py`.

ACP mapping is:

- reasoning → `agent_thought_chunk` (`agent.py:6759-6761`);
- content → `agent_message_chunk` (`:6763-6765`);
- streamed tool identity → pending `tool_call` (`:6825-6839`);
- parsed path/line → location update (`:6848-6878`);
- command output → in-progress update, last 4,000 characters (`:6884-6893`);
- success → completed update with bounded diff/text/location (`:6895-6941`);
- failure → failed update with error capped at 2,000 characters (`:6943-6949`);
- provider usage → ACP `UsageUpdate` after every call (`agent.py:4057-4080`).

Tool call/result pairing is invariant: every stored `role=tool` message has the provider-issued call ID, and compaction must not orphan either side (`agent.py:3947-3952`; compaction tests).

### Replay

Load/resume iterates persisted message order, emits only non-empty `user` and `assistant` content, flattens text blocks, and skips system/tool internals (`agent.py:6790-6823`). Plan and available-command updates follow replay (`:1982-1988`, `:2062-2068`).

## 5. Provider request/stream state machine

`Idle → Requesting → Streaming → Terminal | Incomplete | Cancelled | Error`.

- Body fields and optional controls are defined at `glm_client.py:519-555`.
- Non-200 becomes `GlmApiError` with status/body prefix/Retry-After (`:682-689`).
- Non-data/blank and malformed JSON SSE lines are ignored (`:691-711`).
- `[DONE]` or any finish reason is terminal (`:700-703`, `:764-766`).
- EOF without terminal marker is incomplete (`:783-784`).
- Visible partial deltas prohibit retry to avoid duplication; preserve them and normalize finish to `network_error` (`:573-577`, `:593-597`).
- No-visible-output incomplete/transport failure retries at most three times; retryable HTTP set is 429/500/502/503/504 (`config.py:415-419`).
- Retry-After seconds/HTTP date is honored to 60s; otherwise randomized 75–100% of capped exponential delay (`glm_client.py:623-639`).
- `length` with no tools continues up to 20 times; cap is `continuation_limit`. Continuation is disabled for bounded auxiliary calls (`:169-230`).
- Usage merges cumulative attempts/continuations and keeps cache tokens (`:603-620`).

Provider cancellation must close/drop the active response promptly, emit no post-cancel deltas, and return a cancellation stop—not retry.

## 6. Tool loop

At each iteration (`agent.py:2836-3990`):

1. check cancellation;
2. compact if required;
3. inject explicit prompt references;
4. call provider with `search_tools` first plus stable ordered loaded schemas;
5. update usage/telemetry/context pressure;
6. if no tools, apply failure/unverified-edit/learning/completion guards before accepting completion;
7. if tools, store one assistant tool-call message;
8. validate arguments before permission;
9. show location, apply policy/permission;
10. optionally checkpoint;
11. execute; stream output; complete/fail ACP tool;
12. postprocess diagnostics/evidence/verification/telemetry/hooks;
13. append exactly one paired tool result;
14. repeat.

Iteration limit is session-configurable 1–1000, default 50 (`config.py:41-74`). Three identical consecutive tool batches inject corrective failures; a further identical batch halts (`agent.py:2972-3026`). Result-aware repeated failures/unchanged reads warn then halt (`guardrails.py:17`; `agent.py:3937-3965`). Exhaustion emits an explicit incomplete warning (`agent.py:3966-3989`).

Read/search batch operations may be concurrent; mutations and commands retain provider order (`tools.py:1730-1804`). A transactional patch set validates all roots, unique paths, SHA-256 preconditions, UTF-8, patch hunks, and syntax before writing; write failure rolls already-written files back in reverse order (`tools.py:1657-1727`).

## 7. Permissions and policy

Precedence is contractual:

1. Resolve paths for policy input.
2. Evaluate top-level policy and every workflow step.
3. `deny` is absolute, including Bypass.
4. Plan Mode: allow non-destructive research, generic MCP list/call, and plan artifact writes under `.agent`; deny other destructive operations.
5. Bypass: allow after policy.
6. Read Only: deny destructive tools absolutely.
7. Ask: allow read-only unless policy forces Ask; destructive/MCP calls need approval.
8. Optional smart reviewer may auto-allow only an exact `safe` judgment over redacted arguments within 12 seconds; all other results fall through.
9. ACP permission request offers allow-once and reject-once. Channel error fails closed; non-allow is denial.

Evidence: `agent.py:4772-5092`; `config.py:636-667`; tests in `test_agent.py`, `test_terminal_cli.py`, roadmap tests.

## 8. Context, compaction, and completion

Context pressure thresholds are 60/75/85%; one update per reached tier until pressure falls. At 85%, compaction:

- preserves the system message verbatim;
- keeps four newest messages without splitting a tool-call/result boundary;
- deterministically extracts goal, decisions, fixes, unresolved work, plan, edits, commands, verification, lineage;
- requests a thinking-disabled bounded summary from auxiliary model when its context fits, else main model;
- inserts a delimited summary between system and recent messages;
- commits only a valid non-empty result;
- persists a quality score and warns on a 0.15 decline.

Evidence: `config.py:483-541`; `agent.py:6300-6627`; `tests/test_compaction.py`.

Persistent goal completion requires fresh evidence for every criterion, no active contradiction, and fresh post-edit verification before auxiliary judging. Awareness records cannot cite arbitrary model data as evidence (`awareness.py:EpistemicLedger`; tests `test_awareness.py`). Deliberation critics receive objectives, redacted diff, fresh evidence, hypotheses, and completion metadata—not conversation/reasoning (`agent.py:1507-1762`; `deliberation.py`).

## 9. Persistence and recovery

Authoritative session JSON is atomically replaced; `.meta` and FTS index are derived (`session_store.py:193-229`). Index corruption/failure must not block session save/load/search; it is rebuildable. Stored reasoning obeys `GLM_ACP_PERSIST_REASONING` (`config.py:669-682`).

Cron claims are cross-process, expire/recover, and are token-owned. Updating/removing a running job is forbidden; pausing lets the current claim finish but blocks the next run (`cron.py:356-484`). Finish rejects stale ownership and writes bounded redacted artifacts before state mutation (`:576-643`).

Checkpoint rollback restores only paths whose current hashes equal recorded agent-produced post-hashes; any conflict aborts rather than overwrites (`checkpoints.py:599-670`). Patch-set and worker promotion rollback must preserve prior bytes on any failure.

## 10. MCP, workers, automation, and plugins

- MCP discovers configured HTTP/stdio tools, maps collision-safe local names to exact server/remote names, and reconnects expired HTTP/restarted stdio sessions (`mcp.py:McpManager`; `tests/test_mcp.py`, `test_jit_tools.py`).
- Worker delegation is depth one, read/search only, permission-gated, no MCP/credentials/execution/edit/delegation; budgets are shared across a parent turn (`config.py:23-28`; `agent.py:4208-4387`).
- Scheduled runs use a fresh non-persisted session, cannot recursively schedule, use contained script prechecks with scrubbed environment, renew claims, and enforce an inactivity watchdog (`cron_scheduler.py:64-316`; `tests/test_cron.py`).
- Plugins are declarative/data-only. Schema, path/extensions, hashes, optional signature/trusted publisher, and atomic install all validate before activation (`plugins.py:33-425`).
- Lifecycle hooks are argv-only, executable hash-pinned, workspace-scoped, scrubbed, bounded, and time-limited; invalid/failing hooks are ignored except an explicit pre-tool blocking result interpreted by the agent (`hooks.py:41-107`; `tests/test_extensions.py`).

## 11. TUI synchronization contract

The TUI is a reducer over ACP-like updates via `TuiClient` (`tui.py:1833-1886`). It must not call provider transports directly except the shared agent’s provider-usage facade. Configuration uses the same session methods as editors (`tui.py:2698-2813`). Prompt state is single-active with a queue; cancel, close, and session switching must not leak updates between session IDs.

Terminal cleanup is mandatory on normal exit, panic, and cancellation. Clipboard/voice/notification/editor/screenshot subprocesses use explicit invocation, bounded input/output/time, and scrubbed credentials. Native-mouse mode must symmetrically release and reacquire mouse capture. Screen-reader mode changes rendering/announcements without changing harness behavior. UI reducer and render snapshots belong in `vesper-tui`; business state remains in core.

## 12. Timeouts and cancellation matrix

| Operation | Current rule | Rust parity requirement |
|---|---|---|
| Provider HTTP | 180s client/read timeout | Explicit connect/request/read policy; cancellation token wins |
| Smart approval | hard 12s | Timeout closes reviewer client; fall back to user |
| Delegate | 180s, six iterations | Shared budget cancellation propagates to child |
| Command | default 120s, user positive override | Kill whole tree, await reap, drain bounded output |
| Hooks | clamp 0.1–10s, default 3s | Kill/reap on timeout |
| Cron agent | default 600s inactivity watchdog | Claim renewal stops and final status records cancellation/error |
| TUI shutdown | bounded three seconds by tested contract | Terminal restore must occur even if resource closure times out |

## Intentional-change approval points

Do not silently preserve or change these:

1. Session store path/profile layout and unversioned session JSON.
2. Reasoning persistence default-on.
3. Plan Mode’s generic MCP exception.
4. Bypass semantics.
5. Shell-string execution and platform sandbox truth.
6. GLM continuation prompt wording.
7. TUI command/keybinding behavior.
8. FTS tokenizer/search ranking.
9. Malformed SSE lines being silently ignored.

Each needs a fixture, documented decision, and explicit approval if behavior changes.

## Completion status

Major inputs, outputs, states, transitions, events, persistence, errors, cancellation, timeouts, concurrency, security, and user-observable behavior are specified. Minor slash-command text belongs to captured exact fixtures rather than this prose contract.
