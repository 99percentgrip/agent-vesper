# RRC liveness/watchdog repair — execution report

**Date:** 2026-10-02

**Source base:** `89dfb4eac5644c1738f55c8ea273a04dc6ac5e11`

**Workspace:** clean isolated worktree `.worktrees/rrc-liveness-watchdog-20261002-2`

**Owning PRD:** [`../Agent_Vesper_Release_Recovery_Controller_PRD.md`](../Agent_Vesper_Release_Recovery_Controller_PRD.md)

**Verdict:** **IMPLEMENTED AND FOCUSED LOCAL REGRESSIONS PASSED. BROAD, HOSTED, RELEASE, PUBLICATION, REGISTRY, INSTALLATION, AND CROSS-PLATFORM VERIFICATION WERE NOT RUN.**

## Objective

Repair Release Recovery Controller liveness from the exact requested clean source without importing the dirty historical RRC worktree. The authorized scope was:

- progress-aware, state-specific local-command watchdogs;
- bounded and cancellable GitHub, official-health, and publication commands;
- a remote-matrix progress watchdog with a fresh dispatch window;
- repair-agent heartbeat, progress-aware stall, and cancellation handling without an elapsed-runtime ceiling;
- truthful persisted liveness projected through the shared TUI/ACP progress model;
- focused deterministic regressions only.

The final review also had to determine the exact RSS-heartbeat rule and prevent allocator/resident-set changes from refreshing local progress. The resulting rule is fail-closed: RSS is diagnostic telemetry only; process-tree heartbeat advances only when the owned process count or owned `rustc` count changes. Stdout/stderr remains an independent progress signal.

Explicit exclusions were preserved: no `v0.24.5` resume/reconciliation, live GitHub CI repair, push, tag, publication, Registry action, installer, release candidate build, broad verification, VRO-19 work, or import/modification of `.worktrees/v0245-rrc-autonomy`.

## Methods and commands

### Source isolation

```text
HEAD=89dfb4eac5644c1738f55c8ea273a04dc6ac5e11
BASE=89dfb4eac5644c1738f55c8ea273a04dc6ac5e11
```

The implementation remained in `.worktrees/rrc-liveness-watchdog-20261002-2`; the pre-existing dirty RRC worktree was not read for source transfer and was not modified.

### Focused verification

The accepted verification commands were run from the isolated worktree:

```text
cargo fmt --all
cargo fmt --all -- --check
cargo check -p vesper-harness --lib
cargo clippy -p vesper-harness --lib -- -D warnings
cargo test -p vesper-harness --lib <one exact focused test-name substring>
git diff --check
python3 <changed-Markdown relative-link check>
grep -nE <removed-ceiling token forms> <changed production/docs paths>
git status --short
```

The final focused regression set was:

1. `silent_local_command_is_stopped_by_state_specific_watchdog`
2. `silent_external_command_is_stopped_by_inactivity_watchdog`
3. `output_progress_extends_inactivity_window`
4. `long_running_with_continuous_progress_has_no_absolute_deadline`
5. `quiet_but_alive_child_transition_resets_progress`
6. `tiny_rss_jitter_does_not_refresh_process_heartbeat`
7. `material_process_tree_transition_supports_quiet_but_alive`
8. `external_request_hard_deadline_is_operation_scoped`
9. `external_command_cancellation_preempts_watchdog_deadlines`
10. `repair_heartbeat_distinguishes_progress_stall_and_cancellation`
11. `remote_dispatch_starts_a_fresh_matrix_progress_window`
12. `watchdog_liveness_survives_ledger_round_trip_and_drives_shared_headline`
13. `local_gate_watchdog_stop_is_not_persisted_as_source_failure`
14. `local_gate_cancellation_race_does_not_create_failure_evidence`

## Files changed

- `crates/vesper-harness/src/release_executor.rs`
  - added state-specific meaningful-progress inactivity policies with no elapsed-runtime ceiling for local gates, local mutations or repair turns;
  - retained hard deadlines only for individual remote Git, GitHub evidence, publication and health operations;
  - added owned process-group supervision with concurrent bounded stdout/stderr capture and reliable process-tree transition heartbeats;
  - made RSS telemetry-only so neither tiny allocator jitter nor large RSS growth independently refreshes process progress;
  - routed local gates, source/mutation Git commands, GitHub evidence, official health, publication queries, version metadata, and repair promotion commands through bounded supervision;
  - added repair AgentLoop progress heartbeat monitoring, a bounded cancellation-settlement grace, and typed stop outcomes;
  - preserved a running local gate as unsettled when watchdog/deadline/cancellation stops controller execution, rather than fabricating source-failure evidence;
  - persisted active/settled worker liveness around controller progression.
- `crates/vesper-harness/src/release_recovery.rs`
  - added typed watchdog/cancellation errors;
  - added backward-compatible persisted liveness state independent of source/CI evidence;
  - made the shared progress headline expose stalled/deadline/worker-failure state after worker exit or restart;
  - reset the remote no-progress clock at exact-SHA dispatch;
  - made the GitHub CLI adapter cancellation-aware and bounded.
- `crates/vesper-harness/AGENTS.md`
  - recorded the durable state-specific watchdog, exact RSS/process-transition heartbeat, repair-heartbeat, remote-watch, and shared-liveness contracts.
- `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md`
  - links this current focused repair, records the exact RSS/process-transition heartbeat rule, and preserves its unexecuted broad/hosted boundary.
- `docs/AGENTS.md`, `docs/foundation/AGENTS.md`, `docs/foundation/evidence-index.md`
  - index this report and its ownership boundary.
- `docs/foundation/2026-10-02-rrc-liveness-watchdog-repair.md`
  - records this work unit's scope, exact focused evidence, deviations, and unexecuted boundaries.

## Exact evidence

### Audit regression: retained red receipt

The final source audit found that `advance_release` converted a local-gate watchdog stop into `DiagnosingLocalFailure` before worker liveness persistence. The new regression failed against that behavior before the correction:

```text
running 1 test
test release_executor::tests::local_gate_watchdog_stop_is_not_persisted_as_source_failure ... FAILED

assertion `left == right` failed
  left: DiagnosingLocalFailure
 right: LocalVerification

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 215 filtered out
```

The correction now routes `WatchdogStalled`, `WatchdogDeadline`, and `Cancelled` around local source-failure settlement. Worker persistence records watchdog/deadline liveness separately and ignores the cancellation race until the user-owned cancel transition settles. The repaired watchdog and cancellation regressions both passed:

```text
running 1 test
test release_executor::tests::local_gate_watchdog_stop_is_not_persisted_as_source_failure ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 216 filtered out
```

```text
running 1 test
test release_executor::tests::local_gate_cancellation_race_does_not_create_failure_evidence ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 216 filtered out
```

### RSS-heartbeat audit

The previous byte-level rule included `process_tree_rss_bytes` in the process-heartbeat observation, so any resident-set change could reset `last_meaningful_progress_at`. The final rule removes RSS from that observation entirely. The regression checks both a one-byte change and an additional 1 GiB change; neither changes the heartbeat, and an inactivity interval beyond the 30-minute local-gate window remains `Stagnant`:

```text
running 1 test
test release_executor::tests::tiny_rss_jitter_does_not_refresh_process_heartbeat ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 218 filtered out
```

A companion regression proves that owned child-count and `rustc`-count transitions still support a quiet-but-alive classification:

```text
running 1 test
test release_executor::tests::material_process_tree_transition_supports_quiet_but_alive ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 218 filtered out
```

### Formatting, compilation, and lint

```text
cargo fmt --all -- --check
(exit 0; no output)
```

```text
cargo check -p vesper-harness --lib
Checking vesper-harness v0.24.5 (/home/Alex/Projects/agent-vesper/.worktrees/rrc-liveness-watchdog-20261002-2/crates/vesper-harness)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.95s
```

```text
cargo clippy -p vesper-harness --lib -- -D warnings
Checking vesper-harness v0.24.5 (/home/Alex/Projects/agent-vesper/.worktrees/rrc-liveness-watchdog-20261002-2/crates/vesper-harness)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.48s
```

### Focused regression matrix

All fourteen named focused invocations executed exactly one test and passed. The original twelve-test set reported one pass with 216 filtered tests per invocation. The two added RSS/process-transition regressions reported one pass with 218 filtered tests per invocation. Selected final-review reruns of the silent local watchdog and local-gate source-failure separation also reported one pass with 218 filtered tests.

### Documentation and diff integrity

```text
added_or_changed_relative_links=3 missing=0
git diff --check
(exit 0)
```

A broader scan of the pre-existing evidence index reported 53 stale/missing historical links outside the lines changed by this work unit. Those unrelated historical index defects were not modified; the three links introduced or changed here all resolve.

### Removed-ceiling token audit

```text
no 42-hour ceiling tokens in release executor/recovery
```

The searched forms were `151200`, `42h`, `42 hours`, `Duration::from_hours(42)`, `Duration::from_secs(151200)` and `42-hour`. No production constant, policy or caller in `release_executor.rs` or `release_recovery.rs` retains the rejected ceiling.

## Implemented liveness policy

| Operation | Inactivity bound | Hard deadline |
|---|---:|---:|
| Local verification command | 30 minutes | none |
| Local release/source mutation command | 5 minutes | none |
| Remote Git mutation command | 5 minutes | 30 minutes |
| GitHub evidence request | 2 minutes | 5 minutes |
| Publication evidence request | 2 minutes | 10 minutes |
| Official GitHub health request | 15 seconds | 15 seconds |
| Focused repair AgentLoop | 10-minute progress heartbeat | none |
| Remote exact-SHA matrix | 20-minute no-evidence bound reset by material matrix progress | none |

Local and repair operations are governed by the last observed meaningful progress, not total runtime. Stdout/stderr bytes independently refresh local progress. Process-tree heartbeat refreshes only on owned process-count or `rustc`-count transitions. RSS is retained for diagnostics and resource governance but never independently refreshes progress, regardless of delta size. Provider/tool/proof events reset repair progress. The remote-matrix bound is also a no-progress bound, not a matrix-duration deadline. Hard deadlines remain only on one external request or remote mutation process at a time.

### Remaining hard-deadline rationale

| State / operation | Deadline | Why inactivity alone is insufficient | Required safety property |
|---|---:|---|---|
| Candidate/tag remote Git mutation | 30 minutes | A credential-bearing network client can continue emitting transport progress or keepalives without settling the remote side effect. | Return mutation authority to the controller and stop only its owned client tree. The focused suite does not prove post-timeout remote reconciliation. |
| One GitHub evidence request | 5 minutes | A chatty or retrying CLI can emit output while one API transaction never settles. | Bound one read transaction and return typed timeout evidence to bounded infrastructure policy; this is not a CI-matrix duration limit. |
| One publication evidence request | 10 minutes | Workflow/release asset lookup may emit progress while one CLI transaction remains unsettled. | Recover controller ownership and retry only the observation request without imposing a publication-stage duration limit. |
| One official-health request | 15 seconds | The status classifier requires a timely point-in-time observation; transport output does not make an old health request current. | Keep outage classification bounded and prevent one health endpoint process from freezing failure classification. |

## Deviations

- The first implementation incorrectly retained a 42-hour local ceiling and a two-hour repair-agent ceiling, plus a regression that killed a continuously progressing command. Alex rejected that semantic model. This correction removes both production ceilings and replaces the regression with long-runtime progress and quiet-process evidence.
- The original newly added watchdog tests and the RSS regression were not run red before the corresponding implementation, so they do not have retained red-before-fix receipts. The final audit did add and retain a red-before-fix regression for the discovered source-failure misclassification, shown above.
- The final `cargo fmt --all -- --check` initially reported only formatting drift in the newly expanded RSS assertion. `cargo fmt --all` corrected it; the required check was rerun afterward.
- Test-filter loops were twice attempted with unqualified names plus `--exact`; Cargo selected zero tests. Those outputs were rejected. Every final named invocation was rerun without `--exact`, executed one test and passed.
- One audit test command was initially issued from the primary checkout instead of the isolated worktree. It compiled the primary checkout's `vesper-harness v0.24.4`, selected zero tests, and produced no accepted evidence. All accepted commands and receipts above come from the isolated `v0.24.5` worktree. No release, install, source-transfer, or tracked-file edit was performed by that command.
- The audit corrected five consistency gaps before final verification: remote/resource wait cancellation returns typed `Cancelled`; the remaining production source-discovery Git probe uses bounded supervision instead of direct `.status()` execution; a stopped shared progress stage projects `Paused`; local watchdog/deadline/cancellation stops no longer settle a running gate as a source failure; and RSS changes no longer count as process progress.
- An independent read-only review was requested earlier in the work unit, but delegation failed before review because the configured OpenAI model was unavailable in the current account model list. Review therefore consisted of source tracing, direct-command inventory, diff inspection, compile/lint checks, the retained red audit regression, and focused deterministic tests.

## Unresolved and deliberately unexecuted items

- No complete `cargo test`, `cargo xtask verify`, `cargo xtask acceptance`, MSRV, architecture, supply-chain, or five-target matrix was run.
- No real GitHub CLI/API, official status endpoint, provider, TUI, or ACP process was contacted or launched for this repair.
- No macOS or Windows execution evidence was produced; process-group behavior there remains covered only by existing project evidence, not refreshed by this work unit.
- No repair-agent or local-gate wall-clock stall was held for its production inactivity interval; deterministic policy seams test the classifications with short durations and simulate continuous progress beyond the removed local ceiling.
- No live ambiguous remote-mutation timeout/reconciliation was exercised. The watchdog preserves a typed stop rather than fabricating settlement, but this focused work unit does not add or prove a post-timeout remote-observation workflow.
- The existing `v0.24.5` epoch and dirty historical RRC worktree remain untouched and unreconciled.

## Readiness effect

The clean source now has progress-aware supervision across the RRC command surfaces, a no-progress heartbeat for repair turns without an elapsed-runtime ceiling, a correctly initialized remote-matrix progress window, operation-scoped external request deadlines, and persisted host-neutral stop truth. RSS cannot extend local command life: only output or an owned process/`rustc` count transition refreshes local progress. A watchdog, deadline, or cancellation stop cannot be promoted into local source-failure evidence. The focused local evidence supports review of this liveness repair only. It does **not** authorize release resumption, publication, installation, broad completion, or a claim that all platform gates are current.
