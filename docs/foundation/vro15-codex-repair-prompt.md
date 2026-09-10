# Codex handoff: finish the VRO-15 repairs

You are taking over Alex's Agent Vesper Rust repository. Implement and verify the remaining VRO-15 repairs, not another audit-only or incremental-hardening exercise. Previous work repeatedly stopped after small patches while the same integration gaps remained open. Finish the missing vertical paths, preserve existing repairs, and report externally blocked acceptance honestly.

## Start here

1. Read root AGENTS.md and every applicable child AGENTS.md before edits; follow DOX closeout rules. Use the repository's current contracts, not assumptions in this handoff.
2. Inspect git status/diff, including untracked files. The workspace contains substantial in-progress repairs across swarm, harness, providers, hosts, documentation and verification. Preserve them. Do not reset, clean, overwrite unrelated work, commit, push, publish, tag or alter the registry without explicit approval.
3. Read these requirement/evidence sources:
   - docs/swarm-oracle-extraction-prd.md
   - docs/foundation/vro15-gap-audit.md
   - docs/foundation/vro15-repair-execution.md (especially Current F01–F18 reconciliation)
   - docs/adr/0025-provider-neutral-swarm-orchestration.md
   - docs/adr/0026-swarm-repair-and-cross-host-acceptance.md (superseding repair/parity decision)
   - docs/migration-status.md
   - docs/foundation/vesperlens-end-to-end-acceptance-postmortem.md
4. Inspect actual implementations in crates/vesper-swarm, crates/vesper-sandbox, crates/vesper-harness, crates/vesper-agent, apps/agent-vesper-tui, apps/agent-vesper-acp and relevant configuration/security/verification crates. Locate exact current symbols before editing.
5. Maintain a visible execution plan and an F01–F18 acceptance matrix: requirement, current implementation, missing code, exact test/command evidence, status and blocker. Prioritize the critical integration path below. Historical test counts are not current acceptance.

## Priority 1: asynchronous sandbox lifecycle and actual composition

F09/F10 remain major blockers. The current ledger says lease backend calls remain synchronous under the book lock; catching panics does not fix blocking or asynchronous lifecycle ownership.

- Implement reservation/dispatch/commit semantics so external backend work never runs under the lease bookkeeping lock. Preserve boundary/member/queue limits, unique identities, FIFO admission, shared-group compatibility, explicit grants and fail-closed capability checks throughout in-flight operations.
- Handle concurrent acquire/release/close, cancellation, timeout, partial provisioning, dropped callers and backend failure without losing cleanup ownership, freeing uncertain capacity, allowing stale publication or duplicating backend work.
- Own cleanup explicitly through completion/shutdown. Drop alone is not proof of asynchronous teardown. Expose truthful cleanup/shutdown outcomes; quarantine uncertainty and permit only explicitly safe, bounded, idempotent cleanup recovery.
- Address blocking backend/destructor work at the correct boundary. Wrapping a synchronous call in an async timeout or dropping spawn_blocking does not stop it. Bound admission/resource use, retain ownership and report unresolved cleanup truthfully. Never claim a deadline guarantees physical cleanup without evidence.
- Compose real scoped leases through Hive worker factories, selected-worker execution, permission grants, scale/replacement, cancellation and shutdown using the real sandbox backend. A disconnected API or test fake is not completion.
- Verify process-tree termination/reaping, backend error propagation, filesystem alias protection and per-worker grant isolation. Do not weaken isolation to make tests pass.

## Priority 2: complete the native product path in both hosts

F01–F04 and related lifecycle acceptance remain incomplete.

- Implement the persisted native Settings activation path and shared command/service composition required by the PRD and ADR 0026. Use existing native Save/Cancel conventions; manual configuration-file editing is not the normal workflow.
- Keep default-off and fail-closed activation until the defined gates pass. Persisted preference must not bypass capability or permission gates. Implement and test disabled, enabled, unavailable and cancellation paths.
- Wire TUI and ACP to the same host-neutral service and command catalog. Document only genuine protocol/interactive UX differences in owning host AGENTS.md files. Do not invent unsupported controls or provider capabilities.
- Verify navigator plus three independently executing workers, actual permissioned tools, bus-correlated assignments, topology-aware routing/failover, grounded synthesis, selected-worker provenance, cancellation and visible partial results in the composed path.
- Verify existing cognition, semantic compaction, context/tool-transaction integrity, streaming/finalization and no-replay rules through Hive—not just direct AgentLoop tests.
- Preserve provider-neutral behavior for real registered adapters. Codex CLI is only the external development tool for this task; do not introduce it as a Vesper production runtime dependency.
- Verify VesperLens feedback from the real browser submission through the native continuation path into provider-visible messages using deterministic local provider transport capture. An interview-page test alone does not prove this end-to-end route.

## Priority 3: reconcile every remaining acceptance gap

Use the full ledger rather than treating this list as a replacement specification:

- F05: preserve scorer eligibility/arithmetic and pinned-oracle vectors; verify composed selection separately.
- F06/F07: native detached-work cleanup, real instance retirement and bounded lifecycle/resource ownership.
- F08: composed routing and failover, including disabled-failover semantics.
- F11/F12: retain bus close/wakeup, atomic broadcast, ACK/byte/TTL bounds and clock-injection evidence under concurrency.
- F13/F14: broaden malformed snapshot/property coverage and cross-target snapshot compatibility/continued insertion.
- F15: measure high-dimensional/default-capacity resource behavior and adversarial clustering against the accepted requirements; record realistic memory/time limits. A 10k/16D benchmark is not million-entry certification.
- F16: reconcile timestamp and retention requirements explicitly against the PRD/oracle; sequence ranges are not timestamps. Verify coherent retained readers and transactional whole-ledger operations at the required scale.
- F17: map every finding to exact desired-behavior assertions, native host/supervisor evidence and permission traceability.
- F18: finish required timing injection and platform gates without weakening optional/default-off architecture or naming-baseline policy.

## Engineering and verification rules

- Fix root causes and missing integration before optional extra hardening. Reuse existing foundations; no parallel harness, fake production embeddings, silent fallbacks, invented capabilities or mocked production success.
- Keep MSRV 1.88 and dependency-direction constraints. Read-only oracles stay pinned and unchanged; respect the swarm-oracle naming embargo.
- Use offline/deterministic providers, isolated state and real permitted local process/browser boundaries for foundation tests. No live provider calls, real credential access, arbitrary project state or user-state writes. Honor permissions; request approval for genuinely necessary external/privileged acceptance rather than bypassing sandbox restrictions.
- Add regressions for success and refusal, cancellation races, timeout, closed-state behavior, cleanup failure, resource bounds and absence of unintended side effects. Prefer deterministic barriers and injected clocks over sleeps.
- Run focused tests during implementation. At meaningful integration checkpoints run repository-defined canonical verification, workspace default/all-feature tests, strict all-target/all-feature Clippy, Rust 1.88 checks/tests, formatting, architecture, naming and supply-chain gates. Discover canonical commands from current xtask/CI contracts rather than guessing flags.
- Run required explicit ignored scale, supervisor/container, real-browser and platform tests where supported. Report executed assertions versus skipped bodies. Missing namespaces/Docker/target runners are acceptance blockers, not passes and not permission to disable security. Implement test/CI coverage where possible and record exact external follow-up commands.
- Update nearest owning AGENTS.md files, repair execution matrix, evidence index and current status as appropriate. Keep evidence concise and distinguish implementation complete, locally verified, externally blocked and fully accepted.

## Execution discipline and final deliverables

Continue autonomously across ordinary milestones; do not stop after another small patch and require Alex to say continue. Do not restart the audit repeatedly or rerun broad suites without relevant changes. If a real environment/permission blocker occurs, record it precisely and continue independent executable work. Respect actual execution limits; preserve an exact resume point if interrupted rather than claiming completion.

Deliver production integration, regression/acceptance tests, updated DOX/evidence and a final F01–F18 matrix. The final report must identify changed behavior, exact commands/results, any remaining external blockers and how to run their gates. Do not say all fixed while required code or acceptance remains missing. Do not release or activate the feature broadly merely because unit tests pass.
