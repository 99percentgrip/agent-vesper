# Parity and Differential Test Strategy

Status: COMPLETE

## Objective

Prove that Agent Vesper preserves observable behavior and security guarantees without requiring identical internals. The frozen Python commit `bf4d4287e2e3320aa3f09015f678e6169d520045` is the oracle until the GLM parity gate is signed off.

## Harness design

Build a language-neutral fixture package:

```text
fixtures/
  acp/*.jsonl
  provider/glm/{requests,sse,errors}/*
  sessions/v1/*
  tools/*/{input,workspace,expected}
  security/*
  tui/events/*
  benchmarks/repos/*
```

A Python capture runner and Rust runner consume the same scenario manifest and emit canonical JSON:

- monotonic event sequence with normalized volatile IDs/timestamps/paths;
- final session snapshot;
- persisted file manifest/hashes/modes;
- safe logs/telemetry;
- process and network observations.

Never compare live remote model text. Provider fixtures replay HTTP status/headers and arbitrary byte chunks. Live quality cases are a separate statistical gate.

## Comparison classes

| Class | Use | Examples |
|---|---|---|
| Exact-output | Wire/schema/text is contractual | ACP initialize JSON, tool schemas, request JSON, SSE normalized event sequence, permission options, error classes, persistence decode |
| Semantic parity | Platform/render variations are acceptable | search ranking ties, help formatting, TUI layout, diagnostics messages |
| Schema compatibility | Old/new records round-trip safely | session v1, checkpoint schemas 1/2, cron v1, plugin v1, telemetry JSONL |
| Security-invariant | Rust may be stricter but never weaker | containment, redaction, policy precedence, process cleanup, signature checks |
| Performance | Compare distributions and resource ceilings | startup, memory, render latency, search, cancellation |

## Fixture matrix

### ACP/protocol

- Initialize: protocol version echo; capability negotiation with absent/full client capabilities; auth method and agent info.
- New/load/resume/list/close/fork: missing/corrupt/legacy session, additional roots, cwd filter, nested lineage, close during idle/turn.
- Replay: system/tool omission, multipart text, empty messages, plan after history, commands after plan.
- Prompt: message ID echo, empty prompt, slash command, plan approval, text/image combinations.
- Event order: reasoning→content→tool-start→location→in-progress→completed/failed→usage→terminal.
- Cancellation at every await boundary and duplicate cancellation.

Evidence anchors: `agent.py:1842-2560`, `:6759-6950`; `tests/test_agent.py`, `test_quality.py`.

### GLM/provider

- Golden request for every plan/model/thinking/generation/tools/no-tools/vision/auxiliary combination.
- SSE split at every byte boundary, CR/LF variants, blank/comment/non-data lines, malformed JSON, missing fields, empty choices.
- Interleaved tool indices, fragmented IDs/names/JSON, missing names, invalid/empty args, generated IDs.
- Separate reasoning/content batches and preserved history.
- Usage-only chunks, missing usage, cache details, cumulative continuations.
- HTTP 401/429/500/502/503/504/non-retryable, numeric/date/invalid Retry-After.
- EOF before terminal with zero vs visible deltas; transport error before/after visible output.
- cancellation before connect/headers/mid-SSE/flush/continuation;
- length with tools/no-tools, 20-continuation cap, bounded auxiliary no-continuation.

Evidence: `glm_client.py:169-230`, `:519-801`; `tests/test_stream_integration.py`, `test_glm_client.py`.

### Session/persistence

- Golden schema-1 files with every field, minimal legacy, unknown fields, invalid bounded lists, reasoning on/off.
- Atomic interruption points and concurrent revision conflicts.
- metadata-sidecar fallback, FTS backfill/delete/browse/search/redaction, corrupted DB rebuild.
- lineage and tool-call/result preservation through compaction and fork.

### Tools

Each canonical schema and:

- UTF-8/CRLF/NUL/binary, line continuation, output caps;
- root/additional-root/absolute/relative/`..`/symlink escape and race;
- rg present/absent, gitignore, glob/regex errors, 500/200 bounds;
- write/edit mismatch/ambiguous/no-op;
- patch context/count/newline;
- patch set hash/syntax/duplicate/all-or-none/injected rollback failure;
- batch read 1/20/invalid/concurrency/output reduction;
- command stdout/stderr/binary/silent/nonzero/huge/timeout/cancel/grandchild pipe/process tree;
- diagnostics/LSP missing/crash/restart/position conversion;
- workflow cycles/unknown dependencies/nested denial/stop on failure.

Evidence: `tools.py:205-2082`; `tests/test_tools.py`, `test_extensions.py`, roadmap tests.

### Permissions/security

Cartesian table of session mode × policy effect × tool class × workflow nesting × smart-review outcome × approval-channel outcome. Include Bypass+deny and ReadOnly+ask.

Secret canaries traverse provider errors, command output, tool args, permission UI, hooks, telemetry, FTS, cron artifacts, worker transcript, MCP/browser, TUI export, logs. Assert absent.

Plugin/signature, checkpoint/conflict, promptware/delimiter, browser/mobile replay, and real OS sandbox cases follow `security-invariants.md`.

### Compaction/learning

- 60/75/85 pressure tier once/reset;
- too few messages, exact boundary, forced focus;
- system/recent/tool-pair partition;
- empty/error/oversize summary transaction rollback;
- auxiliary context fallback;
- evidence categories and quality decline;
- awareness staleness/contradiction/criteria completion;
- metacognitive direct-task restraint and profile thresholds;
- hypotheses two/three and stale evidence;
- repository node/edge/body bounds and freeze-at-edit;
- causal draft two-support/fresh/time-split/mutated/no-regression promotion.

### MCP/workers/automation/plugins

- slow discovery does not block ACP startup; stable gateway order; name collisions route exact origin.
- HTTP expiry and stdio process restart; malformed response/cancel/close.
- workers enforce depth/tool/token/time budgets and read-only surface; background cleanup and transcripts.
- cron schedule/DST/claim race/renew/stale recovery/pause-running/script-only/watchdog/silent/history.
- plugin schema/path/symlink/extension/hash/signature/trust/tamper/atomic replacement.

### Slash commands/TUI

Freeze the advertised command catalog and parser outcomes. Compare TUI reducers using canonical events, not pixels, then snapshot Ratatui buffers at fixed sizes. Cover queue, reasoning visibility, tools/plans/usage, session switching, settings, worktree views, mobile approval, images/voice capability status, notifications, clipboard, exports, screen-reader/theme/Vim/native mouse and terminal restore.

Current source tests: `test_tui.py` (3,017 lines) and `test_terminal_cli.py`.

## Differential runner normalization

Normalize UUIDs by encounter order; timestamps to relative monotonic offsets; workspace roots to `$WORKSPACE`; platform separators; randomized retry delays to asserted ranges; process IDs to tokens. Do not normalize event ordering, IDs linking calls/results, status, modes, permission outcomes, hashes, or redaction.

## State-machine/property/fuzz tests

- Stateful session command generator with invariants for lineage, lock serialization, persistence revision, and no post-close events.
- Arbitrary SSE chunk segmentation and JSON field omission.
- Arbitrary patch hunks and injected filesystem failure positions.
- Cron clock/claim interleavings with a fake clock.
- Permission decision algebra and policy nesting.
- Proptest for bounded parsers; cargo-fuzz for SSE, ACP JSON, patch, policy/plugin/session codecs.
- Loom only for small lock/channel algorithms, not the whole runtime.

## Live semantic quality

Reuse `benchmarks/cases.json` with repeated isolated runs. Compare task success, per-case regressions, median/P95 latency, tokens, tool failures, false completion and unsupported claims. Remote variance means no exact text comparison. Credentials and spend require explicit opt-in, matching `.github/workflows/quality.yml`.

## Gate order

1. Fixture schema and Python oracle approved.
2. Domain codecs exact.
3. GLM request/SSE exact.
4. ACP lifecycle/event exact.
5. Tool/security invariant pass on each platform.
6. Persistence dual-reader and migration rollback.
7. Core scenario semantic pass.
8. TUI reducer/snapshot/accessibility pass.
9. Full Python suite and Rust suite green.
10. Live GLM outcome evaluation no material regression.

## Intentional differences

Allowed without exact parity only when documented and approved:

- stronger descriptor-relative filesystem containment;
- atomic/private `mcp.json`;
- explicit argv command path;
- versioned Vesper store;
- different TUI layout with equivalent accessible operations;
- corrected source bugs backed by a failing Python fixture and accepted ADR.

No security weakening, lost user data, reordered ACP semantics, duplicate streamed output, or orphan process can be accepted as an intentional difference.

## Current baseline

Collection under isolated HOME/config/cache found **879 tests**. Full-suite result is recorded in the evidence index/source inventory after completion. The source test categories are broad, but a language-neutral golden corpus and real process/sandbox conformance suite do not yet exist; those are pre-implementation blockers for high-risk code, not for repository foundation.
