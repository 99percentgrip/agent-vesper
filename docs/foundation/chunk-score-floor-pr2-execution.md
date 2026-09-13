# Chunk score-floor PR-2 execution: the conjunction gate

Date: 2026-09-13. Status: **PR-2 complete**.
Owner PRD: [Chunk-tier score floor](../chunk-score-floor-prd.md).

## Objective

Eliminate the chunk-tier cosine noise floor by implementing the PRD's D1
conjunction gate — a chunk routes only when it carries literal signal
(`overlap >= 1` **or** a name match) **and** `score >= 520` — strictly
inside `rank_chunks`, with the skill tier untouched, and flip the PR-1
noise-floor anchor from `#[ignore]`d-red to green.

## Constraints held (directive fidelity)

- **Exact invariant (D1):** implemented literally as
  `literal_signal && score >= MIN_CHUNK_ROUTING_SCORE` where
  `literal_signal = overlap >= 1 || name_match`. Bare-floor alternative
  rejected in the PRD (§1.1) was not reintroduced.
- **Scope containment:** one admission filter inside `rank_chunks`
  (`skill_orchestrator.rs`), plus the exported constant
  `MIN_CHUNK_ROUTING_SCORE: i32 = 520`. `AUTO_ACTIVATION_SCORE`,
  `MAX_SELECTED_SKILLS`, budgets, and all skill-tier scoring are
  byte-identical (`git diff` scope receipt below).
- **Naming embargo:** the only new brand hit came from the *prior unit's*
  uncommitted `docs/skill-routing-quality-prd.md:60` (brand word + vendor
  URL in a citations table); neutralized to a no-URL neutral reference
  per the alias rule. `cargo xtask naming-guard`: **clean — 18 hits, all
  frozen** (zero baseline change).
- **Recon-before-edit (explicit user constraint):** the working tree was
  dirty with the prior unit's dollar-skill routing repair (explicit
  selection parser `explicit_skill_from_prompt`/`dollar_skill_from_prompt`
  + 2 tests + docs). Verified textually disjoint from `rank_chunks`
  before any edit; the repair is preserved unmodified and committed in
  its own commit ahead of this one.

## Recon receipts

- `git status --short` (pre-edit): 7 modified + 3 untracked paths, all
  belonging to the dollar-repair and routing-quality-PRD units.
- `git diff crates/vesper-memory/src/skill_orchestrator.rs` (pre-edit):
  hunk at `:427` (explicit selection) and `:1254` (dollar parser) — zero
  overlap with the chunk tier at `:863`.
- Baseline table re-measured in a detached worktree at `5cf5835`
  (pre-dirty-tree) to isolate PR-2's effect from the dollar repair.

## Changes

| File | Change |
|---|---|
| `crates/vesper-memory/src/skill_orchestrator.rs` | + `MIN_CHUNK_ROUTING_SCORE` exported constant; `RankedChunk.literal_signal` field; admission filter `score > 0` → `literal_signal && score >= MIN_CHUNK_ROUTING_SCORE`; gate doc-comment |
| `crates/vesper-memory/tests/chunk_routing_eval.rs` | anchor `#[ignore]` removed (now green); stale `score > 0` provenance comment corrected to the gate; assertion text unchanged |
| `docs/skill-routing-quality-prd.md` | embargo fix on the prior unit's file (line 60 citations row) |
| `docs/chunk-score-floor-prd.md` | status → PR-2 landed; AC-3 letter correction; status-history entry |
| `crates/vesper-memory/AGENTS.md` | contract bullet for the conjunction gate |

## Methods and commands

```
cargo test -p vesper-memory --test chunk_routing_eval          # anchor + control
cargo test -p vesper-memory                                     # full crate
cargo test -p vesper-memory --test chunk_routing_eval full_metrics -- --nocapture
cargo test --workspace --all-features                           # floor
cargo xtask acceptance
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo xtask naming-guard
cargo fmt --check
```

## Evidence (verbatim receipts)

- **Anchor (CRITICAL CONSTRAINT 3):** `noise_floor_pin_...` — the pre-PR-2 receipt in the PR-1 report records the unhardened failure. Today:
  `test noise_floor_pin_zero_overlap_prompt_routes_nothing ... ok` (0 ignored in its binary: `test result: ok. 9 passed; 0 failed; 0 ignored`).
- **Control (directive §2):** `test noise_floor_control_genuine_overlap_still_routes ... ok`.
- **Crate:** 82/82 green (`chunk_routing_eval` 9, `chunk_store` 12,
  `exemplar_routing` 4, `skill_routing` 14 including the dollar-repair
  tests, `chunk_store`-unit 43).
- **D3 canonical table (AC-3):** all 12 success flags and probe ladders
  identical to the recorded table. Three rows differ exactly by the
  chunks the gate removes — see §Stop rule.
- **Exemplar 16/16:** `routing_proofs_target_chunk_ranks_first_for_its_phase ... ok`,
  `budget_proof_worst_case_activation_inside_per_skill_cap ... ok`,
  `manifest_proof_sixteen_valid_entries_all_files_present_under_cap ... ok`,
  `g5_proof_no_chunk_vocabulary_routes_lean_body_only ... ok` (bounded
  form; literal restoration is PR-3, intentionally out of scope).
- **Workspace floor:** `WORKSPACE TOTAL passed=2256 failed=0`
  (138 result lines, all `0 failed`). Reconciles: 2,253 (PR-1 floor) +
  2 (dollar-repair tests, prior unit) + 1 (anchor unignored) = 2,256.
- **Acceptance:** `Acceptance regression gate: 23 exact cases passed in 22576 ms.`
- **Clippy:** `Finished dev profile … in 3.99s` (no warnings, `-D warnings`).
- **Naming-guard:** `naming-guard: clean (18 hits, all frozen in baseline)`.
- **fmt:** `cargo fmt --check` silent.

## Stop rule (PRD §5 PR-3): fired and resolved

The D3 table changed in three rows. The PRD's stop rule
("if any control or previously-resolving probe regresses, stop and
re-derive") was applied immediately:

- Baseline (worktree `5cf5835`, unhardened filter):
  `procedure-application | SummaryOnly | false | irrelevant_before=1 | routed=1`
  and `SummaryKeyElements | true | 0 | routed=2`; factual-retrieval
  SummaryOnly metadata probe `routed=2`.
- After gate: `routed=1` on both, `irrelevant_before=1 → 0`.
- **No success flag changed anywhere; no probe ladder changed.**
- Re-derivation (runtime probe in the disposable worktree, unhardened
  filter, prompt `"use skill cutover-runbook: canary rehearsal"`):
  `SummaryOnly -> routed ["certificates"]` and
  `SummaryKeyElements -> routed ["migrate", "certificates"]`.
  `certificates` has **zero overlap** with the probe (canary/rehearsal
  absent from its description and summary) and no name match — it was
  admitted on pure cosine (+0.6547-class hash collision), i.e. exactly
  the defect class this PRD eliminates. Its loss is the intended effect;
  the row was itself a noise-floor instance, not the "honest ranking
  noise" PRD §3 assumed. AC-3's letter ("`irrelevant_before` rows
  preserved") is corrected in the PRD; the AC's substance — no honest
  routing regressed — holds with runtime proof. Worktree and probe
  deleted; no source residue.

## Deviations

- The changed `routed` counts (2→1) are measured, intended effects of the
  gate on zero-overlap candidates, documented as an AC-3 letter
  correction rather than treated as regression — with the §Stop-rule
  runtime proof distinguishing noise from signal.
- `docs/skill-routing-quality-prd.md` line 60 belongs to the prior unit;
  committed with the embargo fix because this unit commits the whole
  tree and the guard must pass on the committed state.

## Unresolved items

- PR-3 (G5 literal restoration + re-eval) remains open per the PRD plan;
  `g5_proof_...` still asserts the bounded form with its finding comment.
- PRD Q1/Q2 remain defaulted (no change).

## Readiness effect

Automatic chunk routing no longer admits zero-signal candidates on
hash-cosine collisions — the noise floor is eliminated structurally, not
by threshold tuning. The exemplar migration's recorded finding (§5.1) is
now addressable by PR-3. Skill-tier behavior, budgets, and the dollar
routing repair are byte-identical; the floor, acceptance, clippy, and
naming gates all hold.
