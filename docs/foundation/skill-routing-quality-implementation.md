# Skill routing quality implementation ledger

Date: 2026-09-13. Status: **in progress; not integrated or accepted**.
PRD: [Skill routing quality](../skill-routing-quality-prd.md).
Checkout: `/tmp/vesper-routing-quality`, branch `feat/skill-routing-quality`.

## Objective and coexistence

Implement task-aware routing while preserving the complete library and GLM's
parallel score-floor work. [Recon](skill-routing-coexistence-recon.md) records
its exact PR-1 baseline, unchanged chunk implementation and required integration.
GLM has finished PR-1 only; PR-2/PR-3 are still pending. No other agent is assumed
to have finished or been instructed by this implementation.

## Work performed

- Preserved the prior dollar repair/planning artifacts in local commit `3f80163`.
- Added `crates/vesper-memory/src/routing_quality.rs`: descriptor bounds and
  revision checks, duplicate/cap validation, metadata-only BM25 retrieval,
  twelve-candidate cap and typed artifact/effect/resource comparisons. This is
  an independent component, not yet connected to live orchestration or Settings.
- Added `tests/routing_quality.rs`: four structural tests for sibling effects,
  order invariance, zero-overlap abstention, stale/oversized/conflicting
  descriptors and resource/artifact constraints.
- Added `tests/routing_quality_cases.json`: 240 authored labels, 13 positive-task
  families, 120 development / 120 held-out, paired cases grouped within splits,
  >=60 positive prompts without literal skill slugs. Categories: 80 positive,
  40 no-skill, 40 sibling, 40 syntax, 40 resource cases. Sibling/resource records
  are explicitly synthetic execution-contract fixtures, not invented production
  skill capability claims. Labels were authored before inspecting predictions.
- Added `tests/routing_quality_corpus.rs`: corpus integrity checks and diagnostic
  Standard-mode predictions against the real catalog for the 160 positive,
  no-skill and syntax cases. Synthetic cases and distractor matrices remain to
  be wired; this is not the complete PRD evaluation.
- Added the module export and nearest memory DOX ownership. Library sources are
  untouched. No new dependency, provider call or installed-state write.

## Evidence

- `cargo test -p vesper-memory --test routing_quality --offline`: 4 passed / 0 failed.
- `cargo test -p vesper-memory --test routing_quality_corpus --offline -- --nocapture`:
  2 integrity/execution tests passed. These tests do not assert the quality gate.
- Diagnostic real-catalog Standard result: acceptable skill in top three for
  34/80 positive queries; false activation on 3/40 no-skill prompts; explicit
  errors on 2/20 literal syntax examples. These are aggregate diagnostic counts,
  not Enhanced results or passed accuracy acceptance. Full predictions:
  `/tmp/routing-quality-baseline.log`. Do not relabel failed cases to hide them.
- `cargo clippy -p vesper-memory --all-targets --offline -- -D warnings`: passed
  for the final component. Full `cargo test -p vesper-memory --offline`: 87 passed, 0 failed, 1 ignored (including GLM’s pending pin).
- GLM genuine-match control passed 1/0; the explicitly run ignored pin failed
  0/1 with the expected `bytecode-lexicon` false match. Both files and the
  `rank_chunks` function remain unchanged in our branch.
- All 488 skill-library file paths and SHA-256 values match the recorded baseline.
  Commands used Python hashlib comparisons, focused Cargo checks and
  `git diff --check`. No workspace-wide formatter ran in GLM's checkout.

## Remaining work and acceptance

R1 quoted-input handling, store/index lifecycle, live orchestrator selection,
Settings/ACP controls, trace/fallback behavior, synthetic sibling/resource
execution, distractor scaling, held-out adoption gates, and combined GLM R6
acceptance are pending. No claim of an intelligent live router or complete PRD
implementation is made. Current tests establish structural properties only.
Descriptors are supplied by the caller in the independent component; catalog
revision derivation and authoritative policy eligibility remain integration work.
Optional embeddings and body-aware retrieval remain outside mandatory delivery.

## DOX closeout for this checkpoint

Memory ownership updated for the new module; the PRD and evidence index link this
ledger and coexistence report. Existing parent contracts and child indexes remain
valid. Code and planning artifacts are retained in the isolated branch. The
original checkout, GLM's branch/index, installed binaries and library are unchanged
by this implementation checkpoint.
