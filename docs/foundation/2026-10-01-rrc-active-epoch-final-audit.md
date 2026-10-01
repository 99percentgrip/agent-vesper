# RRC active-epoch final audit

**Date:** 2026-10-01

## Objective

Audit the completed natural-language RRC admission repair for:

```text
Release all completed work as the next patch.
```

The audit re-derived the source-resolution and active-epoch invariants, corrected
the remaining mismatch between the documented human clarification contract and
the actual current-objective wording, and removed live workstation pressure from
the acceptance case that owns registered-worker lifecycle rather than resource
admission.

This audit did not invoke release admission against the live repository. It did
not mutate the live RRC ledger, primary dirty checkout, tags, remotes, Registry,
GitHub releases or the installed application. No release-profile replacement
candidate was built.

## Methods and commands

Work remained isolated in:

```text
/home/Alex/Projects/agent-vesper/.worktrees/release-prep-v0.24.4-corrected-2026-10-01
```

The audit used:

```sh
# Detached pre-audit mutant: apply only the strengthened version-transition
# assertions to HEAD, then require the old implementation to fail.
cargo test --manifest-path ../rrc-version-transition-mutant-2026-10-01/Cargo.toml \
  --locked --offline --all-features -p vesper-harness --lib \
  release_recovery::tests::unrelated_active_release_asks_one_human_clarification_without_mutation \
  -- --exact --test-threads=1 --nocapture

cargo test --locked --offline --all-features -p vesper-harness --lib \
  release_recovery::tests::unrelated_active_release_asks_one_human_clarification_without_mutation \
  -- --exact --test-threads=1 --nocapture
cargo test --locked --offline --all-features -p vesper-harness --lib \
  release_recovery::tests::irreversible_release_state_is_never_discarded_by_new_admission \
  -- --exact --test-threads=1 --nocapture
cargo test --locked --offline --all-features -p vesper-harness --lib \
  release_recovery::tests::canonical_objective_supersedes_historical_active_prerelease_without_clarification \
  -- --exact --test-threads=1 --nocapture
cargo test --locked --offline --all-features -p vesper-harness --lib \
  release_recovery::tests::tui_natural_release_retains_registered_controller_through_local_gate_progress \
  -- --exact --test-threads=1 --nocapture
cargo test --locked --offline --all-features -p vesper-harness --lib \
  host_resources_constrained_tests:: -- --test-threads=1 --nocapture
cargo test --locked --offline --all-features -p vesper-harness --lib \
  release_executor::tests::resource_governor_defer_keeps_local_epoch_resumable_without_source_failure \
  -- --exact --test-threads=1 --nocapture
cargo fmt --all -- --check
cargo clippy -p vesper-harness --all-targets --all-features -- -D warnings
cargo xtask acceptance
cargo xtask verify
```

## Files

- `crates/vesper-harness/src/release_recovery.rs`
  - derives the current candidate's known version transition from its committed
    workspace package version and the admitted major/minor/patch bump;
  - retains the bounded `next <bump>` fallback when a trustworthy transition
    cannot be derived;
  - requires both unrelated-objective and irreversible-state clarification
    regressions to show `0.24.4 -> 0.24.5` rather than the vague `next patch`;
  - preserves canonical supersession without clarification and preserves the
    dirty primary checkout byte-for-byte.
- `crates/vesper-harness/src/release_executor.rs`
  - centralizes the pre-existing permissive test policy;
  - adds a `#[cfg(test)]` direct injection used only by the lifecycle fixture,
    so that fixture proves worker registration, real subprocess progression and
    cancellation independently of ambient RAM/swap/disk pressure;
  - leaves production worker construction on `resource_policy_for_worker()`.
- `crates/vesper-harness/AGENTS.md` and `xtask/AGENTS.md`
  - record the lifecycle-fixture/resource-governor acceptance ownership split.
- `docs/foundation/2026-10-01-rrc-active-epoch-final-audit.md`
  - records this audit, correction and exact evidence.
- `docs/foundation/evidence-index.md`,
  `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md`,
  `docs/AGENTS.md`, `docs/foundation/AGENTS.md` and
  `docs/foundation/release-objective-provenance.json`
  - link and own this final audit without changing the release boundary.

Implementation commit verified before documentation closeout:

```text
commit: 2c273fe668d3986846b3d764a33dc7752def861f
subject: fix(release): complete active epoch admission audit
tree: 0fd14b97639765380b0c7310e1f1886cb0972012
```

## Exact evidence

### Audit correction and red-first proof

The preceding report said the clarification named both known version transitions,
but the pre-audit current-objective formatter still emitted `next patch release`.
A detached mutant retained that old implementation and applied only the stronger
assertion. It failed at the intended behavior boundary:

```text
assertion failed: message.contains("0.24.4 -> 0.24.5")
test result: FAILED. 0 passed; 1 failed
EXPECTED_MUTANT_FAILURE_EXIT=101
MUTANT_WORKTREE_REMOVED=yes
```

The corrected implementation passed all three focused active-epoch cases:

```text
unrelated_active_release_asks_one_human_clarification_without_mutation ... ok
irreversible_release_state_is_never_discarded_by_new_admission ... ok
canonical_objective_supersedes_historical_active_prerelease_without_clarification ... ok
```

Each command reported:

```text
test result: ok. 1 passed; 0 failed
```

### Lifecycle acceptance determinism and governor separation

Before the test-only injection, the registered-controller lifecycle case could be
replaced by a truthful production governor deferral. The observed host had either
an undersized 14 GiB `/tmp` filesystem for the 50 GiB disk-headroom policy or
swap usage above the 25% pressure threshold. The acceptance run therefore stopped
at case 33 with:

```text
Paused before workspace-verify: Host Resource Governor deferred unsafe local work
```

That was not a product governor failure; it showed that a lifecycle fixture was
incorrectly coupled to ambient resource admission. With direct test-only policy
injection, the exact real-child lifecycle case passed:

```text
running 1 test
test release_recovery::tests::tui_natural_release_retains_registered_controller_through_local_gate_progress ... ok
test result: ok. 1 passed; 0 failed; finished in 4.65s
```

The independent resource-policy regressions remained green:

```text
running 2 tests
constrained_disk_acceptance_preserves_target_cache_and_withholds_gate ... ok
constrained_memory_acceptance_withholds_expensive_cargo_before_spawn ... ok
test result: ok. 2 passed; 0 failed

running 1 test
resource_governor_defer_keeps_local_epoch_resumable_without_source_failure ... ok
test result: ok. 1 passed; 0 failed
```

Production construction still calls the default/live `resource_policy_for_worker()`;
only the named `#[cfg(test)]` lifecycle entry point receives the permissive policy.

### Static and acceptance gates

The complete release-recovery unit suite passed after the documentation and
provenance links were added:

```text
running 38 tests
test result: ok. 38 passed; 0 failed; 0 ignored; 152 filtered out; finished in 7.67s
```

Strict harness Clippy exited zero:

```text
cargo clippy -p vesper-harness --all-targets --all-features -- -D warnings
Finished `dev` profile
```

The fixed acceptance gate passed every enrolled case, including canonical
supersession, unrelated and irreversible clarification, lifecycle progression,
constrained resource behavior, ACP process inheritance and real PTY ownership:

```text
acceptance progress: 45/45 — rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty
Acceptance regression gate: 45 exact cases passed in 41570 ms. Offline fixture model cost: zero; live-model effectiveness is not measured.
```

The complete repository verification exited zero after running format, workspace
all-target/all-feature Clippy, workspace all-feature tests and the enrolled
acceptance gate:

```text
running: cargo fmt --all --check
running: cargo clippy --workspace --all-targets --all-features -- -D warnings
running: cargo test --workspace --all-features
cargo xtask verify: exit 0
```

## Invariant audit

- Natural-language admission resolves the canonical corrected `v0.24.4` source.
- Equal trees and equivalent implementation diffs collapse before ambiguity.
- Unrelated historical objectives are excluded from candidate competition.
- Explicit canonical supersession archives only local, reversible historical
  state; irreversible push/remote/tag/publication evidence always clarifies.
- Genuine clarification names both human objectives and both known version
  transitions while omitting paths, raw SHAs, refs and internal state names.
- The live/dirty primary checkout is never selected as the mutation workspace.
- Production resource admission remains live, conservative and fail-closed.
- Lifecycle acceptance no longer depends on the workstation's momentary pressure;
  dedicated governor tests still exercise pressure and disk refusal.

## Deviations

1. The first acceptance rerun used the default `/tmp` tmpfs and truthfully deferred
   the lifecycle fixture for insufficient target-filesystem headroom. A second
   attempt with a repository-local `TMPDIR` caused an unrelated acceptance fixture
   to inherit this repository's DOX contract; that attempt was discarded. An
   external home-filesystem temp root removed that inheritance but still exposed
   ambient swap-pressure coupling, leading to the scoped test-policy repair.
2. The prior execution report's statement that both version transitions were
   already shown was too strong for the pre-audit implementation. This report
   preserves that discrepancy explicitly and supplies the red-first correction.
3. No live provider, GitHub mutation, release admission, release-profile build,
   installer or application replacement was used as verification.
4. An ad hoc whole-file Markdown link scan was not a valid clean gate: it did
   not URL-decode percent-escaped paths and also reported pre-existing unrelated
   evidence-index references absent from this isolated candidate. The scoped
   check for every link and provenance path added by this audit passed
   (`ADDED_REPORT_LINKS_OK`; `PROVENANCE_REPORTS_OK count=5`). No unrelated
   historical index entry was rewritten to make this audit appear green.

## Unresolved items

- The live historical RRC epoch remains unmodified until Alex authorizes actual
  release admission.
- The future `0.24.4 -> 0.24.5` candidate commit, push, exact-SHA hosted matrices,
  tag, GitHub Release, assets, Registry update and publication remain unexecuted.
- Local Linux verification does not substitute for those future exact-SHA release
  gates or a user-operated installed-candidate acceptance.

## Readiness effect

The source repair and its final audit are locally complete. The ordinary request
now resolves the canonical corrected integration, automatically supersedes only
the obsolete local historical epoch, and asks one human-readable question only
for genuinely unrelated or irreversible work. When it must ask, both known
version transitions are explicit. Acceptance is deterministic with respect to
lifecycle ownership while production resource safety remains unchanged. This is
release-controller readiness evidence, not authorization or evidence of a release.
