# RRC active-epoch final-integration repair

**Date:** 2026-10-01

> **Final audit correction:**
> [`2026-10-01-rrc-active-epoch-final-audit.md`](2026-10-01-rrc-active-epoch-final-audit.md)
> found that this report had overstated the current-objective clarification:
> the pre-audit implementation still said `next patch release`. The linked
> red-first audit corrects it to the known `0.24.4 -> 0.24.5` transition and
> records the final deterministic acceptance and complete verification receipts.

## Objective

Repair the last natural-language admission blocker for:

```text
Release all completed work as the next patch.
```

When the persisted active epoch is a failed local-only historical prerelease and the resolved source is the explicitly canonical corrected integration that supersedes it, RRC must archive the complete old epoch as superseded evidence, admit the canonical source without clarification, preserve the dirty primary checkout, and start the normal local verification pipeline. Genuinely unrelated objectives and any epoch with push, remote-CI, tag or publication evidence must remain fail-closed behind one human-readable clarification.

This work did not invoke release admission against the live repository and did not run a release, push, tag, publication, installer or replacement-candidate build.

## Methods and commands

Work remained isolated in:

```text
/home/Alex/Projects/agent-vesper/.worktrees/release-prep-v0.24.4-corrected-2026-10-01
```

The live ledger and retained worktrees were inspected read-only. The implementation was verified with:

```sh
cargo test -p vesper-harness release_recovery::tests:: -- --nocapture
cargo test -p vesper-harness \
  release_recovery::tests::canonical_objective_supersedes_historical_active_prerelease_without_clarification \
  -- --exact --nocapture
cargo clippy -p vesper-harness --all-targets -- -D warnings
cargo xtask acceptance
cargo xtask verify
```

A detached temporary mutant worktree removed only the new explicit-supersession branch and ran the new exact regression. The regression failed as required; the temporary worktree was then removed.

## Files

- `crates/vesper-harness/src/release_recovery.rs`
  - persists stable source `variant_id`, canonical-source status and supersession identities in every newly admitted epoch;
  - recognizes an explicitly canonical source whose marker supersedes the active local-only epoch objective or variant;
  - archives that complete historical record through the existing transactional `supersede_and_save` path before activating the replacement;
  - keeps irreversible remote state higher priority than supersession;
  - names both human objectives and known version transitions in the one-question clarification without paths, raw SHAs or internal state names;
  - adds regressions for canonical supersession, durable identities, dirty-primary preservation and irreversible-state clarification.
- `xtask/src/main.rs` enrolls the canonical historical-epoch supersession case in the fixed acceptance gate.
- `crates/vesper-harness/AGENTS.md` and `xtask/AGENTS.md` record the runtime and acceptance contracts.
- `docs/foundation/release-objective-provenance.json` links this report from the canonical source marker.
- `docs/foundation/evidence-index.md` and `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md` link this report.

Implementation commit verified before documentation closeout:

```text
0d1c8313cd8ea796c50bf8cd90b96bc457fe6310
subject: fix(release): reconcile canonical active epochs
tree: 287aea97a8210cb2481f1529f53599c510f50b86
```

## Exact evidence

### Live interrupted epoch, inspected without mutation

The active record remains:

```text
ledger: /home/Alex/.local/state/agent-vesper/release-recovery/abde39a5088456752525d06909d10c51fbafd907be499cb113abf15d9afce206.json
ledger sha256: 35af7a158b39225e89c5d57b14ec0dd0068afbef66791120e8a9a227c675899d
epoch_id: 20260930T070601Z-3f8ea52e7ee6
state: diagnosing_local_failure
objective_id: rrc-source-resolution-ux-repair-20260930
objective_label: RRC source-resolution autonomy repair
source_commit: 3f8ea52e7ee663498b401848444b689d07f09511
source subject: fix(release): bind source selection to completed objective
version transition: 0.24.4 -> 0.24.5
candidate_committed: false
candidate_pushed: false
tag_pushed: false
publication_verified: false
```

The record is therefore local-only and safely supersedable. Its full identity remains on disk unchanged; production admission will archive the complete record before replacing the active ledger.

### Original ambiguous candidate identities

The two final-integration alternatives that originally shared the same historical objective marker were:

1. Corrected `v0.24.4` integration:

   ```text
   commit: d5fe363283b9eef5c7930c3b15ffbf58e94a8389
   tree: e5bfbec5c5f1ca1d5814915a9bf3bcb12034886e
   subject: docs: correct v0.24.4 release preparation evidence
   workspace: /home/Alex/Projects/agent-vesper/.worktrees/release-prep-v0.24.4-corrected-2026-10-01
   ```

2. Rejected `3.3.1` prerelease integration:

   ```text
   commit: 8f2506a4e3daf4ecba05a8f589385715b37cea35
   tree: f9a90f7592f56fbe457d4177550361a116ae2693
   subject: docs: record integrated v3.3.1 prerelease evidence
   workspace: /home/Alex/Projects/agent-vesper-release-prep-2026-10-01
   ```

They diverge after merge base `20584fc5f17bb1d3f1c5b51a86bfded343cb7355`. The corrected line is canonical because it retains the accepted `0.24.4` release-owned version state, excludes rejected version commit `1ea840fb0afccbc369e2a5059cc582f94cf78a02` from ancestry, contains the completed provenance/source-resolution repair, and is explicitly bound by:

```text
objective_id: corrected-v0.24.4-prerelease-integration-20261001
variant_id: corrected-v0.24.4-canonical-source-20261001
canonical_release_source: true
supersedes: rrc-source-resolution-ux-repair-20260930
```

The rejected `3.3.1` candidate is neither selected nor an ancestor of the corrected source.

### Regression and mutation proof

The repaired release-recovery suite passed:

```text
running 38 tests
test result: ok. 38 passed; 0 failed; 0 ignored; 0 measured; 152 filtered out
```

The exact canonical-supersession case passed independently:

```text
running 1 test
test release_recovery::tests::canonical_objective_supersedes_historical_active_prerelease_without_clarification ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 189 filtered out
```

Removing only the supersession classification in a detached mutant made that same test fail at the expected behavior boundary:

```text
explicitly superseded historical prerelease must not clarify
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 189 filtered out
EXPECTED_MUTANT_FAILURE_EXIT=101
MUTANT_WORKTREE_REMOVED=yes
```

### Acceptance and repository verification

Strict harness Clippy exited zero:

```text
cargo clippy -p vesper-harness --all-targets -- -D warnings
Finished `dev` profile
```

The fixed acceptance gate now includes the exact canonical supersession case and passed:

```text
acceptance verified: release_recovery::tests::canonical_objective_supersedes_historical_active_prerelease_without_clarification
Acceptance regression gate: 45 exact cases passed in 22312 ms. Offline fixture model cost: zero; live-model effectiveness is not measured.
```

The final `cargo xtask verify` retry exited zero after format, workspace all-feature Clippy, workspace all-feature tests and the enrolled acceptance gate.

## Behavior proved

- A matching recoverable epoch still resumes automatically.
- A stale same-objective local epoch still archives and replaces automatically.
- A canonical integrated source that explicitly supersedes a historical local-only objective now archives and replaces automatically without asking the user to choose between them.
- The replacement epoch durably records the exact objective ID, variant ID, canonical flag and superseded identity list.
- The dirty primary checkout is byte-preserved and never used as the release mutation workspace.
- Unrelated objectives still produce exactly one clarification without ledger mutation.
- Any push, remote-CI, tag or publication evidence still blocks automatic replacement, even when the new source is canonical and explicitly superseding.
- That irreversible-state clarification names both human objectives and known version transitions while omitting paths, raw SHAs, refs and internal state names.

## Deviations

1. The first final `cargo xtask verify` attempt failed one unrelated voice-pack environment-isolation test, `f9_gate_separates_disabled_from_blocked_with_neural_selected`, after the affected RRC test and strict Clippy had passed. The exact all-features voice test passed immediately when rerun serially, and the complete unmodified `cargo xtask verify` retry passed. No voice source was changed.
2. The live historical ledger was intentionally not mutated. Running natural-language admission would start the real autonomous release controller and could progress toward remote release actions, which this work did not authorize.
3. No new release-profile candidate was built or installed; the change is source-complete and locally verified only.

## Unresolved items

- The live historical ledger has not yet been transactionally archived because no release admission was authorized or executed.
- The future `0.24.4 -> 0.24.5` candidate commit, push, exact-SHA hosted matrices, tag, GitHub Release, assets, Registry update and publication remain unexecuted.
- Linux-local verification does not replace those future exact-SHA cross-platform release gates.

## Readiness effect

The final-integration provenance repair is now complete in source: ordinary natural-language admission can select the canonical corrected integration, supersede the obsolete local-only historical epoch without redundant clarification, retain complete forensic lineage, and start the existing RRC pipeline. Remote-state ambiguity remains fail-closed. This is release-controller readiness evidence, not a release or publication claim.
