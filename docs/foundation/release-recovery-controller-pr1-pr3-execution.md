# Release Recovery Controller — PR-1 through bounded PR-3 execution

**Date:** 2026-09-29
**Status:** PARTIAL IMPLEMENTATION; LOCAL FOUNDATION GREEN; PR-4–PR-7 OPEN
**Owning PRD:** [`../Agent_Vesper_Release_Recovery_Controller_PRD.md`](../Agent_Vesper_Release_Recovery_Controller_PRD.md)

> Historical first implementation receipt. Subsequent deterministic-policy,
> resume, required-gate, last-green and AC-23 hardening is recorded in
> [`release-recovery-controller-hardening-execution.md`](release-recovery-controller-hardening-execution.md).
> Its current status supersedes this report's then-open item list.

## Objective

Move `/release` retry authority out of a free-form AgentLoop prompt and establish
the deterministic, provider-neutral RRC foundation: typed lifecycle, immutable
failure evidence, exact-SHA matrix discipline, retry admission, bounded
persistence, and one shared TUI/ACP status surface.

This work unit intentionally does **not** claim the whole PRD complete. It closes
the locally testable PR-1 core and bounded PR-2/PR-3 foundations. End-to-end
release orchestration, automatic refresh/polling, model-repair dispatch,
worktree promotion, external-status transport, publication, controlled live
GitHub acceptance, and the full cross-platform matrix remain open.

## Methods and commands

Repository and contract inspection:

```text
read AGENTS.md
read crates/AGENTS.md
read crates/vesper-harness/AGENTS.md
read apps/AGENTS.md
read apps/agent-vesper-tui/AGENTS.md
read apps/agent-vesper-acp/AGENTS.md
read docs/AGENTS.md
read docs/foundation/AGENTS.md
inspect current /release, /ci, HarnessToolService, TUI CheckpointOp, ACP host command paths
```

Verification executed against the changed tree:

```text
cargo fmt --all
cargo test -p vesper-harness release_recovery --lib
cargo test -p agent-vesper-tui --lib release_routes_to_the_controller_instead_of_a_model_workflow
cargo test -p agent-vesper-acp --lib
cargo check -p agent-vesper-tui -p agent-vesper-acp
cargo clippy -p vesper-harness -p agent-vesper-tui -p agent-vesper-acp --all-targets -- -D warnings
cargo xtask architecture
cargo test -p vesper-harness
cargo test -p agent-vesper-tui --lib
cargo test -p agent-vesper-acp --lib
cargo fmt --all -- --check
```

No provider call, installer, tag, release publication, Actions rerun, or user
installation occurred.

## Files

Production:

- `crates/vesper-harness/src/release_recovery.rs` — typed RRC state/evidence
  model, legal transitions, exact-SHA matrix application, log parsing,
  classification/fingerprints, retry/focused-proof budgets, outage admission,
  watchdog, atomic ledger, structured GitHub port and `gh api` adapter, shared
  commands and status rendering.
- `crates/vesper-harness/src/lib.rs` — exports the shared RRC module.
- `crates/vesper-harness/src/host_commands.rs` — shared `/release` execution and
  `/ci` RRC projection.
- `crates/vesper-harness/Cargo.toml`, `Cargo.lock` — direct `chrono` and
  `thiserror` dependencies used by the typed persisted core.
- `apps/agent-vesper-tui/src/commands.rs` — `/release` now resolves to a typed
  `CheckpointOp::ReleaseControl`; invalid actions fail with bounded usage.
- `apps/agent-vesper-tui/src/main.rs` — executes shared RRC commands and appends
  RRC state to `/ci`.
- `apps/agent-vesper-acp/src/lib.rs` — `/release` executes the same shared RRC
  command without provider dispatch.

Contracts/evidence:

- `AGENTS.md`
- `crates/vesper-harness/AGENTS.md`
- `apps/agent-vesper-tui/AGENTS.md`
- `apps/agent-vesper-acp/AGENTS.md`
- `docs/AGENTS.md`
- `docs/foundation/AGENTS.md`
- `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md`
- `docs/foundation/evidence-index.md`
- this report

Pre-existing unrelated modified/untracked workspace files were not cleaned,
rewritten, or claimed by this work unit.

## Exact evidence

### Core tests

Command:

```text
cargo test -p vesper-harness release_recovery --lib
```

Receipt:

```text
running 10 tests
test release_recovery::tests::community_reports_alone_never_confirm_outage ... ok
test release_recovery::tests::official_degradation_needs_repository_exclusion_and_infrastructure_evidence ... ok
test release_recovery::tests::published_cannot_transition_back_to_publishing ... ok
test release_recovery::tests::partial_matrix_blocks_retry ... ok
test release_recovery::tests::stale_sha_cannot_advance_candidate ... ok
test release_recovery::tests::watchdog_triggers_after_six_stagnant_actions ... ok
test release_recovery::tests::ledger_round_trip_preserves_job_ids ... ok
test release_recovery::tests::secret_canaries_are_redacted ... ok
test release_recovery::tests::equivalent_volatile_logs_have_same_fingerprint ... ok
test release_recovery::tests::complete_matrix_collects_first_causal_log_and_classifies_it ... ok

test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 127 filtered out
```

These tests directly cover partial-matrix blocking, first-causal extraction,
volatile normalization, secret redaction, official-vs-community outage rules,
stale SHA rejection, restart persistence of exact job IDs, publication-boundary
transition discipline, and six-action stagnation.

### TUI route

Command:

```text
cargo test -p agent-vesper-tui --lib release_routes_to_the_controller_instead_of_a_model_workflow
```

Receipt:

```text
running 1 test
test commands::tests::release_routes_to_the_controller_instead_of_a_model_workflow ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 266 filtered out
```

### ACP library suite

Command:

```text
cargo test -p agent-vesper-acp --lib
```

Receipt:

```text
running 61 tests
...
test result: ok. 61 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### Host compilation

Command:

```text
cargo check -p agent-vesper-tui -p agent-vesper-acp
```

Receipt:

```text
Checking vesper-harness v0.24.4
Checking agent-vesper-tui v0.24.4
Checking agent-vesper-acp v0.24.4
Finished `dev` profile [unoptimized + debuginfo]
```

### Architecture and complete affected-crate suites

Commands and receipts:

```text
cargo xtask architecture
architecture boundaries validated for 31 packages

cargo test -p vesper-harness
test result: ok. 135 passed; 0 failed; 2 ignored

cargo test -p agent-vesper-tui --lib
test result: ok. 267 passed; 0 failed; 0 ignored

cargo test -p agent-vesper-acp --lib
test result: ok. 61 passed; 0 failed; 0 ignored

cargo fmt --all -- --check
(exit 0; no output)
```

The two ignored harness tests are pre-existing explicit environment gates: a
workspace-layout acceptance probe and a contained real-runtime browser probe.
Neither is RRC acceptance.

### Strict lint

Command:

```text
cargo clippy -p vesper-harness -p agent-vesper-tui -p agent-vesper-acp --all-targets -- -D warnings
```

Receipt:

```text
Checking vesper-harness v0.24.4
Checking agent-vesper-acp v0.24.4
Checking agent-vesper-tui v0.24.4
Finished `dev` profile [unoptimized + debuginfo]
```

## Requirement and acceptance trace

| PRD item | Current evidence | Verdict |
|---|---|---|
| Typed lifecycle and legal transitions | `ReleaseRecoveryState`, `legal_transition`, transition ledger | Implemented foundation |
| Complete-matrix rule | `GateRecord::matrix_complete`, `refresh_remote_evidence`, partial-matrix test | Implemented foundation |
| First causal evidence and fingerprints | parser/classifier/fingerprint functions and focused tests | Implemented foundation |
| Same fingerprint/no change blocks retry | `retry_admission` deterministic guard | Implemented foundation; exhaustive combination matrix still open |
| Focused proof and retry budget | `RepairAttempt`, `FocusedProofStatus`, `RetryBudget`, admission token | Implemented foundation; host verifier binding open |
| Exact-SHA correlation | `apply_matrix` stale-SHA refusal and test | Implemented |
| Persistence/restart | atomic JSON ledger + exact job-ID round trip | Implemented locally; real process restart acceptance open |
| Secret redaction | bounded persisted excerpt and canary test | Implemented locally |
| Outage hierarchy | pure admission/verdict function and tests | Implemented policy; official-status transport open |
| TUI/ACP state parity | both call shared harness command/status implementation | Source and unit verified; process parity acceptance open |
| `/release` not a prompt | TUI typed op + ACP host executor | Implemented |
| End-to-end release lifecycle authority | no complete local-gate/repair/tag/publish coordinator yet | OPEN |
| Controlled real GitHub acceptance | not run | OPEN |
| AC-23 demonstrated incident fixture | not yet added | OPEN |

## Deviations

- The PRD proposes PR-1 through PR-7. This work unit does not collapse those
  phases into a false completion claim.
- `GhCliEvidenceAdapter` uses structured `gh api` JSON and implements run/job/log
  reads and rerun endpoints, but no live GitHub call was made in foundation
  verification.
- `/release resume` currently restores the checkpoint and explicitly reports
  that a remote refresh is required; the automatic host-owned refresh loop is
  not yet wired.
- `/release retry` reports deterministic eligibility but does not perform a
  rerun. This is fail-closed and preserves the existing permission boundary.

## Unresolved items

1. PR-4: wire local verification, candidate creation, complete gate discovery,
   model repair tasks, focused verifier, recovery worktree promotion, tagging,
   publication and post-release closeout through one controller owner.
2. PR-5: add bounded official GitHub-status transport, direct API evidence,
   pause/resume refresh, and the 10–15 second budget.
3. PR-6: add dedicated status presentation/events beyond shared text and real
   TUI/ACP process parity/restart tests.
4. PR-7: add exhaustive transition/admission tables, all GitHub/log fixtures,
   AC-23 incident fixture, controlled real repository acceptance, and required
   cross-platform/exact-release regression gates.
5. Intercept or structurally fence direct release-affecting shell/GitHub
   mutations while an RRC epoch is active so the model cannot bypass admission.
6. Run `cargo xtask architecture`, complete workspace verification and the PRD's
   controlled live acceptance before any completion or release-ready claim.

## Readiness effect

Agent Vesper now has a real provider-neutral RRC foundation, persistent shared
state, deterministic retry guards, structured GitHub evidence seams, and no
longer turns `/release` directly into an unconstrained model workflow. This is
a meaningful anti-loop boundary, but it is **not yet an autonomous release
controller**. Release readiness for the full PRD remains **blocked** on the open
PR-4 through PR-7 work above.
