# Skill routing language follow-up

Date: 2026-09-13. Status: lexical implementation verified; quality HOLD; semantic experiment ongoing.
PRD: [Skill routing quality](../skill-routing-quality-prd.md).
Baseline: `1a8ac656cac500c862e2cddb3d459a699a4da6ac` in
`/tmp/vesper-routing-quality`, branch `feat/skill-routing-quality`.

## Objective

Continue toward reliable natural-task selection after the initial quality HOLD,
without modifying the skill library or GLM score-floor work. This report supplements
rather than replaces the [original failed measurements](skill-routing-quality-implementation.md).

## Findings and implementation

- Exact-token retrieval missed ordinary singular/plural forms and common task
  vocabulary. Requiring two matches rejected strong one-term evidence such as a
  requested spreadsheet. Conversational words inflated the overlap denominator.
- `routing_quality.rs` now uses English morphology from workspace-pinned
  `rust-stemmers`, normalizes a bounded general vocabulary (file-format phrases,
  workbook/spreadsheet, debugging and planning terms), and uses writing context for
  natural/human wording. There is no skill-ID lookup table, body indexing, model
  download, provider call or library rewrite. Unknown vocabulary remains unknown.
- Metadata anchors exclude generic action words. A single anchor can activate only
  in a task-request form; ordinary questions do not acquire task intent from a
  topic word alone. Scores remain relevance measures, not confidence probabilities.
- Direct descriptor validation replaces repeated one-entry index construction.
  The live catalog still validates and invalidates the cache before search.
  Standard scoring, policy gates, loading budgets and `rank_chunks` are untouched.
- `Cargo.toml`/`Cargo.lock` add the already workspace-pinned stemmer dependency to
  memory. `routing_quality_development.rs` measures development cases separately;
  `routing_quality.rs` adds arbitrary-ID normalization and conversational-topic tests.

## Methods and evidence

All commands run in the isolated checkout with
`CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0`.
No live provider or installed-user-state writes occur.

```sh
cargo test -p vesper-memory --test routing_quality_development -- --nocapture
cargo test -p vesper-memory --test routing_quality --test routing_quality_store --test routing_literal_requests
cargo test -p vesper-memory --test routing_quality_perf -- --nocapture --test-threads=1
cargo clippy -p vesper-memory --all-targets --all-features --offline -- -D warnings
cargo +1.88.0 check -p vesper-memory --all-targets --all-features --locked --offline
VESPER_ROUTING_EVAL_OUTPUT=/tmp/routing-quality-language-verified.json cargo xtask verify
```

Development-only diagnostic on the actual catalog:

| Stage | Positive hits | False abstention | No-skill activation |
| --- | --- | --- | --- |
| Initial preview | 21/40 | 17/40 | 0/20 |
| Morphology + anchor experiment | 33/40 | 5/40 | 1/20 |
| Contextual vocabulary + task-request distinction | 40/40 | 0/40 | 0/20 |

The intermediate regression was a weekly-planning skill activated by a general
question about days in a week. The task-request distinction repairs it without
removing the negative label. Parameters were frozen before running the follow-up
full corpus. The original corpus has been inspected in earlier work: its follow-up
results are regression evidence, not a newly unseen independent benchmark.
A new test fixture initially declared a read-only descriptor for a mutating editing
skill; correcting its effect to Workspace preserved the authoritative-effect gate.

Focused tests and changed-crate Clippy/Rust 1.88 checks pass. Initial 500-entry
end-to-end warm p95 rose to 67.095 ms and failed the budget. Removing redundant
per-entry index builds reduced it to 32.730 ms (maximum 33.908 ms); cold store
64.961 ms, pure-index cold 46.937 ms and warm p95 2.576 ms. Same Linux CPU as the
initial report, ten warm-ups and 100 timed queries. Full workspace verification passed: 2,282 tests passed, 0 failed and 35 ignored,
plus all 23 acceptance cases. Ignored external/runtime gates remain unexecuted. Logs: `/tmp/routing-development-before.log`,
`/tmp/routing-development-morphology.log`, `/tmp/routing-development-language.log`,
`/tmp/routing-language-tests.log`, `/tmp/routing-language-perf-cached.log`,
`/tmp/routing-language-msrv.log`, `/tmp/routing-language-clippy.log`, and
`/tmp/routing-language-verify.log`.

## Acceptance, unresolved items and readiness

Full workspace verification passed, including all 23 acceptance cases. The
[unchanged-corpus receipt](skill-routing-language-results.json) records 36/40
positive Recall@3 (90%) at every size, 3/40 abstentions, zero no-skill activations,
and zero harmful-sibling exposures. The no-name subset reaches 27/31 (87.1%),
with 3/31 abstentions. Misses remain q006, q032, q046 and q072. The validation
optimization produced identical predictions to the prior build. This still fails
the 95% and 5% positive gates; the selector remains on HOLD. Do not
interpret development accuracy as the PRD's 95% held-out acceptance. The original
score-floor checkout remains at PR-1 (`5cf5835`); integrated R6 awaits GLM's work.
Native new submitted prompts reroute through the shared selector; in-flight
steering is still ordinary guidance and does not invoke a second skill search.
No default promotion, release, push or local installation has occurred.

## DOX and preservation

The memory contract documents normalization and cached validation; foundation
ownership, evidence index and PRD link this report. Root, app and library contracts
remain unchanged because this change stays in the shared selector and adds no host
workflow or source-library mutation. All 488 library files match their pre-work hashes; the five protected GLM files
and `rank_chunks` remain byte-identical to its PR-1 baseline. `git diff --check`
and the naming guard pass. No original-checkout writes occurred.
