# RRC provenance ambiguity repair and corrected v0.24.4 candidate

**Date:** 2026-10-01

## Objective

Remove the final manual-selection ambiguity from RRC release-source resolution while preserving all completed product, RRC, Host Resource Governor, OpenAI, red-CI, regression and documentation work in the corrected prerelease integration. Retain historical worktrees, keep all release-owned version state exactly `0.24.4`, exclude the rejected `3.3.1` version commit from ancestry, run the complete local gates under the Host Resource Governor, and preserve a freshly compiled release-profile TUI for Alex's later natural-language RRC acceptance.

This work did not authorize or perform a release. RRC still owns the future `0.24.4 -> 0.24.5` transition after Alex enters `Release all completed work as the next patch.`

## Methods and commands

The repair was made only in the detached corrected worktree:

```text
/home/Alex/Projects/agent-vesper/.worktrees/release-prep-v0.24.4-corrected-2026-10-01
```

Representative commands, with every Cargo command launched through the Host Resource Governor:

```sh
cargo test -p vesper-harness --lib <focused provenance regression>
cargo clippy -p vesper-harness --lib --tests --all-features -- -D warnings
cargo xtask verify
cargo xtask acceptance
cargo metadata --locked --no-deps --format-version 1
cargo build --locked --release --package agent-vesper-tui \
  --features docker,swarm,bridge,voice-kokoro,voice-flm
```

A temporary detached probe worktree compiled an operator-only test around the production-private `resolve_release_source` function. The probe inspected the real 23-worktree inventory, called only the resolver, asserted the selected commit/workspace, and was then removed. It did not admit a release, create controller state, mutate the dirty primary checkout or remove any retained historical worktree.

## Files changed

Production and local contract:

- `crates/vesper-harness/src/release_recovery.rs`
  - accepts bounded optional `variant_id`, `canonical_release_source` and `supersedes` metadata;
  - filters to an explicitly canonical final integration when present;
  - permits explicit ancestor supersession;
  - collapses equal Git trees;
  - collapses equal implementation diffs after excluding only bounded provenance bookkeeping paths;
  - preserves ambiguity for genuinely different implementations;
  - disambiguates duplicate human labels with bounded commit subjects instead of paths or SHAs;
  - adds regression fixtures for all four behaviors.
- `crates/vesper-harness/AGENTS.md` records the resolver contract.
- `docs/foundation/release-objective-provenance.json` binds the corrected integration as the canonical `0.24.4` source and supersedes the historical source-resolution objective.
- `docs/foundation/AGENTS.md`, `docs/foundation/evidence-index.md` and `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md` link and own this evidence.
- `docs/foundation/2026-10-01-corrected-v0.24.4-release-preparation.md` is marked historical for manual acceptance because the provenance-fixed candidate replaces it.

No Cargo manifest, `Cargo.lock`, Registry manifest, release URL, provider implementation, VRO-19 file or installation path was changed.

## Exact evidence

### Regression-first proof

The four new tests were run against the pre-repair resolver body while retaining the new test body. All four failed as required:

```text
EXPECTED_FAILURE=same_tree_under_two_provenance_records_collapses_to_one_candidate
EXPECTED_FAILURE=equivalent_implementation_diffs_collapse_despite_distinct_provenance
EXPECTED_FAILURE=canonical_final_integration_outranks_historical_autonomy_variants
EXPECTED_FAILURE=genuinely_different_duplicate_labels_explain_the_difference
```

The exact repaired file was restored byte-for-byte after that controlled red run. The repaired focused suite then passed all four tests; the existing ambiguity and dirty-primary source-selection regressions also passed. Red/green logs and governor receipts are under:

```text
/home/Alex/Projects/agent-vesper-release-prep-evidence/20261001-provenance-ambiguity/
```

### Complete governed verification

The first `cargo xtask verify` attempt was admitted under normal resource pressure but failed strict Clippy because three test helpers exceeded the seven-argument lint:

```text
error: this function has too many arguments (8/7)
error: this function has too many arguments (8/7)
error: this function has too many arguments (9/7)
xtask failed: cargo exited with exit status: 101
```

The helpers were refactored to typed option/variant structs. Focused strict Clippy and all four repaired regressions passed before the complete gate was retried.

The final `cargo xtask verify` exited zero and ended with:

```text
Acceptance regression gate: 44 exact cases passed in 50876 ms. Offline fixture model cost: zero; live-model effectiveness is not measured.
```

The separately required `cargo xtask acceptance` exited zero:

```text
Acceptance regression gate: 44 exact cases passed in 22055 ms. Offline fixture model cost: zero; live-model effectiveness is not measured.
```

Final gate resource receipts:

| Command | Minimum available RAM | Peak owned-tree RSS | Maximum swap used | Minimum disk available | Exit |
|---|---:|---:|---:|---:|---:|
| `cargo xtask verify` retry | 19,352,154,112 B | 402,231,296 B | 1,424,928,768 B | 299,507,482,624 B | 0 |
| `cargo xtask acceptance` | 20,275,548,160 B | 121,860,096 B | 1,424,875,520 B | 299,170,140,160 B | 0 |
| real-inventory resolver probe | 19,591,581,696 B | 110,673,920 B | 1,424,867,328 B | 299,047,346,176 B | 0 |
| fresh release TUI build | 19,082,280,960 B | 121,274,368 B | 1,424,867,328 B | 298,366,619,648 B | 0 |

Every receipt reports `pressure_events=["normal"]`, `stopped_for_critical_pressure=false`, `CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=2`, and a governor-managed `CARGO_TARGET_DIR`.

### Real retained-worktree resolution

With all historical worktrees retained, the production resolver selected the committed corrected candidate without clarification:

```text
RETAINED_WORKTREE_COUNT=23
ACTUAL_CANONICAL_SOURCE=416cda721b4545b1c75da7ced80e154afb174c68
ACTUAL_SOURCE_WORKSPACE=/home/Alex/Projects/agent-vesper/.worktrees/release-prep-v0.24.4-corrected-2026-10-01
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 189 filtered out
```

The probe was read-only and was removed after settlement. The corrected source worktree remained clean.

### Version and ancestry boundary

Governed locked Cargo metadata plus direct manifest/ancestry checks recorded:

```text
WORKSPACE_VERSION=0.24.4
WORKSPACE_PACKAGE_COUNT=31
WORKSPACE_PACKAGE_VERSIONS=0.24.4
INTERNAL_EXACT_0_24_4_PIN_COUNT=118
REGISTRY_VERSION=0.24.4
REJECTED_VERSION_COMMIT_IS_ANCESTOR=no
RELEASE_OWNED_PATH_3_3_1_MATCH_COUNT=0
V3_3_1_TAG_COUNT=0
V0_24_5_TAG_COUNT=0
```

Historical evidence documents still mention the rejected string `3.3.1`; the bounded release-owned path audit is zero and no version state uses it.

### Fresh corrected candidate

The build used a new empty governor state/target directory after both complete gates passed.

```text
BUILD_SOURCE_SHA=416cda721b4545b1c75da7ced80e154afb174c68
TUI_PATH=/home/Alex/Projects/agent-vesper-prerelease-candidates/v0.24.4-corrected-provenance-fixed-linux-x86_64-20261001T072311Z/agent-vesper-tui
TUI_VERSION=agent-vesper-tui 0.24.4
TUI_SHA256=b75a52717323a6662ff759956d6338293b60c34e40476c01014517ee2cb3d6dd
FILE=ELF 64-bit LSB pie executable, x86-64, dynamically linked, stripped
BUILD_RESULT=Finished release profile in 5m 19s
```

The source status hash was the empty-inventory digest both before and after compilation:

```text
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
```

The candidate directory contains the binary, source identity/lineage, version and file receipts, canonical marker, complete build log, governor receipt, manifest and verified checksums. It was copied for manual use only and was not installed.

## Constraints held

- No release, push, tag, publication, Registry update or installer ran.
- No `0.24.5` or `3.3.1` version mutation was made.
- The rejected commit `1ea840fb0afccbc369e2a5059cc582f94cf78a02` is not in corrected ancestry.
- VRO-19 was not touched.
- Historical worktrees were retained.
- The dirty primary checkout was not reset, cleaned, stashed, committed or used as a mutation workspace.
- No live provider call or user-state write was used for verification.

## Deviations

1. The first complete verify attempt failed the strict argument-count lint in new test helpers. The failure receipt is preserved. Typed helper structs corrected it, focused Clippy passed, and the entire verify gate then passed.
2. The fresh binary's build-source SHA precedes this documentation-only closeout. This report, index/PRD links and its addition to the provenance marker do not alter executable code or candidate bytes.
3. Linux-local gates and this Linux x86-64 candidate do not substitute for RRC's future exact-SHA hosted five-target release matrices.

## Unresolved items

- Alex has not run the natural-language release acceptance from this candidate.
- The `0.24.4 -> 0.24.5` mutation is intentionally unexecuted.
- Push, hosted exact-SHA matrices, tag, GitHub Release, assets/checksums, Registry update and publication remain RRC-owned future work after authorization.
- No live-provider acceptance was requested or run.

## Readiness effect

The provenance ambiguity is repaired and regression-protected. Equivalent historical candidates no longer force manual selection, genuinely different implementations still fail closed with useful human distinctions, and the actual retained worktree inventory resolves to the canonical corrected source. The replacement TUI is ready for Alex's manual RRC test and truthfully reports `0.24.4`. This is prerelease preparation only, not release authorization or completion.
