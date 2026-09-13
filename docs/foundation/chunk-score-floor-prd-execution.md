# Execution Report: Chunk Score Floor PRD Generation

- **Work unit:** PRD drafting (planning artifact)
- **Directive source:** Fast-track directive "SCORE FLOOR PRD GENERATION", 2026-09-13
- **Status:** COMPLETE
- **Prior work:** exemplar migration (same session, floor 2,252); uncommitted with this unit

## 1. Objective

Draft `docs/chunk-score-floor-prd.md` for eliminating the chunk-tier cosine
noise floor; update the DOX chain (docs/AGENTS.md, migration-status);
record in the evidence index.

## 2. Methods / commands

- Line-anchor grounding: `grep -n` over `skill_orchestrator.rs`
  (`:20` AUTO_ACTIVATION_SCORE, `:558` activation filter, `:887` cosine
  term, `:891` name-match, `:896` `score > 0` admission filter) and
  `exemplar_routing.rs` (`:221` finding comment).
- Floor sensitivity analysis (Python): floor/2200 = admitted noise-cosine;
  compared against the observed outlier set (678/207/166/163/154 pts).

## 3. Files

- Created: `docs/chunk-score-floor-prd.md` (8,007 B)
- Modified: `docs/AGENTS.md` (new PRD ownership bullet after
  ranker-hardening), `docs/migration-status.md` (PLANNING row after the
  Ranker hardening row), `docs/foundation/evidence-index.md` (entry below)

## 4. Exact evidence

- Problem anchored: admission filter `ranked.score > 0` at
  `skill_orchestrator.rs:896`; cosine term `:887`; measured outliers from
  exemplar-migration §5.1 (678 pts max, zero overlap).
- **Directive correction (§1.1 of the PRD):** the directive's example floor
  `score >= 520` does not implement its own stated invariant ("at least one
  literal token match") — the observed 678-pt noise outlier passes it.
  Sensitivity: a bare floor of 520 admits noise cosine up to 0.2364; 700
  admits up to 0.3182 (22-pt margin over the worst observation — empirical
  whack-a-mole). The PRD therefore specifies the **conjunction** gate
  `(overlap >= 1 || name_match) && score >= 520` (D1), with the bare-floor
  alternative explicitly rejected and recorded.
- Structure: 3-PR plan (anchor → threshold → G5 restoration + re-eval with
  stop rule), 6 acceptance criteria, risks, 2 open questions with defaults.

## 5. Deviations

- One material deviation from the directive's example value, justified and
  recorded in PRD §1.1 + §4 (conjunction instead of bare 520 floor). The
  directive's *stated objective* ("strictly ineligible without genuine
  semantic overlap") is what the conjunction implements; the example floor
  was the directive's illustrative mechanism, not its goal.
- No code changes (planning unit). Read-only on production sources.

## 6. Unresolved items

- Q1 (name-match path breadth) and Q2 (sub-floor rejection visibility) left
  open with defaults — PRD §8.
- Implementation awaits PR directives.

## 7. Readiness effect

The last open finding from the exemplar migration now has a scoped,
analyzed, anchor-first remediation plan. G5's literal form ("vocabulary-free
prompts route zero chunks") becomes implementable and provable.
