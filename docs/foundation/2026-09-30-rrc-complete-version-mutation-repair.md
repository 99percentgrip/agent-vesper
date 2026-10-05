# RRC Complete Version Mutation Repair

Status: **IMPLEMENTED AND LOCALLY VERIFIED; NO RELEASE PERFORMED**

## Objective

Repair Release Recovery Controller (RRC) version preparation so it cannot advance with a partially bumped Cargo workspace. The release mutation must cover the root workspace version, every workspace-owned exact path dependency in every Cargo dependency table, generated Registry metadata, and the regenerated lockfile as one fail-closed operation before local release gates, commit, tag, push, or publication.

The observed defect was concrete: package metadata could report `agent-vesper-acp 0.24.5` while its exact internal `vesper-acp` requirement remained `=0.24.4`, causing Cargo resolution to fail before release gates.

## Methods and commands

The audit read the root and local DOX contracts, inspected `NativeReleaseExecutor::prepare_version_bump`, inventoried all workspace member manifests and Registry metadata, and counted the current internal exact pins.

Commands executed from `/home/Alex/Projects/agent-vesper/.worktrees/rrc-autonomy-repair`:

```text
python3 inventory over Cargo.toml, apps/*/Cargo.toml, crates/*/Cargo.toml and xtask/Cargo.toml
cargo fmt --all -- --check
cargo test -p vesper-harness
cargo clippy -p vesper-harness --all-targets --all-features -- -D warnings
cargo xtask acceptance
cargo xtask architecture
git diff --check
```

A delegated read-only inventory was attempted first, but the configured worker provider rejected its unavailable selected model. The audit then proceeded directly against repository sources; no scope or evidence was omitted because of that tooling deviation.

## Implementation

`crates/vesper-harness/src/release_executor.rs` now:

1. Builds a typed `VersionMutationPlan` entirely in memory before writing.
2. Parses the root `[workspace] members` inventory and requires every declared member manifest to exist.
3. Requires the root `[workspace.package] version` to equal the expected source version.
4. Requires every member package to inherit `workspace.package.version`.
5. Resolves each local `path` dependency against canonical workspace member directories and requires its exact version pin to equal the source workspace version. This applies independently of normal, development, build, optional, and target-specific dependency sections.
6. Leaves non-path/external dependency requirements unchanged, even if their numeric version equals the workspace version.
7. Requires `registry/agent.json` and every Registry `archive` URL to match the source version before planning replacements.
8. Applies all planned manifest and Registry replacements through atomic per-file replacement with transaction rollback of every already-written file on failure.
9. Regenerates `Cargo.lock` only after the complete metadata mutation has been applied.
10. Runs `cargo check --workspace --all-targets`, then `cargo metadata --locked --no-deps --format-version 1` and validates that every owned package resolves at the new version.
11. Restores every planned file and the original lockfile bytes if Cargo regeneration/checking or postcondition validation fails.
12. Stages only the enumerated workspace manifests, `Cargo.lock`, and `registry/agent.json` for the candidate commit; any other changed path still refuses the commit.

## Files

Changed by this repair:

- `crates/vesper-harness/src/release_executor.rs`
- `crates/vesper-harness/AGENTS.md`
- `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md`
- `docs/AGENTS.md`
- `docs/foundation/AGENTS.md`
- `docs/foundation/evidence-index.md`
- `docs/foundation/2026-09-30-rrc-complete-version-mutation-repair.md`

Other dirty-worktree files belong to the already-active RRC autonomy/source-resolution work and were preserved rather than reset or released.

## Exact evidence

### Version inventory

The direct inventory found:

```text
pin_count 118
manifest_count 27
```

Those 118 exact internal path pins span the two application manifests, 24 crate manifests with internal pins, and `xtask/Cargo.toml`. The typed repository regression rebuilds the mutation plan from the live `[workspace] members` list, ensuring every current version-bearing internal manifest and the generated lockfile route remain represented.

### Regression evidence

`cargo test -p vesper-harness`:

```text
test result: ok. 162 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out
```

New version-mutation regressions passed inside that suite:

```text
release_executor::tests::version_plan_updates_every_internal_dependency_section_and_not_external_pins ... ok
release_executor::tests::version_plan_rejects_one_mismatched_internal_pin_before_writes ... ok
release_executor::tests::version_plan_write_failure_rolls_back_every_written_file ... ok
release_executor::tests::repository_version_plan_covers_every_owned_manifest_and_lockfile ... ok
release_executor::tests::registry_bump_rejects_stale_archive_version ... ok
release_executor::tests::registry_bump_updates_version_and_archive_urls ... ok
```

The section-coverage fixture includes normal, dev, build, and target-specific exact internal path requirements and proves an equal-version external requirement remains unchanged. The mismatch fixture proves planning fails before any write. The injected write-failure fixture fails after one replacement and proves every file equals its original bytes afterward.

### Static and controller gates

`cargo clippy -p vesper-harness --all-targets --all-features -- -D warnings` completed successfully:

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 14.80s
```

`cargo xtask acceptance`:

```text
Acceptance regression gate: 30 exact cases passed in 29466 ms. Offline fixture model cost: zero; live-model effectiveness is not measured.
```

`cargo xtask architecture`:

```text
architecture boundaries validated for 31 packages
```

The required fast consistency command completed successfully on the current workspace graph:

```text
cargo check --workspace --all-targets
Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.30s
```

A locked metadata read reported 31 owned packages and one common version, `0.24.4`. `cargo fmt --all -- --check` and `git diff --check` both completed with no reported errors.

## Invariants re-derived

- A stale internal exact pin is detected while the plan is still read-only.
- A stale Registry version or archive URL is detected while the plan is still read-only.
- External dependency requirements are never rewritten merely because their number matches the workspace version.
- A replacement failure rolls back prior replacements.
- A Cargo check or locked-metadata postcondition failure restores manifests, Registry metadata, and the original lockfile.
- Candidate commit staging is derived from the workspace member inventory and rejects non-version paths.
- Commit, tag, push, and publication remain downstream of successful version preparation and therefore cannot run against a known mismatched workspace graph.

## Deviations

- The read-only delegated audit could not start because its configured OpenAI model was unavailable in the current account model list. Direct repository inspection supplied the inventory and implementation evidence instead.
- No production release, tag, push, publication, Registry submission, installer, or local installation was run. This was an implementation/verification repair, not release authorization.
- Cross-platform CI was not started. The changed scanner and metadata validation use platform-neutral `Path`/canonicalization APIs, but this report claims local Linux verification only.

## Unresolved items

- A real public release remains intentionally unexecuted; therefore this work does not claim live publication acceptance.
- Fresh Windows and macOS execution of the new version-mutation path is not recorded in this work unit.

## Readiness effect

The reported partial-version defect is closed in the RRC executor: version preparation now fails before release gates on inconsistent source metadata, rolls back partial replacement/check failures, regenerates the lockfile only after complete metadata mutation, and validates the resulting workspace graph before candidate commit. Local RRC, acceptance, architecture, formatting, and clippy gates are green. Release publication and fresh cross-platform acceptance remain separate and unexecuted.
