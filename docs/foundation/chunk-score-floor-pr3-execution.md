# Chunk score-floor PR-3 execution: literal G5 restoration & initiative closeout

Date: 2026-09-13. Status: **PR-3 complete — score-floor initiative COMPLETE**.
Owner PRD: [Chunk-tier score floor](../chunk-score-floor-prd.md).

## Objective

Restore the exemplar G5 proof to its strict literal intent — a
vocabulary-free prompt routes exactly ZERO chunks — completing the
score-floor initiative closeout: full re-eval, docs, and final gates.

## Constraints held

- **Naming embargo:** no new brand stems; `cargo xtask naming-guard`
  clean (18 hits, all frozen) — zero baseline erosion.
- **Scope containment:** exactly one test function rewritten
  (`exemplar_routing.rs::g5_proof_no_chunk_vocabulary_routes_lean_body_only`)
  plus docs. Zero ranker-arithmetic lines touched (`git diff` receipt:
  only the G5 function changed in the test file).

## The stop rule fired — and was resolved, not masked

Restoring the literal assertion on the original prompt **failed on the
hardened tree** (`routed: ["phase4-analysis"]`). Investigation by exact
offline replication of the shipped tokenizer (split on
non-alphanumeric → stem → stop-word filter):

- The original prompt `"produce a research publication from finished
  trials on gating mixtures"` was **never vocabulary-free**:
  `finished` stems to `finish`, and phase4-analysis' description reads
  `"Load after experiments finish:"` — a genuine 1-token overlap → 520
  points → admitted at exactly the floor. The gate worked correctly.
- The bounded `<= 3` era never exposed this: phase4's honest admission
  hid inside the noise mass.
- Conclusion: fixture defect, not a G3 violation. No ranker change made
  (scope fence held).

### Fixture correction

Replacement prompt chosen by exhaustive offline replication against all
16 chunk pools (description + summary + key_elements):

> `"turn these outcomes into a research publication on gating mixtures"`

- **Zero overlap** with every chunk pool (verified token-by-token).
- **Still carries unhardened-ranker noise**: phase8-post-acceptance
  cosine +0.1826 → 401 pts, paper-types +0.0945 → 207 pts,
  phase4-analysis +0.0722 → 158 pts — all sub-520, none admittable
  without literal signal. This keeps the test **sharp**: it exercises
  the gate against real hash noise, not a trivially quiet prompt.
- Skill still auto-activates (`research` name-match at the skill tier) —
  the G5 contract (lean body only) holds.

## Non-vacuity receipts (audit discipline)

The restored test fails on the pre-PR-2 tree (`b13940e` worktree, both
prompts) and passes on the hardened tree:

**Original prompt, literal assertion, unhardened tree:**
```
ROUTED_CHUNKS: ["phase3-execution-monitoring", "phase4-analysis", "phase8-post-acceptance"]
G5 LITERAL: vocabulary-free prompt must route ZERO chunks, routed: [...]
test result: FAILED. 0 passed; 1 failed
```

**Corrected prompt, literal assertion, unhardened tree:**
```
G5 LITERAL: vocabulary-free prompt must route ZERO chunks, routed: ["phase8-post-acceptance", "paper-types", "phase4-analysis"]
test result: FAILED. 0 passed; 1 failed
```

The corrected prompt's unhardened routing matches the offline
replication digit-for-digit (phase8 401 > paper-types 207 > phase4 158).
Worktrees deleted after each receipt.

## Changes

| File | Change |
|---|---|
| `crates/vesper-memory/tests/exemplar_routing.rs` | G5 restored: `chunks.is_empty()` assertion; fixture-corrected prompt; comment records the `finish` overlap finding, the correction rationale, and non-vacuity lineage |
| `docs/chunk-score-floor-prd.md` | Status → COMPLETE; PR-3 status-history entry |
| `docs/migration-status.md` | Score-floor row → COMPLETE with receipts |
| `docs/AGENTS.md` | chunk-score-floor ownership: planning → completed contract |
| `docs/foundation/evidence-index.md` | PR-3 entry (below) |

## Final re-evaluation receipts

- Exemplar suite: 4/4 (`manifest_proof_…`, `g5_proof_…` literal,
  `budget_proof_…`, `routing_proofs_…`) — the 16/16 per-chunk routing
  proofs pass inside `routing_proofs_target_chunk_ranks_first_for_its_phase`.
- D3 eval ladder: 11/11 in `chunk_routing_eval`, canonical table
  identical to the PR-2 recorded state.
- Crate: 6/6 binaries green (43 + 11 + 12 + 4 + 14 + 0).
- **Workspace floor: 2,258 passed / 0 failed** (`--all-features`, all
  binaries).
- **Acceptance: 23/23** (`6419 ms`).
- Clippy `-D warnings`: clean. `cargo fmt --check`: clean.
- **naming-guard: clean (18 hits, all frozen)**.

## Initiative audit (all 6 ACs re-traced to current code)

| AC | Status | Current evidence |
|---|---|---|
| AC-1 anchor fails unhardened, passes after | ✅ | PR-1 receipt + anchor green in current tree |
| AC-2 G5 literal | ✅ | This PR; non-vacuity receipts above |
| AC-3 D3 ladder | ✅ | 11/11 green; table identical to PR-2 record; noise rows removed with runtime proof (PR-2 §Stop rule) |
| AC-4 exemplar 16/16 | ✅ | 4/4 suite green |
| AC-5 floor ≥2,252, acceptance 23/23, naming clean | ✅ | 2,258/0; 23/23; 18 frozen |
| AC-6 skill tier byte-identical | ✅ | `git diff 5cf5835~1..HEAD -- skill_orchestrator.rs` shows only the two chunk-tier additions (constant + filter) and Q1 matcher; no scoring line changed |

## Deviations

- The directive said "update the assertion to its strict literal intent…
  verify the test passes." It did not pass on the hardened tree until the
  fixture was corrected — the original prompt had a genuine token
  overlap (`finish`). Deviation is test-side fixture correction,
  documented above with receipts; no ranker change was made or needed.
- The directive's floor target was "2,256+"; the actual final floor is
  **2,258** (2,256 + 2 Q1 tests). Exceeds target.

## Unresolved items

- None for this PRD. (Adjacent, separate PRDs: `skill-routing-quality`
  remains proposed/not-started; exemplar G5's `routing map` reachability
  fallback is by design, not a defect.)

## Readiness effect

The chunk tier's admission logic is now fully signal-gated (cosine
cannot admit; embedded names cannot admit) and the flagship exemplar
suite asserts the literal noise-floor contract. Initiative closed with
every AC traced to current, scope-appropriate evidence.
