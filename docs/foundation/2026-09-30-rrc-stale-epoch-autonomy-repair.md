# RRC Stale-Epoch Autonomy Repair — Execution Report

## Objective

Repair the live acceptance blocker in which a fresh prerelease TUI and the ordinary-language request `Release all completed work as the next patch.` were rejected because an older persisted epoch remained in `DiagnosingLocalFailure`. New release admission must reconcile the active checkpoint without requiring epoch IDs, `/release status`, `/release cancel`, `/release resume`, worktree selection, or ledger-path knowledge. Preserve old evidence, keep the user's primary checkout unchanged, fail closed around remote irreversible state, run local verification, and build a fresh release-profile TUI without releasing, publishing, tagging, pushing, installing, or touching VRO-19.

## Methods and commands

Inspected the shared admission, ledger and executor paths in:

- `crates/vesper-harness/src/release_recovery.rs`
- `crates/vesper-harness/src/release_executor.rs`
- `apps/agent-vesper-tui/src/main.rs`
- `apps/agent-vesper-acp/src/lib.rs`
- `xtask/src/main.rs`

Implemented reconciliation before new natural-language admission, then ran:

```text
cargo test -p vesper-harness release_recovery::tests:: --no-fail-fast
cargo test -p vesper-harness
cargo clippy -p vesper-harness --all-targets --all-features -- -D warnings
cargo xtask acceptance
cargo xtask architecture
cargo xtask verify
cargo clippy -p xtask --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
```

The fresh candidate used a new external target directory and no incremental state:

```text
CARGO_TARGET_DIR=/tmp/agent-vesper-rrc-stale-epoch-build-20260930T031231Z-01d2df045a7c
CARGO_INCREMENTAL=0
cargo build --release --locked -p agent-vesper-tui
```

The user-operated live acceptance phrase was not submitted.

## Implementation

Natural-language admission now classifies a persisted active record before creating another epoch:

- **Matching recoverable objective:** validates the user-owned release worktree against repository identity, expected HEAD and the bounded planned version-file set; `Preparing` or `DiagnosingLocalFailure` advances to `LocalVerification`; the existing epoch and transition evidence are retained; the background controller launches automatically.
- **Obsolete same-objective prerelease epoch:** detects changed completed-source/diff identity or a release worktree that no longer matches persisted provenance; serializes the complete old record plus reason and replacement identity under `release-recovery/superseded/<repository-key>/`; compares the active ledger epoch/timestamp under the ledger lock; atomically installs the replacement active record; then launches local verification in a new isolated worktree.
- **Published or irreversible state:** any recorded candidate push, tag, publication, or remote/post-push lifecycle state is preserved; admission asks one bounded human-facing clarification and launches nothing.
- **Unrelated active objective:** preserves the active ledger, asks one bounded human-facing clarification, and launches nothing.

Normal admission output does not require or expose an epoch ID, `DiagnosingLocalFailure`, ledger path, or manual resume/cancel command. Both TUI and ACP inherit the behavior from the shared provider-neutral admission path. The active primary checkout is read-only during reconciliation.

## Files

Production and verification changes:

- `crates/vesper-harness/src/release_recovery.rs`
- `xtask/src/main.rs`

Durable contract updates:

- `AGENTS.md`
- `crates/vesper-harness/AGENTS.md`
- `apps/agent-vesper-tui/AGENTS.md`
- `apps/agent-vesper-acp/AGENTS.md`
- `xtask/AGENTS.md`
- `docs/AGENTS.md`
- `docs/foundation/AGENTS.md`
- `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md`
- `docs/foundation/evidence-index.md`
- `docs/foundation/2026-09-30-rrc-stale-epoch-autonomy-repair.md`

Preserved candidate:

- `/home/Alex/Projects/agent-vesper-prerelease-candidates/rrc-stale-epoch-autonomy-20260930T031231Z-01d2df045a7c/agent-vesper-tui`
- `/home/Alex/Projects/agent-vesper-prerelease-candidates/rrc-stale-epoch-autonomy-20260930T031231Z-01d2df045a7c/SHA256SUMS`
- source identity files in the same candidate directory

## Exact evidence

Focused reconciliation suite:

```text
running 32 tests
...
test release_recovery::tests::same_recoverable_epoch_resumes_from_natural_language_without_manual_command ... ok
test release_recovery::tests::obsolete_diagnosing_epoch_is_preserved_and_newer_candidate_is_admitted ... ok
test release_recovery::tests::unrelated_active_release_asks_one_human_clarification_without_mutation ... ok
test release_recovery::tests::irreversible_release_state_is_never_discarded_by_new_admission ... ok

test result: ok. 32 passed; 0 failed; 0 ignored; 0 measured; 136 filtered out
```

Full harness suite:

```text
test result: ok. 166 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out
```

The same full suite also passed the native process restart case:

```text
test release_executor::tests::release_worker_cancel_restart_process_acceptance ... ok
```

Acceptance gate after enrolling all four stale-epoch cases:

```text
acceptance verified: release_recovery::tests::same_recoverable_epoch_resumes_from_natural_language_without_manual_command
acceptance verified: release_recovery::tests::obsolete_diagnosing_epoch_is_preserved_and_newer_candidate_is_admitted
acceptance verified: release_recovery::tests::unrelated_active_release_asks_one_human_clarification_without_mutation
acceptance verified: release_recovery::tests::irreversible_release_state_is_never_discarded_by_new_admission
Acceptance regression gate: 34 exact cases passed in 12805 ms. Offline fixture model cost: zero; live-model effectiveness is not measured.
```

Architecture and repository verification:

```text
architecture boundaries validated for 31 packages
cargo xtask verify: exit 0
cargo fmt --all -- --check: exit 0
cargo clippy -p vesper-harness --all-targets --all-features -- -D warnings: exit 0
cargo clippy -p xtask --all-targets -- -D warnings: exit 0
git diff --check: exit 0
```

The exact stale-candidate regression starts an older same-objective epoch, persists it in `DiagnosingLocalFailure`, creates a newer completed candidate, submits only `Release all completed work as the next patch.`, verifies the old full record in the superseded archive, verifies the replacement at `LocalVerification`, verifies automatic launcher invocation, and byte-compares Alex's dirty primary file before and after. The separate typed mutation regressions in the same full harness run prove complete internal-pin propagation, stale-pin refusal before writes, all-file rollback, Registry propagation and live-workspace manifest/lockfile coverage.

Fresh candidate build:

```text
Finished `release` profile [optimized] target(s) in 1m 40s
agent-vesper-tui: OK
agent-vesper-tui 0.24.4
agent-vesper-tui: ELF 64-bit LSB pie executable, x86-64, dynamically linked, stripped
agent-vesper-tui 24098576 bytes
```

Candidate identity:

```text
Path: /home/Alex/Projects/agent-vesper-prerelease-candidates/rrc-stale-epoch-autonomy-20260930T031231Z-01d2df045a7c/agent-vesper-tui
SHA-256: 99de2f87e50abd696bac8b204539ec752f14ce9a41370ef8d44af7516c545f01
Source HEAD: 01d2df045a7c30253bea8e613e9840b9f62d053b
Build-input tracked diff SHA-256: af0e9d147f2ce638ec99e2c3223c6f0f6eb50874dc8b310608cb1a6bde3c6e19
Build-input status inventory SHA-256: 6dcf0de91cdc9cc7a141d14a8176cde155e8b61264255965f9422e28da6ee64a
```

Final DOX/artifact audit after linking this report:

```text
agent-vesper-tui: OK
agent-vesper-tui 0.24.4
DOCUMENT_LINKS=PASS
VRO19_UNTOUCHED=PASS
VERSION_METADATA_UNCHANGED=PASS
HEAD=01d2df045a7c30253bea8e613e9840b9f62d053b
FINAL_AUDIT=PASS
```

The preserved candidate directory contains the exact pre-build tracked-diff and
status-inventory digests. Only this execution report's candidate receipt changed
after compilation; no Rust source or version metadata changed after the build.

## Constraints held

- No release was started by this repair session.
- No publication, tag, push, install, or production version bump occurred.
- Alex's manual live acceptance was not executed.
- The primary checkout used by the regression remained byte-identical.
- VRO-19 was not inspected or modified.
- No live provider call or production user-state mutation was used for verification.

## Deviations

- A read-only delegated review could not start because the configured OpenAI model was unavailable in the current account model list. Direct source inspection, regression-first implementation and the repository gates supplied the review evidence instead.
- One attempted Cargo invocation supplied multiple test-name filters; Cargo rejected the extra arguments before running tests. The complete `release_recovery::tests::` filter was then run successfully.
- `cargo xtask verify` generated an untracked test credential-lock fixture under `apps/agent-vesper-acp/.config/`; it was removed after verification and before sealing the candidate source inventory.
- Verification is local Linux x86-64. No fresh Windows or macOS run was requested or executed.

## Unresolved items

- Alex-operated live acceptance of the fresh TUI is **NOT RUN**.
- No production release/publication acceptance is claimed.
- Fresh cross-platform execution of this reconciliation path is not recorded.

## Readiness effect

The stale `DiagnosingLocalFailure` checkpoint no longer blocks an ordinary-language release request. The shared RRC now resumes the matching safe objective or archives and replaces an obsolete same-objective prerelease checkpoint automatically, while preserving unrelated and irreversible state behind one bounded clarification. The new release-profile TUI is identity-verified and ready for Alex's manual non-publishing acceptance.
