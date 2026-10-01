# RRC Linux live closeout audit

**Date:** 2026-09-30  
**Status:** Linux host decision and one exact PTY run passed; progress contract is not ready.  
**Scope boundary:** Read-only host-policy observation, one exact focused TUI PTY test invocation, and a source/test audit only. No production or test feature was changed. No RRC epoch, release, publication, tag, push, installation, candidate build, provider call, VRO-19 work, or additional expensive Cargo verification was started.

## Objective

Close the requested Linux-only RRC evidence pass without feature or release work:

1. make a read-only live host-governor decision;
2. run exactly `cargo test -p agent-vesper-tui --test rrc_terminal_pty` once with the resulting conservative concurrency;
3. audit progress-contract items A–H independently against current code and tests; and
4. issue the release-readiness sentence only if every Linux live-acceptance requirement is proven.

## Methods and commands

1. Re-read the root, documentation/foundation, TUI, and TUI-test DOX contracts.
2. Reused the source-matched, read-only Linux observation method documented by [`2026-09-30-rrc-governor-live-host-observation.md`](2026-09-30-rrc-governor-live-host-observation.md): `/proc/meminfo`, `/proc/self/cgroup`, cgroup-v2 ancestry, CPU affinity, and `statvfs` against the nearest existing ancestor of the default managed target path. The observation did not construct `HostResourceGovernor`, create `host-resources`, or acquire the expensive-work lease.
3. Ran the required test target exactly once:

   ```text
   CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 cargo test -p agent-vesper-tui --test rrc_terminal_pty
   ```

4. Audited the exact progress model, retry/reopen update paths, TUI conversation projection, fixed-width RUN rail, and directly relevant tests. No additional Cargo workload was run.
5. Confirmed after the run that `/home/Alex/.local/state/agent-vesper/release-recovery/host-resources` was still absent.

A previous continuation attempted this different, nonexistent target before the required run:

```text
CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 cargo test --locked --offline -p agent-vesper-tui --test rrc_governed_resource_recovery -- --exact rrc_governed_resource_recovery
```

It exited 101 with `error: no test target named 'rrc_governed_resource_recovery' in 'agent-vesper-tui' package`. It is retained as a deviation, is not counted as the required PTY run, and supplies no acceptance evidence.

## Files assessed and changed

Assessed:

- `crates/vesper-harness/src/host_resources.rs`
- `crates/vesper-harness/src/release_recovery.rs`
- `crates/vesper-harness/src/release_executor.rs`
- `crates/vesper-harness/src/release_progress_tests.rs`
- `apps/agent-vesper-tui/src/main.rs`
- `apps/agent-vesper-tui/src/ui.rs`
- `apps/agent-vesper-tui/tests/rrc_terminal_pty.rs`
- the existing RRC foundation reports and owning PRD

Documentation changed for closeout only:

- `docs/foundation/2026-09-30-rrc-linux-live-closeout-audit.md`
- `docs/foundation/evidence-index.md`
- `docs/foundation/AGENTS.md`
- `docs/AGENTS.md`
- `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md`
- `docs/foundation/2026-09-30-rrc-progress-milestones.md` — corrected one prior narrative overstatement about process-restart deduplication

No Rust source, test source, Cargo manifest, lockfile, fixture, release state, candidate artifact, or installed application was changed.

## Exact Linux host-governor decision

The source-matched observation at `2026-09-30T14:32:58.923232+00:00` produced:

```text
MEM_TOTAL_BYTES=29024219136
MEM_AVAILABLE_BYTES=21496590336
EFFECTIVE_MEMORY_LIMIT_BYTES=29024219136
EFFECTIVE_MEMORY_AVAILABLE_BYTES=21496590336
SWAP_TOTAL_BYTES=8589930496
SWAP_FREE_BYTES=6970822656
SWAP_USED_PERCENT=18
LOGICAL_CPUS=20
MEMORY_RESERVE_BYTES=7256054784
NORMAL_HEADROOM_BYTES=2902421913
NEW_EXPENSIVE_MEMORY_THRESHOLD_BYTES=10184776913
PRESSURE=Normal
CARGO_BUILD_JOBS=2
RUST_TEST_THREADS=2
EXPENSIVE_SLOTS=1
DISK_TOTAL_BYTES=1021431513088
DISK_AVAILABLE_BYTES=368278962176
DISK_SAFETY_RESERVE_BYTES=102143151308
EXPECTED_EXPENSIVE_GATE_GROWTH_BYTES=32212254720
REQUIRED_DISK_FREE_BYTES=134355406028
DISK_ADMISSION=true
MANAGED_TARGET_PATH=/home/Alex/.local/state/agent-vesper/release-recovery/host-resources/targets/abde39a5088456752525d06909d10c51fbafd907be499cb113abf15d9afce206
```

All applicable cgroup-v2 `memory.max` files were `max`, so no finite cgroup limit displaced physical RAM. The decision retained 25% (`7,256,054,784` bytes) as the desktop memory reserve, selected only two Cargo jobs and two Rust test threads despite 20 available logical CPUs, admitted at most one expensive workload, and found `368,278,962,176` bytes free against a `134,355,406,028`-byte disk requirement. This is conservative and does not choose unsafe Cargo parallelism.

The managed target-cache parent remained absent after verification:

```text
HOST_RESOURCES_ABSENT_EXIT=0
```

## Exact PTY verification receipt

The required command was invoked once, with the conservative host-derived settings above. It exited successfully:

```text
running 1 test
test rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.98s

PTY_EXIT_STATUS=0
```

The test drives the production TUI binary in a real 120×36 PTY and directly rejects inherited child CSI/OSC/carriage-return/control output, telemetry outside RUN, premature `Ready`, and stale post-settlement telemetry. This is terminal-ownership/confinement evidence. It does not, by itself, prove the separate progress-percentage contract.

The complete `cargo xtask verify` had already passed earlier in this worktree and is recorded in [`2026-09-30-rrc-governor-acp-process-verification-rerun.md`](2026-09-30-rrc-governor-acp-process-verification-rerun.md). It was intentionally not rerun here because this directive prohibited another expensive Cargo verification workload.

## Progress-contract audit A–H

A requirement is marked **YES** only where the current implementation and scope-appropriate evidence directly establish it. A source path that looks compatible, an adjacent test, or a historical assurance is not promoted to completion evidence.

### A. Overall phase has a real completed/total denominator and percentage — **NO**

- **Exact code path:** `ReleaseProgressUnits::render` in `crates/vesper-harness/src/release_recovery.rs` renders only `completed/total`. `ReleaseRecoveryRecord::refresh_progress` derives phase and separate local/remote counts, but no overall completed/total model or percentage. `render_sidebar` in `apps/agent-vesper-tui/src/ui.rs` renders `phase · x/y local · x/y CI`.
- **Exact test:** `release_progress_tests::persisted_hierarchy_uses_typed_gate_and_job_states_without_percentages` explicitly expects `1/2`, `18/41`, and `1/2`; `ui::tests::registered_release_task_replaces_ready_and_no_active_tasks` expects `Running · Local verification · 3/7` and `4/5 CI`.
- **What is missing:** no real overall denominator exists and no overall percentage is rendered. This is an implementation gap, not merely missing test coverage. It also conflicts with the current durable narrative that deliberately forbids an overall release percentage because no stable weighted denominator is defined.

### B. Unknown-denominator active tasks use status/time/activity and never fabricate a percentage — **YES**

- **Exact code path:** `ReleaseProgressUnits::render` returns `None` for a missing or zero denominator. `progress_task_lines` omits units in that case and renders the typed state marker. `render_sidebar` renders the active release marker, stage/status, elapsed gate time, and last-activity/quiet-process text.
- **Exact test:** `ui::tests::registered_release_task_replaces_ready_and_no_active_tasks` asserts `10m 14s`, a changing child/gate count, and `No output for 4m 12s`; the progress-hierarchy test above proves unknown units remain representable without a fabricated denominator.
- **Evidence boundary:** the marker is a live status dot rather than an animated spinner, but the required status/time/activity fallback is present and no percentage is synthesized.

### C. Local aggregate is derived from completed/total with a percentage — **NO**

- **Exact code path:** `ReleaseRecoveryRecord::refresh_progress` counts `SettlementState::Succeeded` local gates and stores `completed_local_gates` / `total_local_gates`; `render_sidebar` prints the fraction only.
- **Exact test:** `ui::tests::registered_release_task_replaces_ready_and_no_active_tasks` asserts `3/7`, then `4/7`. `release_progress_tests::persisted_hierarchy_uses_typed_gate_and_job_states_without_percentages` asserts `1/2`.
- **What is missing:** the aggregate count is real, but there is no derived or displayed local percentage.

### D. CI aggregate is derived from terminal jobs/total jobs with a percentage — **NO**

- **Exact code path:** `ReleaseRecoveryRecord::refresh_progress` counts terminal remote jobs from current required-gate job state and stores `terminal_remote_jobs` / `total_remote_jobs`; `render_sidebar` prints `x/y CI` only.
- **Exact test:** `ui::tests::registered_release_task_replaces_ready_and_no_active_tasks` asserts `4/5 CI`; `release_progress_tests::persisted_hierarchy_uses_typed_gate_and_job_states_without_percentages` asserts the current gate fraction.
- **What is missing:** the terminal/total count is real, but there is no derived or displayed CI percentage.

### E. Retry/reopen never double-count and progress never exceeds 100% — **NO (not independently proven)**

- **Exact code paths inspected:**
  - `ReleaseProgress::update_local_subtask` rejects `total == 0` and `completed > total`.
  - `ReleaseProgressUnits::render` clamps displayed completed units to the denominator.
  - `ReleaseRecoveryRecord::refresh_progress` recomputes aggregates from current typed state rather than incrementing cached totals.
  - `ReleaseRecoveryRecord::apply_matrix` replaces an existing gate with the same name and rejects an older workflow attempt.
  - retry dispatch clears `required_gates` before transition to the new remote-gate run.
- **Exact nearby tests:** `verified_repair_is_the_only_path_to_one_full_gate_retry`, `changed_fingerprint_after_retry_opens_a_new_diagnosis`, and `paused_epoch_reopens_with_exact_identity_and_resumes_through_remote_refresh` exercise retry/reopen control flow.
- **Missing evidence:** none of those tests asserts progress totals before and after retry/reopen, detects double-counting, or establishes a percentage ceiling. The source has defensive mechanisms, but the specific acceptance contract is not independently proven and no percentage exists to test against 100%.

### F. Durable completed progress survives TUI/process restart — **NO (not independently proven)**

- **Exact code/test paths:** `ReleaseRecoveryRecord` serializes `progress`; `release_progress_tests::typed_progress_tracks_local_gate_and_survives_ledger_serialization` performs an in-memory serde round trip and compares the projection. `ledger_round_trip_preserves_job_ids` exercises a real ledger save/load but asserts only a job ID. `release_worker_cancel_restart_process_acceptance` exercises worker process restart without asserting restored progress counts/hierarchy.
- **Missing evidence:** there is no durable ledger or TUI/process-restart test that writes completed progress, restarts/reopens, and proves the same counts/hierarchy are restored in the host. In-memory serde plus a separate restart test cannot be combined as a substitute for this acceptance item.

### G. Conversation progress appears once per stage change without per-frame duplicate spam — **NO (implementation present, direct regression missing)**

- **Exact code path:** `project_release_milestones` in `apps/agent-vesper-tui/src/main.rs` filters persisted milestones by `(epoch_id, sequence)`, appends only rows whose sequence exceeds the in-session cursor, then advances that cursor. A new process intentionally seeds up to the last three milestones.
- **Exact nearby test:** `release_progress_tests::progress_history_is_bounded_and_monotonic` proves bounded monotonic milestone sequences in the shared model.
- **Missing evidence:** no test invokes `project_release_milestones` over repeated render frames and asserts one conversation insertion on stage change plus zero duplicates on unchanged frames. The shared monotonic-sequence test does not prove the TUI projection behavior. Process restart also does not persist the TUI cursor and intentionally reseeds recent messages, so the previous blanket “across redraws and restarts” claim was corrected.

### H. Concise subordinate statuses remain usable at 125–150 columns — **YES**

- **Exact code path:** `render_to_frame` allocates a fixed 40-column sidebar when the body is at least 110 columns; `progress_task_lines` limits the hierarchy to eight rows, caps indentation depth, and truncates names with an ellipsis while preserving state and units.
- **Exact tests:** `ui::tests::registered_release_task_replaces_ready_and_no_active_tasks` renders a 150×32 terminal and asserts the local phase, gate, subtask `18/41`, `4/5 CI`, elapsed time, current child, and quiet activity. The successful `rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty` run exercises the real binary at 120×36 and proves RUN/sidebar confinement, which is narrower than the requested 125–150 range.
- **Evidence boundary:** this proves concise statuses and width handling, not the missing percentages in A/C/D.

## Verdict matrix

| Item | Verdict | Blocking reason |
| --- | --- | --- |
| A | NO | No overall denominator or percentage exists. |
| B | YES | Unknown totals fall back to truthful state/time/activity without a fabricated percentage. |
| C | NO | Local aggregate renders only a real fraction, not a percentage. |
| D | NO | CI aggregate renders only a real fraction, not a percentage. |
| E | NO | Defensive source paths exist, but retry/reopen no-double-count behavior lacks direct progress assertions. |
| F | NO | No durable ledger + process/TUI restart acceptance proves restored progress. |
| G | NO | In-session cursor logic exists, but no direct repeated-frame/stage-change regression proves exactly-once projection. |
| H | YES | 150-column unit rendering and narrower 120-column real-PTY confinement are proven. |

The Linux host decision is **GREEN** and the one required PTY run is **GREEN**. The progress contract is **NOT GREEN** because A, C, and D are absent behavior and E, F, and G lack the required independent acceptance proof.

## Deviations and unresolved items

- The earlier wrong target invocation exited 101 before test execution. It was not substituted for the required command and is not acceptance evidence.
- No real governor object or expensive-work lease was created. The host decision is source-matched read-only policy evidence; the test process received the resulting conservative Cargo/test concurrency explicitly.
- `CARGO_TARGET_DIR` was not redirected to the absent controller-managed cache because constructing that cache would create user state. The target-cache parent remained absent.
- The full verification suite was not rerun. Its earlier current-worktree completion remains historical evidence, not a fresh receipt from this directive.
- A/C/D require product/contract design because the current implementation and documentation deliberately avoid percentages. No feature repair was authorized here.
- E/F/G require direct regression coverage even if existing source mechanisms appear compatible.
- Cross-platform resource backends, Alex-operated candidate acceptance, exact-commit CI/release gates, and publication remain outside this Linux-only closeout and unexecuted.

## Readiness effect

The current Linux machine safely supports the focused PTY run at two Cargo jobs and two test threads, and the real terminal-ownership regression passes. That closes the requested host-capacity and focused PTY checks only.

It does **not** close the requested progress contract. Because six A–H items are not green, this report does not issue the conditional release-readiness sentence and does not authorize a release or prerelease-candidate action.
