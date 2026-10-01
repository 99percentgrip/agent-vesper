# RRC Source-Resolution UX Repair — Execution Report

## Objective

Replace repository-descendant-based release-source ambiguity with completed-objective provenance. A generic natural-language release imperative must select the clean candidate bound to the current completed objective, ignore unrelated historical worktrees such as `red-main-ci`, preserve a dirty primary checkout, and begin the existing RRC automatically. Clarification is permitted only for genuinely different trees bound to the same objective and must use human variant labels instead of paths or SHAs. Do not publish.

## Methods and commands

Implementation worktree:

```text
/home/Alex/Projects/agent-vesper/.worktrees/rrc-autonomy-repair
```

Starting HEAD:

```text
01d2df045a7c30253bea8e613e9840b9f62d053b
```

Regression-first receipt: after adding the dirty-primary/current-objective/unrelated-historical-worktree scenario but before changing resolution, the focused suite failed:

```text
test release_recovery::tests::natural_release_from_dirty_primary_selects_completed_source_without_mutation ... FAILED
assertion failed: matches!(outcome, NaturalReleaseAdmission::Started(_))
```

Verification commands:

```text
cargo fmt --all
cargo test -p vesper-harness natural_release -- --nocapture
cargo test -p vesper-harness ambiguous_completed_candidates_clarify_once_without_mutation -- --nocapture
cargo test -p vesper-harness --lib
cargo test -p agent-vesper-acp --lib
cargo test -p agent-vesper-tui --lib
cargo xtask acceptance
cargo fmt --all -- --check
git diff --check
```

A clean, local-only completion worktree and commit were created from the exact repair snapshot so Alex's prerelease binary has a real clean objective-bound source candidate:

```text
Worktree: /tmp/agent-vesper-rrc-source-resolution-completion-20260930T010629Z
Branch:   rrc-source-resolution-completion-20260930T010629Z
Commit:   3f8ea52e7ee663498b401848444b689d07f09511
Status:   clean
```

Fresh build command:

```text
CARGO_TARGET_DIR=/tmp/agent-vesper-rrc-source-resolution-build-20260930T010648Z \
CARGO_INCREMENTAL=0 \
  cargo build --release -p agent-vesper-tui \
  --manifest-path /tmp/agent-vesper-rrc-source-resolution-completion-20260930T010629Z/Cargo.toml
```

## Files changed

- `crates/vesper-harness/src/release_recovery.rs`
  - Loads committed `docs/foundation/release-objective-provenance.json` records from candidate HEADs.
  - Validates bounded objective/variant labels, RFC 3339 completion time, and committed evidence-report paths.
  - Resolves the newest completed objective before comparing candidate trees.
  - Excludes clean descendants without that current objective binding.
  - Clarifies only when the same objective retains multiple maximal tree identities.
  - Keeps paths and SHAs out of ordinary clarification text.
  - Persists objective identity, labels and evidence reports with existing source/base/commit/diff/workspace provenance.
  - Exposes raw source/release workspaces, SHAs and diff identity through `/release evidence`.
  - Adds regression coverage for the exact dirty-primary + relevant completion + unrelated historical worktree scenario and same-objective human-labelled ambiguity.
- `docs/foundation/release-objective-provenance.json`
  - Versioned current completed-objective binding used by production RRC resolution.
- `crates/vesper-harness/AGENTS.md`
- `apps/agent-vesper-tui/AGENTS.md`
- `apps/agent-vesper-acp/AGENTS.md`
- `docs/AGENTS.md`
- `docs/foundation/AGENTS.md`
  - Updated durable contracts and documentation ownership.
- `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md`
- `docs/foundation/evidence-index.md`
- This report.

The earlier natural-language TUI/ACP admission code remains shared and unchanged by this follow-up.

## Exact evidence

### Objective-linked source-selection regression

```text
test release_recovery::tests::natural_release_intent_is_imperative_and_fail_closed ... ok
test release_recovery::tests::natural_release_from_dirty_primary_selects_completed_source_without_mutation ... ok

test result: ok. 2 passed; 0 failed
```

The production-path fixture includes:

- a dirty primary checkout whose bytes are asserted unchanged;
- a clean `RRC autonomy repair` candidate with committed objective provenance;
- a divergent clean `Historical red main CI repair` candidate with older, different objective provenance;
- only the natural-language input `Release all completed RRC work as the next patch.`;
- automatic worker-launch observation;
- exact assertion that the selected workspace is the RRC completion candidate and not the historical candidate;
- persisted objective, base, intended commits/diff, release workspace and final candidate identity.

### Same-objective ambiguity UX

```text
test release_recovery::tests::ambiguous_completed_candidates_clarify_once_without_mutation ... ok
```

The assertion requires both human labels:

```text
Variant completed after CI recovery
Earlier variant before CI recovery
```

and rejects candidate directory names and the temporary root from the user-facing message. It also proves no controller state or worker is created and dirty primary bytes remain unchanged.

### Full suites

```text
vesper-harness: 158 passed; 0 failed; 2 ignored
agent-vesper-acp: 61 passed; 0 failed
agent-vesper-tui: 267 passed; 0 failed
```

The two ignored harness cases are existing explicit environment-dependent probes, unrelated to RRC.

### Completion-assurance gate

```text
Acceptance regression gate: 30 exact cases passed in 44637 ms. Offline fixture model cost: zero; live-model effectiveness is not measured.
```

### Fresh prerelease TUI

```text
Artifact: /home/Alex/Projects/agent-vesper-prerelease-candidates/rrc-source-resolution-20260930T010648Z-3f8ea52e7ee6/agent-vesper-tui
Source:   3f8ea52e7ee663498b401848444b689d07f09511
SHA-256: 159a269bdc7d97d8569630097e327e5394089d4a76e351b8fe6cb84d353c7b97
Version:  agent-vesper-tui 0.24.4
File:     ELF 64-bit LSB pie executable, x86-64, dynamically linked, stripped
```

The completion source worktree remained clean after the build.

## Constraints held

- No release or publication was started.
- No version was changed.
- No tag, push, registry update, installation or credential mutation occurred.
- The primary checkout was not reset, cleaned, stashed, committed, or overwritten.
- The prerelease binary was copied to an external candidate directory and was not installed.
- Existing RRC mutation tokens, local gates, remote matrices, retry/repair policy and publishing boundary remain authoritative after source admission.
- VRO-19 was not touched.

## Deviations

- The live acceptance itself was not replayed by automation because doing so would start the real release controller. Alex retains the requested human acceptance step with the fresh prerelease binary.
- The clean completion commit is local-only and intentionally not pushed.
- Fresh five-platform CI was not launched; this report makes no new cross-platform claim.

## Unresolved items

- Alex's manual natural-language acceptance is not yet run. The intended input is exactly: `Release all completed work as the next patch.`
- Publication remains unauthorized even if manual acceptance passes.

## Readiness effect

The prior repository-topology ambiguity is removed from ordinary UX. RRC now resolves the current completed objective first, ignores unrelated historical clean descendants, selects the objective-bound clean implementation automatically, creates its isolated release workspace, and starts local verification. Only unresolved variants of that same objective can ask one question, and the question uses product-language labels rather than Git internals. A fresh non-installed prerelease TUI is ready for Alex's test.
