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

# Later user-requested build-only continuation, admitted by the unchanged
# Host Resource Governor after workstation pressure returned to Normal.
VESPER_GOVERNOR_STATE_ROOT=<external-evidence>/governor-state \
  <external-governor-runner> <external-evidence>/cargo-build-all-locked-20261001T102118Z.json \
  expensive <corrected-worktree> -- \
  env CARGO_TARGET_DIR=<external-final-build-target> cargo build --all --locked
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

### Governed locked all-workspace build

Alex later requested one build of the exact corrected work and explicitly kept
the full acceptance suite out of this continuation. Before admission, the source
was clean at:

```text
commit: f7de86858f89810065e5a3cacd7fad5703eaeeb9
subject: docs(release): preserve final audit deviations
tree: 59a44b24883b9907b11804e19f2b8d8b2c0476bd
workspace version: 0.24.4
rejected 3.3.1 commit is ancestor: no
status porcelain entries: 0
```

The unchanged governor initially refused the expensive gate because swap usage
was above its 25% threshold. The initial receipt and 25 timed retries all
reported `pressure`; no Cargo child started. The observed swap use moved from
`3227684352` bytes to `3174670336` bytes but did not recover through passive
waiting. After Alex authorized a one-time swap recycle, the agent-side attempt
failed safely with `sudo: a password is required`. Alex performed the privileged
operation in his own terminal. Because `swapon -a` did not recreate the
generator-owned zram device, the build remained paused while Alex restored it
with:

```sh
sudo systemctl start systemd-zram-setup@zram0.service
```

The pre-build receipt then showed the original swap service restored and normal
admission:

```text
/dev/zram0: 8589930496 bytes total, 0 bytes used, priority 100
systemd-zram-setup@zram0.service: active
RAM available: 17.8 GiB
Pressure: Normal
Action: verification admitted within the RRC resource budget
```

Exactly one requested locked all-workspace build ran with its target and
receipts outside the worktree. It passed:

```text
BUILD_START_UTC=20261001T102118Z
BUILD_EXIT=0
BUILD_END_UTC=20261001T102400Z
Finished `dev` profile [unoptimized + debuginfo] target(s) in 2m 41s
```

The resource receipt independently records:

```text
gate_cost: expensive
admitted: true
exit_code: 0
stopped_for_critical_pressure: false
observations: 650
min_memory_available_bytes: 18417094656
max_process_tree_rss_bytes: 128819200
max_swap_used_bytes: 1220325376
min_disk_available_bytes: 273147060224
pressure_events: ["normal"]
```

Durable external receipts:

```text
/home/Alex/Projects/agent-vesper-release-prep-evidence/20261001-active-epoch-fixed-candidate/cargo-build-all-locked-20261001T102118Z.json
sha256: 4c3d3139b5b0203208ed3b21567af4da5ed3b23b0975c2f1c6cb4cce0070e87c

/home/Alex/Projects/agent-vesper-release-prep-evidence/20261001-active-epoch-fixed-candidate/cargo-build-all-locked-20261001T102118Z.log
sha256: 0f982dc05134f06a1f27b5415ef8dc9de962bc1f42832bea4af2931015e35c10

/home/Alex/Projects/agent-vesper-release-prep-evidence/20261001-active-epoch-fixed-candidate/final-closeout-20261001T102552Z.txt
```

Post-build closeout found the corrected source still clean, workspace version
still `0.24.4`, the rejected `3.3.1` commit still outside ancestry, the live RRC
ledger still byte-identical at SHA-256
`35af7a158b39225e89c5d57b14ec0dd0068afbef66791120e8a9a227c675899d`, and
zram active. The primary checkout remained the separate dirty checkout and was
only observed read-only. No candidate artifact was staged or installed during
that development-profile continuation.

### Final release-profile TUI acceptance candidate

Alex subsequently authorized one final Linux x86-64 release-profile TUI build
for his own real GitHub release acceptance. Product source is exactly:

```text
f7de86858f89810065e5a3cacd7fad5703eaeeb9
```

The worktree was clean at documentation-only descendant
`99124bd514405a1d0151233adf7c24761a1d22e8`; its complete diff from the product
source contained only the five documentation files from the preceding evidence
closeout. Workspace and TUI package versions both remained `0.24.4`.

The existing Host Resource Governor admitted the build with `Pressure Normal`.
The command preserved the production candidate feature set recorded by the prior
provenance-fixed candidate:

```sh
cargo build --locked --release --package agent-vesper-tui \
  --features docker,swarm,bridge,voice-kokoro,voice-flm
```

The release build passed once:

```text
BUILD_START_UTC=20261001T103915Z
BUILD_EXIT=0
BUILD_END_UTC=20261001T104427Z
Finished `release` profile [optimized] target(s) in 5m 11s
```

The governor receipt records:

```text
admitted: true
exit_code: 0
stopped_for_critical_pressure: false
observations: 1220
CARGO_BUILD_JOBS: 2
RUST_TEST_THREADS: 2
min_memory_available_bytes: 18779328512
max_swap_used_bytes: 1241968640
pressure_events: ["normal"]
```

The exact output was copied outside the governor-managed target, flushed,
rehash-verified against the source artifact, and then independently checked by
`SHA256SUMS`:

```text
TUI_PATH=/home/Alex/Projects/agent-vesper-prerelease-candidates/v0.24.4-active-epoch-final-linux-x86_64-20261001T103836Z/agent-vesper-tui
TUI_SHA256=c3d249f7488b9f07e429719ce46cdb138dd3411b96dfbe8278ef6fa00b93f144
TUI_VERSION=agent-vesper-tui 0.24.4
BUILD_PROFILE=release
PRESSURE_RESULT=Normal
agent-vesper-tui: OK
```

The new digest differs from the previous provenance-fixed candidate digest
`b75a52717323a6662ff759956d6338293b60c34e40476c01014517ee2cb3d6dd`.
The preserved artifact is a stripped x86-64 ELF PIE executable, 27,112,944
bytes, mode `0755`.

Durable receipts:

```text
/home/Alex/Projects/agent-vesper-prerelease-builds/v0.24.4-active-epoch-final-release-profile-20261001T103836Z/receipts/governor-preflight.json
sha256: f23f23d0d5c0d213fb1cdb72579f72da2e708c99d251619de99ab752ba528097

/home/Alex/Projects/agent-vesper-prerelease-builds/v0.24.4-active-epoch-final-release-profile-20261001T103836Z/receipts/cargo-build-release-tui.json
sha256: 2f2c8f0f20e275c203641a4df01ef04350af02cd7b6776af41f600362322228d

/home/Alex/Projects/agent-vesper-prerelease-builds/v0.24.4-active-epoch-final-release-profile-20261001T103836Z/receipts/cargo-build-release-tui.log
sha256: d70dd4650472437cd73ca81187abb316cab1431d41fab180ace8a3bffa06eb21

/home/Alex/Projects/agent-vesper-prerelease-candidates/v0.24.4-active-epoch-final-linux-x86_64-20261001T103836Z/BUILD_RECEIPT.txt
```

No natural-language release request was submitted. No RRC epoch, version
mutation, push, tag, publication, Registry update or installation was created.
The live historical ledger and dirty primary checkout remained untouched. This
artifact is preserved only for Alex-operated acceptance.

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
3. At audit time, no live provider, GitHub mutation, release admission,
   release-profile build, installer or application replacement was used as
   verification. The later release-profile build is separately recorded above
   as a build-only candidate for Alex; it did not rerun or replace audit evidence.
4. An ad hoc whole-file Markdown link scan was not a valid clean gate: it did
   not URL-decode percent-escaped paths and also reported pre-existing unrelated
   evidence-index references absent from this isolated candidate. The scoped
   check for every link and provenance path added by this audit passed
   (`ADDED_REPORT_LINKS_OK`; `PROVENANCE_REPORTS_OK count=5`). No unrelated
   historical index entry was rewritten to make this audit appear green.
5. The requested build was delayed by truthful Host Resource Governor pressure.
   Twenty-six pressure receipts were retained. Restoring swap required explicit
   OS approval in Alex's terminal because this session had no passwordless sudo;
   the zram generator service was then explicitly restarted before admission.
6. Both build-only continuations did not rerun `cargo xtask acceptance` or
   `cargo xtask verify`, as Alex explicitly prohibited additional acceptance
   work. Their earlier final-audit receipts remain the scope-appropriate
   behavioral evidence; the new receipts prove only the requested builds and
   artifact identity.

## Unresolved items

- The live historical RRC epoch remains unmodified until Alex authorizes actual
  release admission from the preserved binary.
- The future `0.24.4 -> 0.24.5` candidate commit, push, exact-SHA hosted matrices,
  tag, GitHub Release, assets, Registry update and publication remain unexecuted.
- Local Linux verification and the preserved Linux acceptance candidate do not
  substitute for Alex's user-operated acceptance or future exact-SHA release
  gates.

## Readiness effect

The source repair and its final audit are locally complete. The ordinary request
now resolves the canonical corrected integration, automatically supersedes only
the obsolete local historical epoch, and asks one human-readable question only
for genuinely unrelated or irreversible work. When it must ask, both known
version transitions are explicit. Acceptance is deterministic with respect to
lifecycle ownership while production resource safety remains unchanged. One
checksum-verified release-profile Linux TUI at version `0.24.4` is preserved for
Alex's real release acceptance. This is release-controller readiness and candidate
identity evidence, not authorization or evidence of a release.
