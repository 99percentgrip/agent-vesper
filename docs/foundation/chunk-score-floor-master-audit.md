# Score-floor master audit (PR-1 → PR-3 + Q1)

Date: 2026-09-13. Status: **audit complete — initiative verdict upheld with corrections**.
Scope: `docs/chunk-score-floor-prd.md` and all four execution reports
(PR-1, PR-2, Q1, PR-3), re-traced to current code at commit `494019b`.

## Objective

Execute the final-audit discipline (work-unit-reporting skill): re-read
every requirement doc and execution report; re-trace each acceptance
claim to current code; sabotage-check every pin; re-derive invariants by
hand; re-read narrative claims against recorded tables; correct
overstatements in place with audit notes.

## 1. AC re-trace (PRD §6, all six, current code)

| AC | Claim | Current evidence | Verdict |
|---|---|---|---|
| AC-1 | Anchor fails unhardened, passes after | Sabotage (gate→`score > 0`) at HEAD: `noise_floor_pin_...` FAILED; restored: ok | ✅ |
| AC-2 | G5 literal zero chunks | `g5_proof_...` asserts `is_empty()`; gate-sabotage at HEAD makes it FAIL routing `[phase8, paper-types, phase4]` — matches offline replication digit-for-digit | ✅ |
| AC-3 | D3 ladder unchanged | Live table vs recorded: **exactly two rows differ** (procedure SummaryOnly `1|1`→`0|0`, procedure SKE `2`→`1`); all success flags identical; **narrative overstatement found — F5** | ✅ (substance) with F5 correction |
| AC-4 | Exemplar 16/16 | 4/4 suite green at HEAD | ✅ |
| AC-5 | Floor ≥2,252, acceptance 23/23, naming clean | 2,258/0, 23/23, 18 frozen; baseline diff since initiative start: 0 lines | ✅ |
| AC-6 | Skill tier byte-identical | `git diff 5cf5835~1..HEAD` (skill_orchestrator.rs): zero scoring-line changes outside chunk tier; `AUTO_ACTIVATION_SCORE` untouched; `phrase_matches` 4 skill-tier call sites untouched | ✅ |

## 2. Sabotage checks (vacuity defense) — all five

Each pin was reverted to its pre-fix behavior in a disposable worktree at
HEAD; every pin FAILED as required (a pin that survives its fix being
reverted is vacuous):

| Pin | Sabotage | Result |
|---|---|---|
| `noise_floor_pin_zero_overlap_prompt_routes_nothing` | gate → `score > 0` | **FAILED** ✅ |
| `chunk_name_embedded_substring_does_not_admit` | `chunk_name_matches` → `phrase_matches` (Q1-only, gate intact) | **FAILED** ✅ ✅ (ran both as sole sabotage and combined) |
| `chunk_name_delimited_still_routes` (control) | same | ok (control must hold both sides) ✅ |
| `g5_proof_...` (literal) | gate → `score > 0` | **FAILED** ✅ (routes exactly the predicted noise trio) |
| `cross_talk_pin_...` | gate sabotage | ok — **correct**: it guards alias expansion (ranker-hardening PRD), a different invariant; not a score-floor pin |

## 3. Invariants re-derived by hand

1. **Admission soundness.** `literal_signal = overlap>=1 || name_match`;
   gate = `literal_signal && score >= 520`. Zero-overlap ∧ ¬name ⇒ score
   can only come from cosine; cosine ⊆ score but `literal_signal=false`
   ⇒ structurally ineligible regardless of magnitude. Verified: anchor
   fixture (cos +0.6547 → 1,440 pts) blocked. ✅
2. **No over-tightening** (G3). overlap≥1 ⇒ score ≥ 520 by the 520/token
   arithmetic (`overlap.min(8) * 520`). So the floor conjunct is implied
   by the signal conjunct whenever overlap≥1; name-match ⇒ +3,500 > 520.
   ⇒ the gate is *exactly* "literal signal required", with the floor
   serving only as a legibility/stop-word guard. Honest chunks cannot be
   blocked. ✅ (matches PRD §7's derivation)
3. **Budget path (F1 decrement).** Re-read the chunk-load loop:
   `per_skill_remaining` decrements per loaded chunk; over-budget chunks
   skipped with explicit rejection reasons, never truncated; `allowance
   == 0 ⇒ break`. ✅
4. **Q1 matcher alphabets.** `chunk_name_matches` boundary charset =
   `[a-z0-9_-]` continuation; validator charset = `[a-z0-9_-]` (leading
   `-` rejected). Identical sets ⇒ no legal name can be missed. Initially
   suspected a drift (drafted finding F1), re-derived, **withdrawn** —
   the sets coincide. (Withdrawal recorded per honest-audit discipline.)
5. **Redundancy drift.** `literal_signal` is *not* duplicated logic: it
   is the single admission term; the `score >= 520` conjunct is implied
   (invariant 2) but kept per PRD D1 for legibility. Documented, not
   accidental. ✅

## 4. Narrative-vs-table re-read (where reports overstate)

- **F5 (material): PR-2 report miscounted the changed table rows —
  "three rows" with the factual-retrieval SummaryOnly probe listed; the
  definitive byte-diff shows exactly two rows changed, and the factual
  row is unchanged (`true | 0 | 2 | 13` live = recorded) because its
  second chunk routes on a genuine `pipeline` token overlap, which the
  gate correctly preserves.** Miscount was a table-reading error during
  PR-2, propagated into the chat summary. **Corrected in place** in the
  PR-2 report and the PRD status history with explicit audit notes; the
  migration-status "3 noise chunks" phrase refers to G5's routed chunks
  and was verified correct as-is. No conclusion changes: no success flag
  changed anywhere (AC-3 substance holds).
- **F2 (material): the PR-4 eval report's recorded canonical table is
  stale vs. live output on rows 29–30** (pre-gate measurement). Rather
  than rewriting measured history, an explicit **supersession note** was
  added after the audit-correction block, stating the live values, the
  reason (zero-signal removal), the runtime-probe proof, and that the
  ADOPT verdict inputs are unaffected. The report's "Reading the table"
  narrative paragraph about the procedure probe remains accurate for the
  pre-gate measurement it describes.
- **F3 (hygiene): stale flag-off comment** in
  `skill_routing.rs::metadata_fields_ranking_follows_the_shipped_flag_state`
  described pre-gate arithmetic (`score > 0` qualifies `logging`) that no
  longer exists; corrected to describe gate-era truth (literal overlap).
  Additionally the flag-on branch of that conditional proof asserted
  nothing — it now positively asserts `logging` is admitted (literal
  overlap under the gate), so both branches assert. Green at HEAD.

## 5. Regression-first receipts for fixes

- F3 flag-on assertion: added against the current (hardened) tree — the
  assertion asserts gate-era behavior; on a gate-reverted tree with flag
  on it would still pass (logging admits via score>0 too), so it is not
  a gate pin; it exists to close the silent branch. (Honest labeling: a
  *branch-coverage* assertion, not a *regression* pin.)
- F5/F2: documentation corrections with notes; no code change.

## 6. Verdicts re-derived from corrected data

- **PR-2's conjunction gate:** unchanged — soundness and
  no-over-tightening re-derived (§3.1–3.2); all pins sabotage-verified.
- **PR-3's G5 restoration:** unchanged — literal assertion, non-vacuous
  (fails pre-gate, passes hardened), fixture verified zero-overlap.
- **D3 ADOPT:** re-derived on the current table — both metadata probes
  improve SKE-vs-Flat across both families ⇒ adopt=true; shipped
  `CHUNK_METADATA_ROUTING_ENABLED = true` matches (asserted live by
  `d3_verdict_follows_the_decision_rule`). **Stands.**
- **Initiative COMPLETE:** stands, with F5's corrections folded in.

## 7. Current-tree gate receipts (post-audit-fixes)

```
cargo test -p vesper-memory --test skill_routing metadata_fields → ok (new flag-on assert)
full_metrics table live diff vs recorded → exactly rows 29–30 (supersession noted)
naming-guard baseline diff since 5cf5835~1 → 0 lines
```

Full battery re-run after all audit corrections is in §8.

## 8. Final verification receipts (this audit, final state)

- `cargo test --workspace --all-features`: **2,258 passed / 0 failed**
- `cargo xtask acceptance`: **23/23**
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: clean
- `cargo fmt --check`: clean
- `cargo xtask naming-guard`: **clean (18 hits, all frozen)**
- D3 verdict assertion live: adopt=true = shipped flag

## 9. Findings summary

| # | Severity | Finding | Resolution |
|---|---|---|---|
| F1 | withdrawn | Suspected Q1 alphabet drift | Re-derived; sets coincide. Withdrawn. |
| F2 | material (docs) | PR-4 recorded table stale vs live on rows 29–30 | Supersession note added to PR-4 eval report; history preserved |
| F3 | hygiene | Stale pre-gate comment + silent flag-on branch in `metadata_fields` proof | Comment corrected; flag-on positive assertion added; green |
| F5 | material (docs) | PR-2 report + PRD + chat summary miscounted changed D3 rows ("three" → actually two; factual row unchanged) | Corrected in place with audit notes in PR-2 report + PRD |

No code defects found in the shipped gate, matcher, or tests: all five
sabotage checks failed as required, all invariants re-derived, skill
tier byte-identical, naming baseline zero-erosion.

## Readiness effect

The score-floor initiative's completion claim now traces to
audit-verified evidence: every pin sabotage-proven non-vacuous, every AC
re-traced to current code, narrative overstatements corrected in place,
and the stale-table supersession recorded. The gate, Q1 matcher, and G5
literal proof stand as delivered.
