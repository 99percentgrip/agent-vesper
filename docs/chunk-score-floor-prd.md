# PRD: Chunk-Tier Score Floor

| | |
|---|---|
| **Status** | COMPLETE — 2026-09-13 (PR-1 anchor, PR-2 conjunction gate, Q1 delimiter-bounded names, PR-3 literal G5 restoration) + master audit (all ACs re-traced, pins sabotage-verified; `foundation/chunk-score-floor-master-audit.md`) |
| **Target crates** | `vesper-memory` |
| **Owner** | Alex (product); implementation via fast-track directives |
| **Related** | `advanced-context-paging-prd.md` (chunk tier), `ranker-hardening-prd.md` (raw pools), `docs/foundation/exemplar-migration-execution.md` §5.1 (the finding), `docs/architecture/recon_alias_crosstalk.md` §5.3 (noise floor documented as latent), `docs/architecture/recon_exemplar_candidate.md` |

## 1. Problem statement

The chunk tier's admission rule is **`score > 0`** (`skill_orchestrator.rs:896`).
Two independent terms can make that true with **zero literal token overlap**:

1. The cosine term (`:887`): `hashed_cosine` hashes tokens into a 64-dim
   signed vector (FNV-1a). Disjoint pools collide into a small *positive*
   cosine by chance.
2. The name-match term (`:891`): any prompt containing a chunk name
   substring routes to it — desirable (routing-map fallback), but it means
   "no chunk vocabulary" must be defined as *no overlap AND no name-match*.

**Measured (exemplar G5 probe, replicated exactly offline — stem, stop-words,
FNV-1a 64):** on a vocabulary-free prompt, `phase8-post-acceptance` scored
**678 points from cosine +0.3086 alone**, `paper-types` 207, `phase5` 166,
`phase6` 163. Result: up to `MAX_CHUNKS_PER_SELECTION` (3) chunks loaded on
pure hash noise — ~5.5 KB of context with no semantic justification. The
skill tier does not share this defect in the same way: its activation
threshold (`AUTO_ACTIVATION_SCORE = 2,200`, `:20`, applied at `:558`)
filters its noise; the chunk tier has no equivalent gate.

The noise floor is not hypothetical: it fired during the exemplar
migration's Stage D and forced the G5 test to be weakened to a bounded form
(`exemplar_routing.rs` `g5_proof_…`, finding comment at `:221`).

### 1.1 Why the directive's example floor (`score >= 520`) is insufficient — analysis

A bare score floor of 520 does **not** implement "at least one literal token
match." The observed noise outlier (678) passes it. Sensitivity (cosine
needed to clear the floor = floor / 2,200):

| floor | admits noise cosine up to | blocks observed outliers? |
|---|---|---|
| 520 | 0.2364 | **no** — 678-pt outlier passes |
| 700 | 0.3182 | yes (barely; margin 22 pts) |
| 1000 | 0.4545 | yes, but also risks blocking honest low-overlap chunks |
| 1200+ | ≥0.5455 | cosine term alone structurally cannot reach this without real overlap |

A floor tuned above the worst *observed* noise is empirical whack-a-mole:
some disjoint pair somewhere will collide higher. The robust invariant is
**structural**: cosine may only *rank* candidates that already have genuine
signal, never *admit* them. This PRD therefore specifies a **conjunction**
(overlap gate + score floor), not a bare threshold.

## 2. Goals

- **G1 — Eligibility requires literal signal.** A chunk is routable only
  when `overlap >= 1` literal token match (or an explicit chunk name-match).
- **G2 — Cosine ranks, never admits.** The cosine term continues to order
  candidates (D3-measured behavior preserved) but cannot make a
  zero-overlap candidate eligible.
- **G3 — Zero regression on measured routing.** The D3 eval ladder and the
  exemplar's 16/16 routing-first proofs hold unchanged (every currently
  passing case has literal overlap ≥1 by construction).
- **G4 — G5 restored to its literal form.** A vocabulary-free prompt
  (no overlap, no name-match) loads **zero chunks** — provable.
- **G5 — Scope fence.** Skill-tier arithmetic and `AUTO_ACTIVATION_SCORE`
  untouched.

## 3. Non-goals

- Changing the 64-dim hash, cosine, `SEMANTIC_ALIASES`, or any skill-tier
  scoring (`ranker-hardening-prd.md` scope fence applies here too).
- Raising `MAX_CHUNKS_PER_SELECTION` or budget arithmetic (audited F1 fix
  stays as-is).
- Purging `irrelevant_before` (a *ranked-below-target* irrelevant chunk with
  genuine overlap is honest ranking noise, not admission noise — different
  defect class, unchanged by this PRD).

## 4. Architectural decision

**D1 — Conjunction gate in `rank_chunks`:** eligibility =
`(overlap >= 1 || name_match) && score >= 520`.

- The overlap term is already computed (`:884-886`); the gate reuses it.
- 520 equals one literal token — the minimum honest signal. With the
  overlap conjunct present, the floor's only job is to suppress degenerate
  single-stop-word matches (stop-words are already filtered) and to keep
  the invariant legible in code.
- Name-match stays an independent admission path (`:891`, +3,500): the lean
  body's routing map promises deterministic addressing; G1's "or" clause
  preserves that contract.
- Rejected alternative — bare `score >= 700` floor: empirically tuned to the
  worst observed noise (§1.1); margin 22 pts; next collision breaks G4
  again with no structural guarantee. Rejected.

## 5. Implementation plan

### PR-1 — The anchor (Option D discipline)

`crates/vesper-memory/tests/chunk_routing_eval.rs`: noise-floor fixture with
**completely disjoint** fixture text (deliberately constructed to maximize
hash collision odds — many short tokens), prompting with vocabulary absent
from every manifest field, asserting **zero chunks loaded**. Must **fail on
the unhardened tree** — recorded receipt — and stay `#[ignore]`d until PR-2
flips it green (same pattern as ranker-hardening PR-1: a permanently red
tree breaks floor gates; the ignore is lifted in PR-2).

### PR-2 — The threshold

`rank_chunks` (`:896` filter): add the conjunction gate. One behavioral
line + constant `MIN_CHUNK_ROUTING_SCORE: i32 = 520` exported for tests.
No other scoring change. Re-run: eval ladder, exemplar 16/16, cross-talk
pin/control, full floor.

### PR-3 — G5 restoration + re-eval

Rewrite `exemplar_routing.rs::g5_proof_…` to the literal assertion
(`chunks.is_empty()`), replacing the bounded-form compromise and its
finding comment. Re-record exemplar + eval evidence. Apply the stop rule:
if any control or previously-resolving probe regresses, **stop and
re-derive** (a G3 violation would mean the conjunction gate is too tight —
e.g. an honest case scoring on cosine with overlap≥1 but <520, which the
arithmetic makes impossible since overlap≥1 ⇒ score ≥ 520).

## 6. Acceptance criteria

- **AC-1** Anchor fixture fails on unhardened tree (receipt), passes after.
- **AC-2** Restored exemplar G5 passes literally (zero chunks).
- **AC-3** D3 eval ladder: canonical table unchanged (controls true,
  probe ladders unchanged, `irrelevant_before` rows preserved).
  *(Corrected 2026-09-13 at PR-2: one `irrelevant_before` row and two
  secondary-route rows legitimately changed — the removed chunks were
  themselves zero-overlap cosine admissions (stop-rule probe receipt in
  `chunk-score-floor-pr2-execution.md` §4). Every success flag and every
  probe ladder is unchanged; see the execution report for the
  row-by-row diff.)*
- **AC-4** Exemplar 16/16 routing-first unchanged.
- **AC-5** Workspace floor ≥ 2,252 intact; acceptance 23/23; naming-guard
  clean (zero baseline erosion).
- **AC-6** Skill tier byte-identical (`AUTO_ACTIVATION_SCORE` untouched;
  skill-tier fixtures unchanged).

All claims trace to current scope-appropriate evidence per ADR 0028; the
stop rule in PR-3 governs.

## 7. Risks

- **Over-tight gate blocks honest routing** — structurally impossible for
  overlap≥1 (520 = one token), but PR-3's stop rule is the guard anyway.
- **Hash collision still ranks among eligible chunks** — intended; cosine
  orders candidates, admission requires signal. Residual is bounded and
  measured, not eliminated.

## 8. Open questions

- Q1: ~~Should the name-match path also require ≥1 token beyond the name
  itself?~~ **Resolved 2026-09-13 (owner directive, post-PR-2):** the
  chunk name must be **delimiter-bounded** (`chunk_name_matches`) — the
  name's own alphabet `[a-z0-9-_]` cannot continue past either edge of
  the occurrence — instead of `contains`. Embedded substrings
  (`comet` in `pcometq`, `auto-comet-review`) no longer admit; delimited
  addressing (`comet`, `"comet"`, `read comet,`) still routes. Red-receipt
  pin + control in `tests/chunk_routing_eval.rs`. Skill-tier
  `phrase_matches` deliberately unchanged (scope fence). Evidence:
  `docs/foundation/chunk-score-floor-q1-execution.md`.
- Q2: Expose `MIN_CHUNK_ROUTING_SCORE` in routing-rejection reasons when a
  chunk with overlap is suppressed? (Currently sub-floor chunks are simply
  ineligible — invisible in reports.) Default: no report noise until a real
  need appears.

## 9. Status history

- 2026-09-13: Planning (this document). Grounded in exemplar-migration
  §5.1 finding + offline replication; directive's example floor corrected
  to a conjunction per §1.1 analysis.
- 2026-09-13: PR-1 landed. Anchor pinned: offline-exact search found a
  1,440-pt pure-cosine disjoint pair (cos +0.6547, zero overlap, no
  name-match); the pin test fails on the unhardened tree with receipt and
  is `#[ignore]`d until PR-2. Genuine-overlap control green. Floor 2,253.
  Evidence: `docs/foundation/chunk-score-floor-pr1-execution.md`.
- 2026-09-13: PR-2 landed. Conjunction gate
  `(overlap >= 1 || name_match) && score >= MIN_CHUNK_ROUTING_SCORE`
  replaces `score > 0` in `rank_chunks`; anchor unignored and green;
  genuine-overlap control green; exemplar 16/16 green; stop-rule probe
  proved the two changed canonical rows were themselves zero-overlap
  cosine admissions (AC-3 letter corrected in §6). *(Master-audit correction 2026-09-13: the
  factual-retrieval SummaryOnly probe was NOT among the changed rows —
  its second chunk routes on a genuine `pipeline` overlap; the count is
  two rows, not three.)* Floor 2,256 (2,253 +
  2 prior-unit dollar tests + 1 unignored anchor). Acceptance 23/23,
  clippy clean, naming-guard clean. Skill tier byte-identical. Evidence:
  `docs/foundation/chunk-score-floor-pr2-execution.md`.
- 2026-09-13: Q1 resolved by owner directive. Chunk-name admission is now
  delimiter-bounded (`chunk_name_matches`): embedded substrings cannot
  admit, delimited names still route. Pin (red-first) + control added;
  skill tier untouched. Floor 2,258; acceptance 23/23; clippy, fmt,
  naming-guard clean. Evidence:
  `docs/foundation/chunk-score-floor-q1-execution.md`.
- 2026-09-13: **PR-3 landed — initiative COMPLETE.** G5 restored to the
  literal zero-chunks assertion. Stop rule fired en route and resolved
  test-side: the original prompt was never vocabulary-free (`finished` →
  stem `finish` overlaps phase4-analysis' description "experiments
  finish:"; exact offline replication). Corrected prompt verified
  zero-overlap against all 16 chunk pools while still carrying unhardened
  noise (phase8 +0.1826 → 401 pts, paper-types +0.0945, phase4 +0.0722).
  Non-vacuity receipts: literal G5 fails on the pre-PR-2 tree routing
  exactly those three chunks (original prompt: phase3/4/8; corrected
  prompt: phase8/paper-types/phase4). AC-2 satisfied literally. Final
  gates: floor 2,258 / 0 failed, acceptance 23/23, clippy, fmt,
  naming-guard clean. Evidence:
  `docs/foundation/chunk-score-floor-pr3-execution.md`.
