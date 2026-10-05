# RRC PRD audit ownership repair

## Objective

Audit the Release Recovery Controller PRD against current production code and
repair the verified gaps behind the live acceptance observations, without
resuming v0.24.5, mutating the live ledger, tagging, publishing, installing,
or touching VRO-19.

## Live failure causes

- Admission text said local verification had started while the persisted epoch
  could still have no local gates and no resolved `release_version`. The
  expensive `cargo check` inside version preparation ran before that visible
  gate list was saved.
- `Active` liveness had no owner pid. Closing the TUI, or an abrupt process
  death during preparation, left `local_verification` plus `liveness=active`
  with nobody left to own it. The RUN panel only consulted the in-process
  worker map, so it could say Ready / No active tasks while the ledger still
  claimed a live controller.
- With OpenAI selected, startup still awaited xAI model discovery whenever
  that adapter reported a credential or a test route. That network wait
  happened before the terminal event loop.

Same-target historical candidate reconciliation was re-run in the release
library suite and still passes. It was not changed.

## Repair

- `preview_release_version` persists the resolved version and a running
  `version-preparation` gate, then `prepare_version_bump` performs the
  workspace check. The gate is marked succeeded only after that check.
- `Active` records the owner pid. A dead, missing, or unreigstered owner
  becomes `OwnerExited` without adding failure evidence. Graceful TUI and ACP
  exit set that state for epochs this process owns.
- TUI RUN uses `release_run_snapshot_for_workspace`, so a recoverable epoch is
  not Ready and a dead owner is not shown as a live controller.
- OpenAI startup no longer calls xAI discovery before the event loop.

## Methods

Worktree: `/home/Alex/Projects/agent-vesper/.worktrees/rrc-prd-full-audit-20261004`

Base: `132408de3e1d1b5f0aa7c0ffe40f20f99bb63175`

```text
cargo test -p vesper-harness --lib release_ -- --test-threads=2
cargo test -p agent-vesper-tui --features integration-test-harness --test openai_startup_responsiveness -- --test-threads=1
```

After the focused tests, the sequential ladder was:

```text
cargo xtask acceptance
acceptance=0
cargo xtask verify
verify=0
cargo xtask msrv
msrv=0
cargo deny --all-features check
deny=0
cargo audit
audit=0
cargo fmt --all -- --check
fmt=0
git diff --check
diff_check=0
```

## Evidence

```text
test release_recovery::tests::dead_owner_is_recoverable_and_not_active_or_a_source_failure ... ok
test release_executor::tests::version_preparation_is_persisted_before_the_expensive_check ... ok
cargo test -p vesper-harness --lib release_
test result: ok. 98 passed; 0 failed
```

```text
test openai_startup_does_not_wait_for_unselected_xai_discovery ... ok
test stalled_openai_discovery_accepts_input_then_applies_catalog ... ok
test stalled_openai_discovery_failure_leaves_input_responsive ... ok
test result: ok. 3 passed; 0 failed
```

Preserved reconciliation examples in that same 98-test run include
`same_target_candidate_only_epoch_adopts_newer_canonical_descendant`,
`same_target_remote_evidence_requires_descendant_for_workspace_replacement`,
and `pushed_tag_still_requires_irreversible_safety`.

## Deviations

The audit did not re-execute hosted five-target matrices or a live GitHub
release. AC rows below map to the current local suite and prior controlled
acceptance named by the PRD; they are not a new live publication proof.

Cognition startup remains a blocking thread before terminal entry, as its
local store migration is synchronous. It was not moved in this repair.

## Unresolved live-only proof

Real v0.24.5 publication, a fresh exact-SHA hosted matrix, and an interactive
session on Alex's machine are not part of this commit.

## Readiness effect

The controller can be release-tested again from this commit. This commit does
not itself release.
