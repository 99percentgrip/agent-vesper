# RRC explicit-version and stale-epoch repair — execution report

**Date:** 2026-10-03  
**Status:** Implemented and locally verified; no release action performed  
**Owning PRD:** [`../Agent_Vesper_Release_Recovery_Controller_PRD.md`](../Agent_Vesper_Release_Recovery_Controller_PRD.md)  
**Authoritative base:** `f8c6c4ad3307120ea6a70c4340eec0fc45fbcc17`  
**Isolated worktree:** `.worktrees/rrc-explicit-version-stale-epoch-20261003`

## Objective

Repair Release Recovery Controller behavior so natural-language and `/release`
requests may select an exact stable version, including `0.24.5` and `v0.24.5`,
without silently converting that request into the next patch. Preserve ordinary
`patch`, `minor`, and `major` behavior.

An exact target equal to the workspace version must be a true no-op: validate the
complete version graph without rewriting manifests, `Cargo.lock`, or Registry
metadata, and reuse clean canonical `HEAD` rather than creating a synthetic
`Release vX.Y.Z` commit.

For an unpublished release target, treat candidate push receipts, CI runs/jobs/URLs,
publication workflow IDs, and failure evidence as preservable history rather than
publication boundaries. A newer canonical strict descendant for the same resolved
target may start a replacement epoch only after the complete old record is archived.
Conflicting targets with remote evidence, non-descendant or noncanonical replacement
sources, repository mismatch, pushed target tags, and actual publication must still
clarify without mutation.

Do not commit, push, tag, publish, install, resume a live release, mutate a live
`v0.24.5` epoch, or change VRO-19.

## Implementation

### Exact target selection and no-op execution

- `ReleaseVersionSelector::{Patch, Minor, Major, Exact(String)}` carries the typed
  objective while retaining schema-v1 compatibility through the serialized `bump`
  field.
- Stable exact inputs normalize optional `v`/`V`; malformed, prerelease-like,
  leading-zero, and decreasing versions fail closed.
- `selected_release_version` accepts equality and rejects only a target lower than
  the workspace version. Bump arithmetic remains checked and unchanged:
  `0.24.5 + patch -> 0.24.6`.
- `build_version_mutation_plan` produces an empty file mutation set when
  `before == after`, while still validating all workspace manifests, internal pins,
  Registry metadata, and release URLs.
- `prepare_version_bump` skips mutation and rollback writes for an equal target,
  runs locked validation, verifies that `Cargo.lock` bytes remain identical, and
  omits `Cargo.lock` from the mutation receipt.
- `commit_candidate` returns the exact existing `HEAD` when the validated release
  workspace is clean, so an already-versioned target gets no synthetic release
  commit.
- Natural-language admission, harness workspace-command routing, and TUI command
  routing/help accept `X.Y.Z` and `vX.Y.Z`. Exact targets appear in status and typed
  progress before mutation.

### Same-target stale epoch reconciliation

- Reconciliation compares resolved target versions rather than selector spelling;
  historical `Patch` from `0.24.4` and new `Exact("0.24.5")` are the same target.
- Candidate push/ref data, required-gate run IDs/attempts/jobs/URLs, publication
  workflow run IDs, remote-state history, and failure records are historical evidence.
  They do not by themselves establish a tag/publication boundary.
- A changed source for that same target must be a strict Git descendant. When remote
  candidate/CI/failure evidence exists, the replacement must also be marked as the
  canonical release source.
- `ReleaseLedger::supersede_and_save` archives the complete previous record before
  installing the replacement record. Tests compare archived candidate refs, complete
  gate/job/URL records, publication run IDs, and failure records to the prior epoch.
- Different targets with candidate/CI history, repository mismatch, non-descendants,
  noncanonical remote-history replacements, pushed tags, and published releases
  preserve the active ledger bytes and return bounded clarification.
- Historical remote evidence also blocks replacement when the persisted release
  workspace is missing or no longer matches but the selected source has not advanced:
  the same commit cannot satisfy the required strict-descendant proof.
- Candidate push remains a normal fast-forward `git push origin HEAD:main`; no
  force-push or history-rewrite path was added.

### Frozen slash-command oracle contract

The unrelated global rewrite of the frozen slash-command oracle contract was
reverted. Both of these files are byte-identical to the authoritative base:

- `crates/vesper-domain/src/slash_commands.rs`
- `crates/vesper-domain/AGENTS.md`

Runtime `/release X.Y.Z` support remains in TUI routing/help and the shared harness
controller path. The frozen 28-command oracle names, descriptions, order, and their
contract were not changed.

### Acceptance inventory repair

The acceptance inventory still named the removed coarse regression
`irreversible_release_state_is_never_discarded_by_new_admission`. Its current
semantic replacement is
`different_release_target_with_pushed_candidate_requires_clarification`; the xtask
inventory and owning DOX contract were updated. The fixed gate remains 50 exact
cases and still fails closed for a missing or zero-match case.

## Files changed

### Runtime and tests

- `crates/vesper-harness/src/release_recovery.rs`
- `crates/vesper-harness/src/release_executor.rs`
- `crates/vesper-harness/src/release_progress_tests.rs`
- `apps/agent-vesper-tui/src/commands.rs`
- `xtask/src/main.rs`

### DOX, PRD, and evidence

- `apps/agent-vesper-acp/AGENTS.md`
- `apps/agent-vesper-tui/AGENTS.md`
- `crates/vesper-harness/AGENTS.md`
- `xtask/AGENTS.md`
- `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md`
- `docs/AGENTS.md`
- `docs/foundation/AGENTS.md`
- `docs/foundation/evidence-index.md`
- `docs/foundation/2026-10-03-rrc-explicit-version-target-repair.md`

Root `AGENTS.md`, `apps/AGENTS.md`, `crates/AGENTS.md`, and the domain DOX were
intentionally left unchanged: no parent ownership/index changed, and the attempted
domain oracle-contract exception was reverted rather than made durable.

## Regression evidence

The implementation work inherited red-first receipts from the immediately preceding
audit. The typed-selector tests initially failed against the old string-only
objective model:

```text
error[E0026]: variant `release_recovery::ReleaseIntentDecision::Admit` does not have a field named `version`
error[E0609]: no field `version` on type `ReleaseObjective`
error[E0433]: cannot find type `ReleaseVersionSelector` in this scope
error: could not compile `vesper-harness` (lib test) due to 8 previous errors
```

TUI exact-target routing and stale remote-evidence safety also had independent red
receipts:

```text
left: Error("Usage: /release [patch|minor|major|status|resume|cancel|evidence|retry]")
right: Checkpoint(ReleaseControl { argument: "0.25.0" })
test result: FAILED. 0 passed; 1 failed

persisted remote evidence must not be replaced
test result: FAILED. 0 passed; 1 failed
```

Archived receipts:

- `/tmp/rrc-explicit-version-red.log`
- `/tmp/rrc-explicit-version-slash-red.log`
- `/tmp/rrc-explicit-version-command-route-red.log`
- `/tmp/rrc-explicit-version-remote-evidence-red.log`

Named final regressions include:

- `explicit_unpublished_version_does_not_bump_again`
- `next_patch_still_bumps_normally`
- `version_plan_rejects_decreasing_or_unstable_exact_targets`
- `same_target_candidate_only_epoch_adopts_newer_canonical_descendant`
- `same_target_remote_ci_evidence_does_not_force_clarification`
- `same_target_remote_failure_evidence_does_not_force_clarification`
- `same_target_non_descendant_requires_clarification`
- `same_target_noncanonical_descendant_requires_clarification`
- `same_target_remote_evidence_requires_descendant_for_workspace_replacement`
- `repository_mismatch_requires_clarification_without_mutation`
- `pushed_tag_still_requires_irreversible_safety`
- `publication_still_requires_irreversible_safety`
- `exact_live_admission_sentence_adopts_same_unpublished_target`

The live-sentence test uses the exact requested sentence:

```text
Release the current unpublished v0.24.5 from main.
```

It proves exact-target selection, canonical descendant adoption, an already-versioned
`Cargo.toml`, and complete archival of the historical candidate/CI record without
starting a real release. The dirty-primary source-selection regression also asserts
that the isolated release worker retains the resolved `test/repo` repository slug and
exact `https://github.com/test/repo.git` origin while the primary checkout bytes remain
unchanged.

### Final-audit regression and correction

The final hand audit found one narrower path not covered by the first matrix: when an
epoch had historical remote evidence, its selected source commit was unchanged, and
its persisted release workspace became dirty or otherwise mismatched, reconciliation
could create a replacement epoch even though the unchanged source was not a strict
descendant. The new regression failed on the pre-correction code:

```text
remote-history workspace replacement must not launch
test release_recovery::tests::same_target_remote_evidence_requires_descendant_for_workspace_replacement ... FAILED
test result: FAILED. 0 passed; 1 failed
```

Reconciliation now preserves the active ledger and asks for clarification in that
case. A replacement with historical remote evidence is therefore possible only through
the existing changed-source branch, where both strict-descendant and canonical-source
proofs are mandatory. The focused correction passed:

```text
test release_recovery::tests::same_target_remote_evidence_requires_descendant_for_workspace_replacement ... ok
test result: ok. 1 passed; 0 failed
```

Receipts:

- `/tmp/rrc-explicit-version-final-audit-red.log`
- `/tmp/rrc-explicit-version-final-audit-green.log`

## Verification receipts

All commands below ran from the isolated worktree. No provider or live release
operation was used.

### Required first corrective checkpoint

After equality/no-op preparation and clean-HEAD reuse were corrected:

```text
cargo test -p vesper-harness --lib

test result: ok. 233 passed; 0 failed; 2 ignored
```

After reconciliation coverage was strengthened, an over-broad canonical-source
condition caused two existing local-only replacement tests to fail. The condition
was narrowed to epochs with historical remote evidence, the two tests were rerun
individually, and the complete library suite then passed:

```text
release_recovery::tests::obsolete_diagnosing_epoch_is_preserved_and_newer_candidate_is_admitted ... ok
release_recovery::tests::tui_natural_release_retains_registered_controller_through_local_gate_progress ... ok

test result: ok. 235 passed; 0 failed; 2 ignored
```

### Requested final ladder

```text
cargo fmt --all -- --check
exit status 0

cargo clippy -p vesper-harness --lib --tests -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.91s

cargo test -p vesper-harness release_executor -- --nocapture
test result: ok. 33 passed; 0 failed; 0 ignored
release_worker_cancel_restart_process_acceptance ... ok

cargo test -p vesper-harness release_recovery -- --nocapture
test result: ok. 57 passed; 0 failed; 0 ignored

cargo test -p vesper-harness
test result: ok. 235 passed; 0 failed; 2 ignored
all harness integration tests and doc-tests passed; command exit status 0

cargo test --workspace
command exit status 0; all workspace unit, integration, process, PTY, and doc tests passed

cargo xtask acceptance
acceptance progress: 37/50 — release_recovery::tests::different_release_target_with_pushed_candidate_requires_clarification
acceptance progress: 50/50 — rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty
Acceptance regression gate: 50 exact cases passed in 68199 ms. Offline fixture model cost: zero; live-model effectiveness is not measured.
```

The final workspace and acceptance logs are:

- `/tmp/rrc-explicit-version-cargo-test-workspace-final.log`
- `/tmp/rrc-explicit-version-acceptance-final.log`

The acceptance inventory edit also received its focused check:

```text
cargo test -p xtask
test result: ok. 6 passed; 0 failed
```

### Post-audit current-source rerun

After adding the strict-descendant workspace-replacement guard and the explicit
same-origin dirty-primary assertion, the affected and broad gates were rerun on the
final source:

```text
cargo fmt --all -- --check
exit status 0

cargo clippy -p vesper-harness --lib --tests -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.19s

cargo test -p vesper-harness release_recovery -- --nocapture
test result: ok. 58 passed; 0 failed; 0 ignored

cargo test --workspace
vesper-harness: test result: ok. 236 passed; 0 failed; 2 ignored
command exit status 0; all workspace unit, integration, process, PTY, and doc tests passed

cargo xtask acceptance
acceptance progress: 50/50 — rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty
Acceptance regression gate: 50 exact cases passed in 82044 ms. Offline fixture model cost: zero; live-model effectiveness is not measured.
```

Final current-source logs:

- `/tmp/rrc-explicit-version-release-recovery-final-audit.log`
- `/tmp/rrc-explicit-version-cargo-test-workspace-final-audit.log`
- `/tmp/rrc-explicit-version-acceptance-final-audit.log`

The links added or changed by this work unit were checked directly:

```text
test -f docs/foundation/2026-10-03-rrc-explicit-version-target-repair.md && \
  test -f docs/Agent_Vesper_Release_Recovery_Controller_PRD.md
changed RRC report/index/PRD link targets exist
```

### Final audit checks

The manual final audit re-derived these invariants from the implementation rather
than from test names:

1. equality uses `<`, not `<=`, for exact-target rejection;
2. equality creates no version-file mutations and reports no generated lockfile;
3. locked validation checks the full version graph and exact lockfile bytes;
4. a clean already-versioned workspace returns exact `HEAD` without `git commit`;
5. target comparison resolves historical bump selectors before comparison;
6. remote evidence is archived whole through `supersede_and_save`;
7. only pushed tags or actual publication create the immutable boundary;
8. descendant, canonical-source, objective, repository, and target guards are
   evaluated before replacement;
9. remote-history workspace replacement cannot bypass strict-descendant proof by
   reusing the unchanged source commit;
10. the dirty-primary isolated worker retains the resolved repository slug and origin;
11. no force-push command exists in the candidate path; and
12. the frozen domain oracle files have no diff from the authoritative base.

Exact final checks:

```text
git diff --check
exit status 0

git diff --exit-code f8c6c4ad3307120ea6a70c4340eec0fc45fbcc17 -- \
  crates/vesper-domain/src/slash_commands.rs crates/vesper-domain/AGENTS.md
exit status 0
```

## Deviations and unresolved items

1. The first post-reconciliation `cargo test -p vesper-harness --lib` failed two
   established local-only stale-source tests because the canonical-source guard was
   initially applied without conditioning it on remote history. The condition was
   corrected; both focused tests and the complete library/package/workspace suites
   subsequently passed.
2. The first `cargo xtask acceptance` reached case 36 and then failed closed because
   the fixed inventory still referenced the removed coarse test name
   `irreversible_release_state_is_never_discarded_by_new_admission`. The inventory
   now references the current conflicting-target regression. `cargo test -p xtask`,
   a fresh post-edit `cargo test --workspace`, and all 50 acceptance cases passed.
3. The final audit exposed the missing-workspace bypass described above. Its failing
   regression was preserved, the guard was corrected, and focused plus broad final-source
   gates passed.
4. Independent delegated review could not run because the selected OpenAI model was
   unavailable in the current account model list. No review result was invented. The
   manual final audit and deterministic gates were completed instead.
5. A generic whole-file Markdown link scan returned nonzero on unrelated historical
   `evidence-index.md` targets, including URL-encoded paths and references absent from
   this worktree. The links introduced or changed by this correction were checked
   directly and exist; unrelated index repair was outside the audited minimal scope.
6. `cargo xtask verify`, MSRV, `cargo deny`, `cargo audit`, hosted five-target and
   exact-SHA release matrices, installer tests, and live provider calls were not
   requested or run. Passing local workspace tests and acceptance is not represented
   as hosted release evidence.
7. No commit or remote mutation was performed. No live RRC epoch was started or
   resumed, so the current `v0.24.5` release state—if any—was not touched.
8. VRO-19 was not changed.

## Readiness effect

The scoped implementation is locally ready for review. Exact stable targets now
retain their requested identity, including the already-versioned no-op case, while
ordinary bump selection remains intact. Unpublished same-target candidate/CI/failure
history can be superseded only by the safe canonical-descendant path and is archived
in full; a missing or mismatched release workspace cannot bypass that proof. Pushed
tags and actual publication remain immutable boundaries. The frozen slash-command
oracle contract is restored unchanged, runtime `/release X.Y.Z` support remains, and
every requested local verification command passed on final source. This report makes
no claim of commit, push, tag, publication, installation, hosted release-matrix
readiness, or live-release completion.
