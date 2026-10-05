# RRC registered-task lifecycle repair — execution report

**Date:** 2026-09-30  
**Status:** IMPLEMENTED AND LOCALLY VERIFIED; LIVE TUI ACCEPTANCE NOT RUN; NO CANDIDATE BUILT; NO RELEASE PERFORMED

## Objective

Repair the live natural-language RRC failure in the isolated `rrc-autonomy-repair` worktree. After `Release all completed work as the next patch.`, a startup acknowledgement must be backed by one durable, cancellable, process-owned controller task. That task must enter `RunLocalVerification`, expose real gate/subprocess activity, survive command-handler return, persist progress, and drive TUI activity instead of allowing simultaneous `Ready` / `No active tasks` claims.

## Constraints held

- Worked only in `/home/Alex/Projects/agent-vesper/.worktrees/rrc-autonomy-repair`.
- Did not release, publish, tag, push, install, or invoke an installer.
- Did not modify or investigate VRO-19.
- Did not build a new TUI/ACP candidate. The previously rejected TUI SHA-256 `99de2f87e50abd696bac8b204539ec752f14ce9a41370ef8d44af7516c545f01` was not reused as evidence.
- Did not clean, reset, stash, or otherwise mutate the dirty primary checkout.
- Removed no user state and made no live provider or GitHub calls.

## Diagnosis

The admission path could acknowledge controller launch without a queryable task owner that the TUI could project. The prior worker registry retained only a cancellation flag while the thread handle was detached, and the TUI runtime state considered only provider turns. Consequently, admission prose could say local verification had started while the runtime panel independently rendered idle state.

The repair makes worker ownership explicit and authoritative:

1. `release_executor` registers an `ActiveReleaseWorker` containing the join handle, cancellation token, epoch identity, and synchronized activity before releasing the worker start barrier.
2. The launcher returns a typed registration only after the worker is registered and start is admitted.
3. The worker updates activity from persisted RRC state, executes the existing real local-gate path, persists failure settlement, and removes itself only at terminal/paused settlement.
4. Workspace/repository lookup APIs expose snapshots from that registry.
5. The TUI builds `ViewModel.background_task` from the real registry every frame. A registered release task sets running state, replaces `Ready`, replaces `No active tasks`, and shows current local-verification detail.
6. `/release cancel` continues to signal the same registry-owned cancellation token.

## Production-path regression

`release_recovery::tests::tui_natural_release_retains_registered_controller_through_local_gate_progress` exercises ordinary-language admission through the production reconciliation/launcher seam and the real release worker. Its fixture:

- starts from a persisted obsolete epoch;
- admits `Release all completed work as the next patch.`;
- launches exactly one repository-keyed worker;
- waits for a real child command to create `local-gate-started`;
- observes the persisted gate in `Running` state;
- verifies the worker epoch and activity detail;
- verifies the dirty primary fixture bytes are unchanged;
- cancels through the production cancellation API;
- waits for worker deregistration; and
- verifies persisted `DiagnosingLocalFailure` / failed-gate settlement and preservation of the superseded epoch.

`ui::tests::registered_release_task_replaces_ready_and_no_active_tasks` renders the real background-task projection and rejects both contradictory idle labels.

### Red-evidence limitation

The reported live failure is the pre-repair red reproduction: startup text appeared while the runtime panel showed `RUN`, `Ready`, and `No active tasks`, with no subsequent activity. The compacted session record did not retain a command transcript from running the new automated regression against a mechanically reverted pre-repair tree. The current automated receipts are green post-repair receipts; this report does not fabricate a missing automated red command.

## Files

### Runtime and tests

- `crates/vesper-harness/src/release_executor.rs`
  - retained worker ownership and start barrier;
  - typed registration and activity snapshots;
  - cancellation and failure settlement;
  - injectable state-root seam used by the production-path regression.
- `crates/vesper-harness/src/release_recovery.rs`
  - admission launcher ordering;
  - real subprocess/progress/cancellation/dirty-checkout regression.
- `apps/agent-vesper-tui/src/main.rs`
  - per-frame projection from the real registered worker.
- `apps/agent-vesper-tui/src/ui.rs`
  - background-task view model and truthful running/TODO/Run presentation;
  - renderer regression.
- `xtask/src/main.rs`
  - both lifecycle and renderer regressions added to the fixed acceptance set.

### Durable contracts and evidence

- `crates/vesper-harness/AGENTS.md`
- `apps/agent-vesper-tui/AGENTS.md`
- `xtask/AGENTS.md`
- `docs/AGENTS.md`
- `docs/foundation/AGENTS.md`
- `docs/foundation/evidence-index.md`
- `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md`
- `docs/foundation/2026-09-30-rrc-task-lifecycle-repair.md`

The worktree also contains inherited stale-epoch, source-resolution, version-mutation, ACP parity, documentation, and evidence changes from the broader RRC repair. This report does not reclassify those inherited changes as newly produced lifecycle evidence.

## Methods and exact receipts

All commands below ran from `/home/Alex/Projects/agent-vesper/.worktrees/rrc-autonomy-repair` unless stated otherwise.

### Focused lifecycle regression

```text
$ cargo test -p vesper-harness --lib release_recovery::tests::tui_natural_release_retains_registered_controller_through_local_gate_progress -- --exact --nocapture
running 1 test
test release_recovery::tests::tui_natural_release_retains_registered_controller_through_local_gate_progress ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 168 filtered out
release controller paused safely: invalid release recovery input: release recovery was cancelled by the user
```

The final diagnostic is expected fixture behavior: cancellation is observed by the real local command path and persisted as a failed local gate for safe diagnosis.

### Focused TUI projection regression

```text
$ cargo test -p agent-vesper-tui --lib ui::tests::registered_release_task_replaces_ready_and_no_active_tasks -- --exact --nocapture
running 1 test
test ui::tests::registered_release_task_replaces_ready_and_no_active_tasks ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 267 filtered out
```

### Formatting and host compilation

```text
$ cargo fmt --all -- --check
PASS (exit 0; no output)

$ cargo check -p agent-vesper-tui --bin agent-vesper-tui
Finished `dev` profile ...

$ cargo check -p agent-vesper-acp
Finished `dev` profile ...
```

### Full affected-crate suites

```text
$ cargo test -p vesper-harness --lib
running 169 tests
...
test result: ok. 167 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out

$ cargo test -p agent-vesper-tui --lib
running 268 tests
...
test result: ok. 268 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### Enforced acceptance

The first acceptance run, before registering the two new exact cases, passed the existing 34 cases. The fixed acceptance inventory was then updated so lifecycle ownership and truthful renderer projection cannot be omitted.

```text
$ cargo xtask acceptance
...
acceptance verified: release_recovery::tests::tui_natural_release_retains_registered_controller_through_local_gate_progress
...
acceptance verified: ui::tests::registered_release_task_replaces_ready_and_no_active_tasks
Acceptance regression gate: 36 exact cases passed in 13772 ms. Offline fixture model cost: zero; live-model effectiveness is not measured.
```

The final command was preceded by `cargo fmt --all -- --check`; both exited 0.

### Workspace identity before report/index edits

```text
HEAD=01d2df045a7c30253bea8e613e9840b9f62d053b
STATUS_SHA256=372eedd9fec82453dbf1d6435ffaf4f1ac1e841a1a9542098c4001d11c0ba4a2
DIFF_SHA256=bb9c8fd08baf04c7e064edad1349cf3dfe14e7cf9675e1a4ca4cd795c30f078a
ACP_CONFIG_FIXTURE=absent
```

The primary checkout remained at `0b5630d271965e7d9df3a0a09c116c7f8c44042e`. Its pre-existing dirty inventory was observed but not altered by source-edit tools.

## Deviations

- No fresh executable candidate was built, by explicit constraint. Therefore there is intentionally no repaired TUI SHA-256 and no claim that manual live acceptance has passed.
- No mechanically reverted automated red-run transcript is available in the retained session evidence. The exact live failure is the red observation; automated postconditions are green.
- Cross-platform lifecycle CI and live terminal acceptance were not run in this local work unit.
- Full repository `cargo xtask verify` was not run; focused affected suites, both host checks, formatting, and the enforced acceptance gate were selected for this repair.

## Unresolved items

1. Alex-operated live TUI acceptance remains required to confirm a future repaired binary displays registered gate activity under the original interaction.
2. A future candidate, if explicitly authorized, must have a SHA-256 different from `99de2f87e50abd696bac8b204539ec752f14ce9a41370ef8d44af7516c545f01`.
3. Cross-platform workflow evidence remains unchanged; this report makes no new five-target claim.

## Closeout receipts

```text
CLOSEOUT_CHECKS=pass
WORKTREE_HEAD=01d2df045a7c30253bea8e613e9840b9f62d053b
PRE_REPORT_CLOSEOUT_STATUS_SHA256=86589fb2dc69039d87fb2b2734804eba49510127e6e2e482862932d193f7476c
PRE_REPORT_CLOSEOUT_DIFF_SHA256=74b79361fdf24dccee6caee531dcc9228caea9764a175a0b63b6dab46ec5faed
PRIMARY_HEAD=0b5630d271965e7d9df3a0a09c116c7f8c44042e
PRIMARY_STATUS_SHA256=1261528c0926297bde8d3026c24c7797e3299e03b6d6895aab24945b0ceeca1e
```

`CLOSEOUT_CHECKS` comprises `git diff --check`, `cargo fmt --all -- --check`, report existence, and report-link checks in the evidence index, owning PRD, and applicable DOX documents. The primary status hash is identical before and after closeout observation.

## Readiness effect

The source now has a deterministic ownership invariant and local automated proof for the exact controller/TUI contradiction. It is ready, only with explicit authorization, for a fresh candidate build and live manual acceptance. It is not release-ready evidence by itself and authorizes no publication action.
