# RRC persisted progress milestones — execution report

**Date:** 2026-09-30  
**Status:** Implemented and scoped locally verified; no release action was performed.  
**Scope boundary:** This report covers the durable display/read-resume projection for the Release Recovery Controller (RRC). It does not authorize a release transition or claim a release, publication, tag, push, installation, prerelease-candidate build, live-provider call, cross-platform execution, VRO-19 work, or modification of Alex's primary checkout.

## Objective

Make a running RRC epoch visibly and truthfully progress in both hosts without deriving status from timers or release-admission prose. The shared ledger must retain a bounded typed phase, active gate, local/remote completion counts, and monotonically sequenced concise milestones. TUI must project each persisted milestone to its conversation once per epoch/sequence and show the same state in RUN. ACP `/release status` and `/ci` must expose the same host-neutral snapshot.

## Methods and implementation

### Shared, persisted projection

`crates/vesper-harness/src/release_recovery.rs` defines the serde-persisted `ReleaseProgress`, `ReleaseProgressPhase`, and `ReleaseProgressMilestone` values.

- `ReleaseRecoveryRecord::refresh_progress` derives phase, headline, active local gate, succeeded local gates, and terminal remote jobs solely from the typed RRC record and its gate/job data.
- `ReleaseRecoveryRecord::note_progress_milestone` appends redacted, 512-character-bounded summaries with a monotonically increasing per-epoch sequence. Retention is capped at `MAX_PROGRESS_MILESTONES` (32).
- Every ordinary state transition creates a milestone. The executor additionally records local-gate start/settlement, resource deferral, and matrix updates. The display projection has no mutation, retry, or release-admission authority; typed RRC state and immutable evidence remain authoritative.
- Old ledger records deserialize with the default projection. Read-only status rendering refreshes a clone, so `/release status` does not create a user-state write merely to display an older checkpoint.

### Shared host use

`crates/vesper-harness/src/release_executor.rs` carries the current `ReleaseProgress` through `ReleaseWorkerActivity` and `ReleaseWorkerSnapshot`. Its common text renderer emits the current phase/count row and the three most recent milestones. ACP reaches that renderer through its existing shared release-status command route, so no ACP-only lifecycle representation was introduced.

`apps/agent-vesper-tui/src/main.rs` retains the latest `(epoch_id, sequence)` cursor in the TUI session. On each ordinary render cycle it receives the registered worker snapshot and projects only unseen persisted milestones into the Conversation transcript. A newly opened TUI session seeds at most the three newest items, rather than repeating the entire retained history. `apps/agent-vesper-tui/src/ui.rs` includes the same progress phase/counts and recent milestones in the RUN task panel. Its width-constrained row uses `ReleaseProgressPhase::compact_label` while retaining both local and CI counts; full labels remain available in the shared text status and the Stage row.

## Files changed for this capability

- `crates/vesper-harness/src/release_recovery.rs` — typed persisted progress model, deterministic derivation, bounded monotonic milestones, and text status projection.
- `crates/vesper-harness/src/release_executor.rs` — executor milestone emission, registered-worker snapshot transport, and shared text RUN rendering.
- `crates/vesper-harness/src/release_progress_tests.rs` — serialization, shared text-host snapshot, and bounded-sequence regressions.
- `crates/vesper-harness/src/lib.rs` — test-module registration.
- `apps/agent-vesper-tui/src/main.rs` — once-only TUI conversation projection by epoch/sequence.
- `apps/agent-vesper-tui/src/ui.rs` — RUN-panel progress/milestone rendering regression assertion.
- `apps/agent-vesper-acp/src/lib.rs` and `crates/vesper-harness/src/host_commands.rs` — retained shared controller-status composition rather than a separate ACP progress model.
- `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md`, `evidence-index.md`, and applicable `AGENTS.md` files — stable ownership, requirement trace, and evidence linkage.

## Exact verification evidence

Commands below were run in the isolated `rrc-resource-governor` worktree. No release command, network operation, provider call, installer, tag, push, or candidate build was invoked.

```text
cargo fmt --all && cargo fmt --all -- --check && CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo test -p vesper-harness --lib release_progress_tests -- --nocapture && CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo test -p agent-vesper-tui --lib registered_release_task_replaces_ready_and_no_active_tasks -- --nocapture

running 3 tests
test release_progress_tests::progress_history_is_bounded_and_monotonic ... ok
test release_progress_tests::text_host_run_status_uses_the_same_persisted_progress_snapshot ... ok
test release_progress_tests::typed_progress_tracks_local_gate_and_survives_ledger_serialization ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 180 filtered out; finished in 0.25s

running 1 test
test ui::tests::registered_release_task_replaces_ready_and_no_active_tasks ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 267 filtered out; finished in 0.02s

CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo test -p agent-vesper-acp --lib -- --nocapture
running 61 tests
...
test tests::workspace_root_prefers_the_primary_root ... ok
test tests::xai_composition_uses_the_adapter_catalog_for_image_capability ... ok

test result: ok. 61 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.39s
```

The three harness regressions establish all of the following from source-appropriate evidence:

1. a `LocalVerification` record with one running local gate derives the expected typed phase, active gate, and counts;
2. serde round-trip preserves the persisted projection and the common text-host renderer reads the same snapshot; and
3. milestone retention remains bounded while sequence values stay monotonic after old rows are dropped.

The existing TUI task regression additionally passed with a non-default typed `LocalVerification` projection and asserts the rendered `Progress` label plus its `4/5 CI` count; it retains the active-state guard against contradictory `Ready` and `No active tasks` labels.

### Final audit correction

The first strengthened TUI assertion expected the longer `4/5 CI jobs` suffix and failed with exit 101: the fixed-width RUN rail rendered `Progress  Local verification · 3/7 …`, hiding the remote count. This was a real presentation gap in the claimed shared projection, not a successful result. The correction adds the shared `compact_label` display helper and renders `Local · 3/7 local · 4/5 CI`, which retains both counts in the same constrained row. The final rerun below is the valid TUI receipt; the failed pre-correction attempt is not treated as acceptance evidence.

## Deviations and unresolved items

1. This work unit used focused harness/TUI regressions, the ACP library suite, and formatting. `cargo xtask acceptance`, `cargo xtask verify`, the full workspace suite, ACP process-suite coverage, and cross-platform CI were **not rerun** after the milestone rendering change. The earlier Host Resource Governor receipt records a separate 41-case acceptance run; it is not represented here as fresh milestone-change verification.
2. The ACP library suite confirms the composition still compiles and its 61 existing tests pass, but no ACP process test specifically drives `/release status` with an active worker. Shared snapshot/rendering regression evidence is direct; editor-host end-to-end interaction remains unexecuted.
3. No live RRC epoch was started to obtain terminal screenshots or an interactive acceptance trace. This avoids release-oriented mutation and constrained-host load; the UI evidence is deterministic source-level regression evidence only.
4. Rust `cargo fmt --all` updated the repository's Rust formatting before the successful check. No semantic behavior was changed by that formatting action.

## Readiness effect

A persisted RRC checkpoint now contains enough bounded, host-neutral information to state what is actually happening: phase, active gate, local/remote gate completion, and recent controller-owned milestones. TUI and ACP use that same snapshot, and TUI preserves the once-only event identity across redraws and restarts. This removes blank or stale progress presentation without inventing percentages, recovery, success, or authorization from elapsed time.

The capability improves observability and safe resumption only. All existing exact-commit, acceptance, platform, and publication requirements remain unchanged, and this report does not authorize any release action.
