# Executive Verdict

Status: COMPLETE

## 1. Is the source directory correct?

Yes. `/home/alex/Projects/Native GLM-5.2 Provider` is the expected completed Native GLM ACP harness. Evidence: remote `99percentgrip/Native-GLM-ACP`, package `glm-acp`, version 2.7.34, ACP implementation `GlmAcpAgent`, Z.ai `GlmClient`, extensive tests, five-target release automation. The working tree has one pre-existing untracked user document, which reconnaissance did not touch.

## 2. Is the target ready?

It is clean enough for a deliberate migration but not initialized as a software repository. At first inspection `/home/alex/Projects/Agent Vesper` contained only its DOX `AGENTS.md` and was not a Git repository. Reconnaissance added only documentation/DOX records. Before production code: initialize/version-control policy must be approved, fixture/ADR gates completed, and workspace foundation explicitly authorized.

## 3. Real source size and complexity

Approximately 28K tracked production Python lines plus 16.5K test lines (44,487 total), 879 collected tests, and major 7,235-line agent, 5,016-line TUI, and 2,082-line tools modules. It includes ACP, terminal frontends, provider streaming, tools, persistence/FTS, policy/sandbox, MCP/browser, checkpoints, workers/worktrees, learning/awareness/deliberation, cron, plugins/signatures, observability, mobile approval, voice, packaging and uninstall. This is a multi-year-style harness surface, not a client rewrite.

## 4. What is GLM-specific?

Z.ai credentials and official quota host rules; Coding/Standard/BigModel endpoints; GLM catalog/limits; thinking/clear-thinking/reasoning-effort/preserved reasoning; GLM SSE field conventions/tool streaming/cache usage/errors; current continuation prompt/cap; plan-specific vision limitations. These belong in `vesper-provider-glm`.

## 5. What belongs in Vesper core?

Provider-neutral messages/events/capabilities/errors; the single turn/tool loop; session/lineage/plan/goal state; permission orchestration; verification/guardrails/completion; compaction policy; context/repository intelligence; tool registry; ports for tools/persistence/MCP/workers/automation/memory/observability. ACP and TUI are adapters, not core.

## 6. Recommended Rust architecture

A ports-and-adapters workspace:

- small `vesper-domain`, `vesper-provider`, `vesper-security`, `vesper-policy`, `vesper-config`;
- one `vesper-core` turn engine and `vesper-runtime` composition root;
- cohesive service crates for tools, sessions, context, checkpoints, memory, workers, MCP, automation, plugins, observability;
- separate provider adapters;
- thin ACP/CLI/TUI frontends over sequenced commands/events.

Use session actors and hierarchical cancellation, not global state or an uncontrolled `Arc<Mutex<...>>`. Keep providers/core/frontends acyclic.

## 7. What must remain backward compatible?

- Python session JSON schema 1, visible replay, lineage, settings and reasoning policy.
- Project `.glm-acp` memory/skills/bundles/evaluation files.
- Checkpoint schemas 1/2, Git object identities and conflict rules.
- Cron version 1 jobs/claims/artifacts.
- Plugin schema/hash/Ed25519 trust.
- Credentials/profile isolation and uninstall preservation.
- ACP protocol-v1 lifecycle/event semantics and GLM requests/streams.
- CLI aliases, five target artifacts, installer checksum/provenance behavior.
- TUI user operations/commands/accessibility unless an approved UX change exists.

Derived SQLite indexes need semantic/rebuild compatibility, not byte identity.

## 8. What should be redesigned?

Split `agent.py`, `tools.py`, and `tui.py`; replace concrete GLM clients with capability-driven provider ports; introduce versioned domain events/session writes; strengthen path TOCTOU and `mcp.json` atomicity; separate argv from explicit shell execution; supervise process trees natively; make TUI a pure reducer; isolate auxiliary requests behind the same provider abstraction. Do not translate file-for-file.

## 9. Safest order

Decisions and fixtures → domain/provider contracts → GLM adapter and ACP adapter → read-only legacy sessions/session actor → minimal loop → tool registry/filesystem/process/security/policy → verification and writes/search → context/compaction → MCP/checkpoints/workers → memory/learning/automation/plugins/observability → TUI → packaging/cross-platform → GLM parity → multi-provider adapters.

## 10. Largest risks

Hidden coupling, omitted features, persistence/session loss, event/cancellation regressions, process leakage, path/sandbox weakening, provider leakage, TUI/accessibility loss, ACP/MCP SDK churn, cron/checkpoint/plugin compatibility, cross-platform packaging, and untested performance assumptions.

## 11. Tests required before implementation

Before high-risk production modules:

1. language-neutral ACP/provider/session/tool/security fixtures and Python oracle;
2. exact GLM request/SSE/event/cancel/retry corpus;
3. official ACP Rust SDK protocol-v1 spike;
4. session v1 and all persisted-format decode corpus;
5. permission algebra and secret canaries;
6. process-tree/sandbox real-platform conformance;
7. patch/checkpoint/worker fault-injection rollback;
8. SQLite FTS five-target packaging/search spike;
9. reducer-first TUI event fixtures and command/binding matrix;
10. baseline performance measurements before speed claims.

## 12. Is migration safe to begin?

Planning and disposable read-only spikes are safe. Production Rust feature implementation is not yet safe because the cross-language fixture oracle, ACP SDK spike, process/sandbox conformance, SQLite packaging spike, and explicit product decisions do not yet exist. The isolated Python full-suite attempt also stalled at `tests/test_agent.py::TestConfigSwitch::test_auxiliary_model_switch_and_plan_fallback` after 24 passes and was interrupted; the cause is unresolved and the baseline cannot be called green from this mission.

## Precise blockers

1. Approve compatibility/product decisions listed in Master Stage 0.
2. Create and approve the language-neutral fixture schema/capture runner.
3. Prove official ACP Rust 2.0 crate behavior against protocol-v1 transcripts.
4. Prove process-tree cleanup and sandbox capability truth on supported platforms.
5. Prove SQLite FTS5 packaging/search on all five release targets.
6. Diagnose or reproduce the isolated full-suite stall; obtain a completed source baseline.
7. Explicitly authorize repository initialization and production Rust work.

## Final status

READY WITH BLOCKERS
