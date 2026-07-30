# Master Migration Plan

Status: COMPLETE

## Governing rules

- Frozen Python commit remains read-only oracle.
- No stage begins until its characterization fixtures exist.
- One provider-neutral loop; providers/frontends are adapters.
- New state never overwrites legacy state without dry-run, backup, rollback and approval.
- GLM parity precedes production multi-provider expansion.
- Each completion gate requires code review, targeted tests, security checks, and updated evidence/risk records.

## Stage 0 — Product decisions and fixture charter

- **Objective:** approve compatibility scope and canonical fixture schema.
- **Source/targets:** all reports; no Rust crate yet.
- **Prerequisites:** reconnaissance accepted.
- **Boundary:** decide store location/import, reasoning default, Bypass/Plan MCP, command shell, TUI command retention, MSRV/targets.
- **Tests/parity/security:** review scenario matrix and secret policy.
- **Risks/failure:** ambiguous choices cause rework; record ADRs, no state changes.
- **Gate:** fixture charter and intentional-difference list approved.

## Stage 1 — Repository/workspace foundation

- **Objective:** create Cargo workspace, CI, licenses, formatting/lint/audit, crate skeletons only when authorized.
- **Source:** `pyproject.toml`, workflows, registry/installers.
- **Targets:** workspace metadata, `xtask`, initial `vesper-domain/config/security/provider`.
- **Prerequisites:** Stage 0.
- **Boundary:** no feature placeholders pretending parity; pin toolchain/MSRV/dependency policies.
- **Tests:** build/test/clippy/fmt/deny/audit on five target families.
- **Parity/security:** artifact identity and secret-free CI.
- **Risk/rollback:** dependency churn; commits remain revertible, no user data.
- **Gate:** clean cross-platform empty workspace and ADRs.

## Stage 2 — Python oracle and language-neutral fixtures

- **Objective:** capture canonical ACP/provider/session/tool/security outputs.
- **Source:** `tests/*`, schemas in `tools.py`/`mcp.py`, Python behaviors.
- **Targets:** `fixtures`, Python read-only capture runner, Rust fixture schema in `vesper-domain`.
- **Prerequisites:** fixture charter.
- **Boundary:** deterministic mocks only; source runner must not write source/user state.
- **Tests:** schema validation, repeatability, normalization collision checks.
- **Parity/security:** canary secrets never appear.
- **Risk/rollback:** oracle captures implementation accidents; mark exact vs semantic.
- **Gate:** reviewed fixture corpus covers parity strategy critical path.

## Stage 3 — Shared domain contracts

- **Objective:** implement IDs/messages/content/tool/usage/finish/errors/events/session DTO versions.
- **Source:** `agent.py:377-673`, `glm_client.py` dataclasses, ACP/tool schemas.
- **Targets:** `vesper-domain`.
- **Prerequisites:** Stage 2.
- **Boundary:** no I/O/provider/ACP SDK types; opaque namespaced extensions.
- **Tests:** serde goldens, unknown fields, bounds, property tests.
- **Parity/security:** no secret-bearing Debug/Serialize for secret handles.
- **Risk/rollback:** premature traits; keep ports separate and signatures reviewable.
- **Gate:** exact domain fixture round trip.

## Stage 4 — Provider port and capability model

- **Objective:** implement normalized request/stream/capabilities/config/error/cancellation interfaces.
- **Source:** `glm_client.py`, provider abstraction report.
- **Targets:** `vesper-provider` and conformance kit.
- **Prerequisites:** Stage 3.
- **Boundary:** no concrete provider or core loop logic.
- **Tests:** fake adapters for native/emulated/unsupported/fallback, partial-output contract.
- **Security:** secret/endpoint scopes, redacted errors.
- **Risk/rollback:** lowest-common-denominator/leakage; second fake adapter proves extensibility.
- **Gate:** capability and stream conformance suite green.

## Stage 5 — GLM adapter

- **Objective:** reproduce Z.ai auth/catalog/request/SSE/usage/quota/retry/continuation.
- **Source:** `config.py:415-775`, `glm_client.py:111-804`, provider tests.
- **Targets:** `vesper-provider-glm`.
- **Prerequisites:** Stages 3–4; reqwest/SSE spike.
- **Boundary:** GLM fields remain inside adapter; custom parser; no tools/core.
- **Tests:** all provider fixtures with arbitrary chunking/cancel/retry.
- **Parity:** exact requests/events and semantic safe errors.
- **Security:** official quota host allowlist; secret canaries.
- **Risk/rollback:** duplicate deltas/reasoning loss; adapter is opt-in, no legacy writes.
- **Gate:** full GLM conformance and leak tests.

## Stage 6 — ACP SDK/wire adapter

- **Objective:** implement initialize/auth/session/prompt/cancel/event mapping.
- **Source:** `agent.py:1839-2560`, `:6759-6950`; ACP tests.
- **Targets:** `vesper-acp`, thin runtime fake.
- **Prerequisites:** Stage 3 and official SDK spike.
- **Boundary:** SDK types stop at adapter; callbacks enqueue commands and never block dispatch on future inbound traffic.
- **Tests:** process-level JSONL transcripts, malformed/batch, event order.
- **Parity/security:** exact protocol-v1 behavior, no stderr/stdout secret.
- **Risk/rollback:** crate churn/deadlocks; exact pin and wrapper.
- **Gate:** Python/Rust ACP fixture equality.

## Stage 7 — Session actor and persistence read path

- **Objective:** own mutable state per session and load/list/replay/fork/close legacy v1.
- **Source:** `Session`, `SessionStore`.
- **Targets:** `vesper-core::session`, `vesper-sessions::legacy`.
- **Prerequisites:** Stages 3, 6.
- **Boundary:** read-only legacy store first; immutable snapshots/events.
- **Tests:** v1 corpus, corrupt/unknown, lineage, concurrent commands.
- **Parity/security:** cwd/root/profile and reasoning privacy.
- **Risk/rollback:** corruption/stale writes; no writes yet.
- **Gate:** exact load/replay/fork fixtures.

## Stage 8 — Minimal provider-neutral turn engine

- **Objective:** user message → provider events → completion, no tools.
- **Source:** `_prompt_locked`, `_run_turn` no-tool path.
- **Targets:** `vesper-core`.
- **Prerequisites:** GLM, session actor, ACP.
- **Boundary:** ports only; hierarchical cancellation and sequenced events.
- **Tests:** content/reasoning/usage/finish/error/cancel/lock serialization.
- **Parity/security:** no provider metadata leakage.
- **Risk/rollback:** event/backpressure; feature behind test binary.
- **Gate:** no-tool ACP differential pass.

## Stage 9 — Tool catalog/JIT registry

- **Objective:** canonical schemas, safe search and stable ordered loading.
- **Source:** `tools.py:50-1117`, `jit_tools.py`, tests.
- **Targets:** `vesper-tools::catalog/registry`.
- **Prerequisites:** domain/core minimal.
- **Boundary:** schema/search only, no executors.
- **Tests:** >85% initial reduction, BM25/regex limits/latency/name collisions/order.
- **Security:** safe regex and bounded query/results.
- **Risk/rollback:** prompt cache drift; golden schema hashes.
- **Gate:** exact catalog/search fixtures.

## Stage 10 — Filesystem/search/patch tools

- **Objective:** read/list/search/grep/write/edit/patch/patch-set/batch.
- **Source:** `tools.py:1125-1972`.
- **Targets:** `vesper-tools`, `vesper-security::fs`.
- **Prerequisites:** security containment spike and Stage 9.
- **Boundary:** descriptor/capability paths; blocking I/O pool.
- **Tests:** complete tool matrix, races, failure injection.
- **Parity/security:** bounds/UTF-8/newlines/atomic rollback; stronger TOCTOU accepted.
- **Risk/rollback:** data loss; temp fixture only until gate.
- **Gate:** security invariant and differential tool pass.

## Stage 11 — Process supervisor, command, diagnostics

- **Objective:** command/LSP/hook execution with streaming, timeouts and cleanup.
- **Source:** `tools.py:1975-2082`, `diagnostics.py`, `hooks.py`, `os_sandbox.py`.
- **Targets:** `vesper-tools::{process,diagnostics,hooks}`, `vesper-security::sandbox`.
- **Prerequisites:** platform spike.
- **Boundary:** explicit argv and explicit shell intent; supervisor owns tree.
- **Tests:** grandchild/pipe/cancel/timeout/output, real sandbox backend matrix, LSP restart.
- **Parity/security:** scrubbed env and honest isolation status.
- **Risk/rollback:** process leaks; disabled outside tests until all platform gates.
- **Gate:** zero-survivor matrix on five targets.

## Stage 12 — Policy and permissions

- **Objective:** ordered policy, nested workflow closure, modes, ACP/mobile decision port, smart review.
- **Source:** `policy.py`, `agent.py:4772-5092`.
- **Targets:** `vesper-policy`, core permission state machine.
- **Prerequisites:** tools/core/ACP.
- **Boundary:** policy pure; frontend only returns decision; smart review uses auxiliary provider port.
- **Tests:** full Cartesian matrix and timeouts.
- **Parity/security:** deny/read absolute; reviewer redaction.
- **Risk/rollback:** bypass escalation; fail closed.
- **Gate:** exhaustive decision fixtures.

## Stage 13 — Verification, guardrails and completion

- **Objective:** edit generations, canonical checks, repeated-loop/failure/unverified-edit gates, goals/plans.
- **Source:** `verification.py`, `guardrails.py`, agent loop guards/handlers.
- **Targets:** `vesper-core::{verification,guards,goal,plan}`.
- **Prerequisites:** tool loop/policy.
- **Tests:** stale evidence, spoofing, repeated batches/results, iteration cap.
- **Parity/security:** model output cannot forge harness evidence.
- **Risk/rollback:** false completion/infinite loop; deterministic state fixtures.
- **Gate:** reliability/quality differential scenarios.

## Stage 14 — Session writes and search

- **Objective:** transactional Vesper store, legacy compatibility, metadata/FTS index.
- **Source:** `session_store.py`, Session v1.
- **Targets:** `vesper-sessions`.
- **Prerequisites:** Stage 7 model stable.
- **Boundary:** new schema/version and legacy reader; derived index.
- **Tests:** crash/concurrency/migration/rebuild/redaction.
- **Parity/security:** no legacy overwrite; private modes/ACL.
- **Risk/rollback:** data loss; dry-run/backups/new root.
- **Gate:** migration rollback and search parity.

## Stage 15 — Context references and compaction

- **Objective:** progressive instructions, bounded references, pressure and transactional compaction.
- **Source:** `project_context.py`, `references.py`, `agent.py:6300-6627`.
- **Targets:** `vesper-context`, core compaction coordinator.
- **Prerequisites:** provider auxiliary port/session persistence.
- **Tests:** partition/tool pairing/focus/fallback/error/quality/pressure.
- **Security:** containment, secret omission, untrusted delimiters.
- **Risk/rollback:** context loss; preserve original on every failure.
- **Gate:** compaction differential pass.

## Stage 16 — MCP

- **Objective:** configured/preset discovery/call/recovery and JIT routing.
- **Source:** `mcp.py`, MCP/JIT tests.
- **Targets:** `vesper-mcp`.
- **Prerequisites:** official SDK spike, policy/JIT/core.
- **Boundary:** MCP output untrusted; discovered tools permission-gated.
- **Tests:** slow startup, HTTP/stdio recovery, collision, cancel/close.
- **Security:** config/private writes, environment and credential scope.
- **Risk/rollback:** SDK churn/hanging children; feature-isolated.
- **Gate:** recovery and security fixtures.

## Stage 17 — Checkpoints and rollback

- **Objective:** schema-1/2 reader, schema-2-compatible or approved new writer, GC/conflict rollback.
- **Source:** `checkpoints.py`.
- **Targets:** `vesper-checkpoints`.
- **Prerequisites:** filesystem security/persistence.
- **Tests:** object vectors, exclusions, limits, retention, conflict/fault/migration.
- **Security:** never touch `.git` or capture secrets.
- **Risk/rollback:** destructive restoration; preflight all, transaction log.
- **Gate:** cross-language create/read/rollback and injected failures.

## Stage 18 — Workers and worktrees

- **Objective:** bounded read-only delegates, background reports, digest-gated worktree promotion.
- **Source:** agent delegation/worktrees/worktree_session.
- **Targets:** `vesper-workers`.
- **Prerequisites:** core/tools/policy/provider/checkpoints/verification.
- **Tests:** depth/budgets/timeout/cancel/digest/conflict/rollback/dirty cleanup.
- **Security:** no credentials/MCP/mutation for delegates.
- **Risk/rollback:** primary corruption/process leaks; transactional promotion.
- **Gate:** worker parity/security suite.

## Stage 19 — Memory, awareness, learning

- **Objective:** project/user memory, skills/bundles, epistemic state, metacognition, deliberation, repository intelligence, meta-learning/failure corpus.
- **Source:** corresponding modules/tests.
- **Targets:** `vesper-memory`, `vesper-context`, core services.
- **Prerequisites:** verification/context/provider auxiliary/persistence.
- **Boundary:** deterministic state separate from optional auxiliary calls.
- **Tests:** every promotion/privacy/freshness/bounds case.
- **Security:** promptware/secret rejection; advisory-only authority.
- **Risk/rollback:** silent self-modification; candidates inert and explicit promotion.
- **Gate:** all learning suites and metadata canaries.

## Stage 20 — Automation

- **Objective:** schedule parser/store/claims/runner/CLI and delivery.
- **Source:** cron modules/tests.
- **Targets:** `vesper-automation`, CLI/core delivery port.
- **Prerequisites:** sessions/core/tools/memory/provider.
- **Tests:** fake clock/DST/races/renew/stale/script/watchdog/silent.
- **Security:** fresh non-persisted session, scrubbed contained scripts, no recursive cron.
- **Risk/rollback:** duplicate jobs; do not run Rust daemon against legacy store before cross-process claim proof.
- **Gate:** Python/Rust contender exactly-once tests.

## Stage 21 — Plugins

- **Objective:** schema/hash/signature/trust/install/runtime contributions/CLI.
- **Source:** plugins/plugin_runtime/plugin_cli.
- **Targets:** `vesper-plugins`.
- **Prerequisites:** policy/tools/MCP/memory/config.
- **Tests:** existing signed vectors, tamper, path/symlink/extensions, atomic swap.
- **Security:** data-only and explicit publisher trust.
- **Risk/rollback:** arbitrary code/trust substitution; fail closed.
- **Gate:** byte-compatible verification and rollback.

## Stage 22 — Observability

- **Objective:** metadata event schema, JSONL reader/writer, aggregates and CLI.
- **Source:** telemetry/observability/failure metadata.
- **Targets:** `vesper-observability`.
- **Prerequisites:** stable runtime event taxonomy.
- **Tests:** allowlist, malformed data, concurrency, opt-out, aggregates.
- **Security:** no bodies/raw identity/path/command/reasoning.
- **Risk/rollback:** privacy leak; canary gate and disabled-by-policy sink.
- **Gate:** source aggregate semantic parity and privacy proof.

## Stage 23 — TUI/terminal/mobile/media

- **Objective:** reducer-first full user experience over shared runtime.
- **Source:** `tui.py`, terminal_cli/image/voice/mobile/PWA and tests.
- **Targets:** `vesper-tui`, optional mobile/media crates.
- **Prerequisites:** all runtime commands/events stable.
- **Tests:** reducer fixtures, TestBackend snapshots, PTY keys/mouse/paste/restore, screen reader, queue/session isolation, subprocess bounds.
- **Parity/security:** no direct provider clients; permission redaction; terminal always restored.
- **Risk/rollback:** feature/accessibility loss; retain Python frontend during comparison.
- **Gate:** command/binding matrix and user acceptance.

## Stage 24 — Packaging and cross-platform release

- **Objective:** five binaries/archives, aliases, installers, Registry, checksums/provenance/uninstall.
- **Source:** workflows/registry/scripts/uninstall tests.
- **Targets:** CI/release/installer/xtask.
- **Prerequisites:** GLM runtime/TUI complete.
- **Tests:** clean VM install/update/uninstall, version/chat help, asset/size, state preservation.
- **Security:** checksum/attestation; no admin; surgical uninstall.
- **Risk/rollback:** broken installs; retain last Python release and installer rollback.
- **Gate:** all five target artifacts pass.

## Stage 25 — GLM parity gate

- **Objective:** declare Rust GLM harness production-equivalent.
- **Prerequisites:** Stages 1–24.
- **Tests:** full Python baseline, full Rust suite, all differential/security/platform/package tests, performance report, opt-in live quality.
- **Parity criteria:** zero unapproved exact/security/schema regressions; semantic/TUI differences approved; no per-case quality/safety regression.
- **Failure handling:** keep Python default/public release; fix Rust behind preview.
- **Gate:** signed parity report and rollback/runbook.

## Stage 26 — Multi-provider expansion

- **Objective:** ship OpenAI-compatible/LM Studio, then OpenAI, Anthropic, Gemini/runtime adapters based on product priority.
- **Prerequisites:** GLM parity and provider port proven.
- **Boundary:** adapters only plus typed config contributions; no new loop.
- **Tests:** official-doc request/stream fixtures and common conformance suite per provider.
- **Security:** provider-specific auth/endpoint/data-retention review.
- **Risk/rollback:** capability leakage; adapters independently disableable.
- **Gate:** each provider passes common and provider-specific suites; GLM remains green.

## Critical path

`Decisions → fixtures → domain/provider ports → GLM + ACP → session actor → minimal loop → tools/security/policy → persistence/context → advanced services → TUI/package → GLM parity → providers`.

This order moves persistence writes and destructive tools behind proven contracts, while allowing early read-only SDK/network/platform spikes.

## Readiness to start

After this mission, Stage 0 and Stage 2 planning may start. Production Rust implementation should wait for explicit approval and the blocker list in the executive verdict.
