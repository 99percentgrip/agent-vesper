# Release Intent Autonomy Repair — Execution Report

## Objective

Repair the shipped Release Recovery Controller integration so an explicit ordinary-language release imperative enters the real persisted RRC automatically in both production hosts, without requiring `/release`, confirmation, or repeated follow-up. Preserve a dirty active checkout, deterministically select completed provenance, refuse ambiguity before mutation, and run the existing release executor only from a clean isolated release worktree.

## Methods and commands

Work was performed in the isolated worktree:

```text
/home/Alex/Projects/agent-vesper/.worktrees/rrc-autonomy-repair
```

Starting commit:

```text
01d2df045a7c30253bea8e613e9840b9f62d053b
```

Commands executed:

```text
cargo fmt --all
cargo check -p agent-vesper-tui -p agent-vesper-acp
cargo test -p vesper-harness natural_release -- --nocapture
cargo test -p vesper-harness --lib
cargo test -p agent-vesper-acp --lib
cargo test -p agent-vesper-tui --lib
cargo test -p vesper-harness release_executor::tests::production_orchestrator_reaches_publication_only_through_settled_gates
cargo xtask acceptance
git diff --check
```

The regression-first sequence initially produced compile failures because the new classifier/admission APIs and provenance fields did not exist. After implementation, the focused tests passed.

## Files changed

- `crates/vesper-harness/src/release_recovery.rs`
  - Adds conservative ordinary-language release-intent classification.
  - Adds deterministic repository/worktree provenance resolution.
  - Selects one maximal clean completed descendant when the active checkout is dirty.
  - Returns one bounded clarification with concrete candidates when selection is ambiguous.
  - Creates a detached clean release worktree only after provenance is unambiguous.
  - Persists source workspace, source/base/final SHAs, ordered intended commits, exact binary-diff SHA-256, and release-workspace path.
  - Routes explicit `/release` starts through the same isolation/admission path and keys state by the underlying repository rather than one worktree path.
  - Adds regression tests for intent boundaries, dirty-primary preservation, clean isolated launch, and no-mutation ambiguity refusal.
- `crates/vesper-harness/src/release_executor.rs`
  - Keeps `final_candidate_commit` current after candidate creation and verified repair promotion.
- `apps/agent-vesper-tui/src/main.rs`
  - Intercepts admitted ordinary release imperatives before model dispatch and launches the shared RRC with the normal permission-aware worker factory.
- `apps/agent-vesper-acp/src/lib.rs`
  - Implements the same pre-provider admission and shared RRC launch path for ACP.
- `crates/vesper-harness/AGENTS.md`
- `apps/agent-vesper-tui/AGENTS.md`
- `apps/agent-vesper-acp/AGENTS.md`
  - Record the durable shared/host contracts.
- `docs/foundation/2026-09-30-release-intent-autonomy-repair.md`
- `docs/foundation/evidence-index.md`
- `docs/foundation/AGENTS.md`
  - Record and index this execution evidence.

## Exact evidence

### Focused natural-release regression

```text
running 2 tests
test release_recovery::tests::natural_release_intent_is_imperative_and_fail_closed ... ok
test release_recovery::tests::natural_release_from_dirty_primary_selects_completed_source_without_mutation ... ok

test result: ok. 2 passed; 0 failed
```

The broader harness run also executed the ambiguity test:

```text
test release_recovery::tests::ambiguous_completed_candidates_clarify_once_without_mutation ... ok
test result: ok. 158 passed; 0 failed; 2 ignored
```

### Production host suites

```text
agent-vesper-acp: test result: ok. 61 passed; 0 failed
agent-vesper-tui: test result: ok. 267 passed; 0 failed
```

### Production publication boundary

```text
test release_executor::tests::production_orchestrator_reaches_publication_only_through_settled_gates ... ok
test result: ok. 1 passed; 0 failed
```

### Completion-assurance acceptance gate

```text
acceptance verified: release_recovery::tests::partial_matrix_blocks_retry
acceptance verified: release_recovery::tests::verified_repair_is_the_only_path_to_one_full_gate_retry
acceptance verified: release_recovery::tests::paused_epoch_reopens_with_exact_identity_and_resumes_through_remote_refresh
acceptance verified: release_recovery::tests::green_release_then_red_closeout_keeps_publication_immutable
acceptance verified: release_executor::tests::production_orchestrator_reaches_publication_only_through_settled_gates
acceptance verified: release_executor::tests::release_worker_cancel_restart_process_acceptance
acceptance verified: commands::tests::release_routes_to_the_controller_instead_of_a_model_workflow
Acceptance regression gate: 30 exact cases passed in 73637 ms. Offline fixture model cost: zero; live-model effectiveness is not measured.
```

### Build and whitespace checks

```text
cargo check -p agent-vesper-tui -p agent-vesper-acp
Finished `dev` profile ...

git diff --check
(exit 0)
```

## Constraints held

- No release was started against the production repository.
- No version was bumped.
- No tag, publication, push, installation, or credential mutation was performed.
- No provider-specific release-intent logic was added.
- The active dirty checkout is never reset, cleaned, stashed, committed, or used as the release mutation workspace.
- Ambiguous provenance creates neither release state nor an isolated release worktree and launches no worker.
- Existing RRC retry, outage, mutation-token, permission, exact-SHA gate, publication, and closeout behavior remains the authority after admission.
- VRO-19 was not touched.

## Deviations

- No PRD was modified, as explicitly required by the task. This report is linked from the foundation evidence index and owning foundation DOX instead.
- No live GitHub release was attempted; deterministic production-boundary and acceptance tests were used. Consequently this work does not constitute a new release/publication receipt.
- Two harness tests remain intentionally ignored by the existing suite (`workspace-layout probe` and explicit contained-runtime acceptance); they are unrelated to this repair.

## Unresolved items

- Live end-to-end publication was intentionally prohibited and remains unexecuted.
- Cross-platform CI was not launched in this work unit. The implementation uses portable `git` subprocess operations and passed local production-host and acceptance suites, but this report does not claim a fresh five-platform run.

## Readiness effect

The two production hosts now share one conservative ordinary-language admission path into the existing RRC. A direct imperative such as “Release all completed work as the next patch” bypasses provider inference, resolves exact provenance, preserves dirty user bytes, creates a clean detached release workspace, persists auditable provenance, and starts the real background controller. Non-imperative release discussion remains ordinary chat. Ambiguous completed candidates fail closed with one specific clarification and no mutation.
