# RRC typed local-settlement acceptance repair — execution report

**Date:** 2026-10-02

**Source commit:** `25085a95a38f4df6ed6ec5ab41848e8c2be765b6`

**Owning PRD:** [`../Agent_Vesper_Release_Recovery_Controller_PRD.md`](../Agent_Vesper_Release_Recovery_Controller_PRD.md)

**Verdict:** **STALE ACCEPTANCE EXPECTATION CORRECTED; FOCUSED REGRESSIONS AND REQUIRED STATIC CHECKS PASSED. BROAD ACCEPTANCE WAS NOT RERUN.**

## Objective

Diagnose the one broad-acceptance failure at `release_recovery::tests::tui_natural_release_retains_registered_controller_through_local_gate_progress` without weakening typed local-gate settlement. Preserve the product split:

- a genuine nonzero local compiler/test/build/verification result enters `DiagnosingLocalFailure` with causal local evidence;
- `WatchdogStalled`, `WatchdogDeadline`, and `Cancelled` do not become source-failure evidence;
- resource deferral remains the separate Host Resource Governor path.

No release resume, candidate build, MSRV, broad acceptance rerun, push, tag, publication, installation, or VRO-19 work was authorized.

## Causal trace

The failing lifecycle fixture creates two real local gate subprocesses. `workspace-verify` emits `test_a`/`test_b` progress and succeeds. The second `acceptance` gate starts and sleeps for 30 seconds so the test can inspect registered-worker telemetry. The fixture then deliberately calls `cancel_release_worker`, setting the worker-owned cancellation token.

The resulting path was:

```text
acceptance gate marked Running and persisted
→ NativeReleaseExecutor observes the cancellation token
→ owned child group is stopped
→ run_local_gate returns typed RrcError::Cancelled
→ advance_release matches controller_stop_is_not_source_failure
→ no Failed settlement, local:failed evidence, or LocalVerificationFailed event
→ worker failure persistence ignores the cancellation race
→ checkpoint remains LocalVerification with the acceptance gate Running
```

Therefore the fixture outcome was `Cancelled`, not a source/test/build failure. The old expectation of `DiagnosingLocalFailure` and a failed acceptance gate contradicted the typed cancellation contract. Root-cause classification: `STALE_ACCEPTANCE_EXPECTATION`.

## Methods and commands

Read-only tracing inspected:

- the complete lifecycle fixture and generated `xtask` subprocess;
- `NativeReleaseExecutor` cancellation and process-group settlement;
- `advance_release` local-gate result classification;
- `controller_stop_is_not_source_failure`;
- `persist_worker_failure`;
- the existing watchdog and cancellation regressions.

Focused verification commands:

```text
cargo test -p vesper-harness --lib tui_natural_release_retains_registered_controller_through_local_gate_progress
cargo test -p vesper-harness --lib genuine_local_gate_failure_enters_diagnosing_local_failure
cargo test -p vesper-harness --lib local_gate_watchdog_stop_is_not_persisted_as_source_failure
cargo test -p vesper-harness --lib cancellation_does_not_create_local_failure_evidence
cargo fmt --all -- --check
cargo clippy -p vesper-harness --lib -- -D warnings
git diff --check
```

## Files changed

- `crates/vesper-harness/src/release_recovery.rs`
  - corrected the lifecycle fixture's stale post-cancellation assertions;
  - now requires the checkpoint to remain `LocalVerification`, the interrupted gate to remain `Running`, and source-failure evidence to remain absent;
  - retains the existing registered-worker and gate-progress assertions.
- `crates/vesper-harness/src/release_executor.rs`
  - added `genuine_local_gate_failure_enters_diagnosing_local_failure` to prove the source-failure side explicitly;
  - renamed the existing cancellation regression to `cancellation_does_not_create_local_failure_evidence` without changing its behavior.
- `docs/foundation/2026-10-02-rrc-typed-local-settlement-acceptance-repair.md`
  - records this diagnosis, correction, evidence, and unexecuted boundaries.
- `docs/foundation/evidence-index.md`, `docs/foundation/AGENTS.md`, `docs/AGENTS.md`, and `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md`
  - link and scope this follow-up evidence.

Production code was not changed.

## Exact evidence

The previously failing acceptance case passed after correcting only its stale cancellation assertions:

```text
running 1 test
test release_recovery::tests::tui_natural_release_retains_registered_controller_through_local_gate_progress ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 219 filtered out; finished in 10.90s
```

The explicit source-failure side passed:

```text
running 1 test
test release_executor::tests::genuine_local_gate_failure_enters_diagnosing_local_failure ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 219 filtered out; finished in 0.04s
```

The watchdog and cancellation separation sides passed:

```text
running 1 test
test release_executor::tests::local_gate_watchdog_stop_is_not_persisted_as_source_failure ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 219 filtered out; finished in 0.03s
```

```text
running 1 test
test release_executor::tests::cancellation_does_not_create_local_failure_evidence ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 219 filtered out; finished in 0.01s
```

Static checks:

```text
cargo fmt --all -- --check
(exit 0; no output)
```

```text
cargo clippy -p vesper-harness --lib -- -D warnings
Checking vesper-harness v0.24.5
Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.71s
```

```text
git diff --check
(exit 0; no output)
```

## Deviations

- The first rerun after editing did not compile because the lifecycle fixture attempted to call the private `release_executor::release_worker_terminal` helper across its module boundary. That redundant assertion was removed; the explicit `LocalVerification` state assertion already proves the checkpoint remains in a recoverable nonterminal local stage. The exact test was then rerun and passed.
- The prior broad acceptance invocation remains red at case 38/50 on commit `25085a95`; this work unit intentionally did not rerun `cargo xtask acceptance`.
- No red-before-fix test was added because the broad acceptance receipt itself is the retained red evidence and the defect was a stale expected state, not production behavior.

## Unresolved and deliberately unexecuted items

- `cargo xtask acceptance` was not rerun.
- Full verification, MSRV, supply-chain checks, release builds, hosted matrices, and cross-platform gates were not run.
- No candidate, release resume, live ledger mutation, push, tag, publication, installation, or VRO-19 work occurred.
- Broad acceptance must be rerun separately before claiming that gate green for the correction commit.

## Readiness effect

The focused evidence confirms that typed local settlement still distinguishes real local source failure from controller cancellation and watchdog liveness. The broad acceptance failure was caused by its old expectation, not by an over-broad production bypass. This correction restores the acceptance fixture to the current contract but does not itself establish a green complete acceptance or release gate.
