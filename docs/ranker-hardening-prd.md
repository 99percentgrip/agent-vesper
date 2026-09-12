# Ranker Hardening PRD

- **Status:** COMPLETE — PR-3 re-eval passed the stop rule (canonical table identical under the hardened ranker; verdict re-derived ADOPT)
- **Target:** `crates/vesper-memory` (chunk-tier routing only)
- **Owner:** `vesper-memory` maintainers; docs owned by `docs/AGENTS.md`
- **Related:** `docs/architecture/recon_alias_crosstalk.md` (recon evidence),
  `docs/advanced-context-paging-prd.md` (the chunk tier this hardens),
  `docs/foundation/context-paging-pr4-eval.md` (the incident),
  `docs/foundation/recon-alias-crosstalk-execution.md` (recon execution record)

## 1. Problem statement

The skill chunk tier (advanced-context-paging PRD, PR-1..PR-5, shipped) reuses
the **prompt token pool built at `skill_orchestrator.rs:421-422`** — a pool
that includes alias expansion over every prompt token, including any skill
slug the user typed. Chunk pools, by contrast, are built from chunk routing
text only (`:867-868`). The join at `:869` (`prompt_tokens ∩ entry_tokens`)
then manufactures false semantic overlap when a slug token is an alias source
and a *different* chunk's routing text contains a related token:

```
prompt "…use skill deploy-runbook…"
  :422  semantic_tokens → {…, deploy}           (slug token)
  :1300-1303  deploy is source of row :1384
         → prompt pool gains {publish, release, production}

chunk "rollback", description "Undo an unsuccessful release"
  :868  semantic_tokens → {…, release}          (related token)
  :1300-1303  release is source of row :1385
         → chunk pool gains {publish, deploy, version}

:869  intersection = {deploy, publish, release} → 3 tokens
:873  score += 3 × 520 = 1,560 manufactured points
```

The PR-4 evaluation hit this live: the `deploy-runbook` slug routed the
`rollback` chunk on 1,560 points of overlap that exists nowhere in either
literal text (plus a cosine term). The corpus was renamed to `cutover-runbook`
to dodge the trap; production behavior shipped unchanged pending this PRD.
This is a routing-correctness defect, not a security boundary: the permission
gate remains authoritative, and chunks never grant anything — but a
deliberately wrong chunk displaces the right one (top-3, `take(3)` at `:654`)
and spends context budget on noise.

### Current-state evidence

| ID | Claim | Anchor |
|----|-------|--------|
| E1 | Prompt pool tokenizes the whole prompt incl. slug, with alias expansion | `skill_orchestrator.rs:421-422`, `:1292-1303` |
| E2 | Chunk pool is routing-text-only (description/summary/key_elements) | `skill_orchestrator.rs:867-868` |
| E3 | The join at `:869` is where expansion-only tokens meet | `skill_orchestrator.rs:869` |
| E4 | Incident scored 3 tokens × 520 = 1,560 pts (not 520 as first recorded) | `:869`, `:873`; recon §1.2 correction |
| E5 | Overlap term weights 520/token (cap 8), cosine ×2,200, name +3,500 | `skill_orchestrator.rs:872-882` |
| E6 | Skill tier tokenizes `description + tags + name` — same alias loop | `skill_orchestrator.rs:804-809` |
| E7 | Blast radius of one-sided alias removal over shipped tests: zero | recon §3; `skill_routing.rs:121-146,173-212,224-253,260-276,323-356,363-386,400-437,449-475`; eval corpus `:78/:90` |
| E8 | The cross-talk class is untested — no fixture's only overlap is alias-manufactured | recon §3 corollary |

### Residual exposure (honest scope)

Option A removes **chunk-side** alias expansion. Prompt-side expansion remains
(skill-tier behavior, out of scope by constraint 2), so a prompt token that
alias-expands may still match a **literal** token in chunk routing text: the
incident chunk would still score a 1-token overlap (520 pts) through the
prompt's `deploy → release` expansion meeting rollback's literal `release`.
The full 3-token/1,560-pt fan-out requires both sides expanded. Options B/C
from the recon would close the residual but require new evidence and alter
skill-tier behavior; they are explicitly rejected here. The PRD accepts the
bounded 520-pt residual and records it.

## 2. Goals

- **G1** — Eliminate the chunk-tier alias fan-out: chunk pools are built from
  raw stemmed tokens (stop-words applied, alias loop bypassed) so the chunk
  tier never inherits expansion it never authored.
- **G2** — Anchor the defect: a failing fixture pins the buggy behavior before
  the fix lands, so remediation is an observable delta, not silent drift.
- **G3** — Zero regression: global skill-tier routing (scoring, activation,
  selection, conflicts, budgets) is byte-identical; the 2,217+ workspace test
  floor holds; the PR-4 eval ladder holds.
- **G4** — Evaluation honesty: the D3 verdict's empirical basis survives the
  change; the eval re-run under the hardened ranker reconfirms the ladder.
- **G5** — Traceability: fix, fixture, and re-eval trace to recon anchors
  (`docs/architecture/recon_alias_crosstalk.md`).

## 2.1 Success metrics

- **M1** (anchor): the cross-talk fixture fails on the pre-fix tree, records
  the manufactured-overlap outcome, and passes post-fix. Fail-then-pass is
  mandatory, not optional.
- **M2** (decoupling): post-fix, a prompt whose slug token is an alias source
  cannot gain >520 overlap points against a chunk whose routing text contains
  only a related token (i.e., the fan-out `{deploy, publish, release}` at
  1,560 pts is impossible; a 1-token 520-pt literal-residual is allowed).
- **M3** (floor): workspace test count ≥ 2,217 passing, zero failures; the
  full routing suites (chunk-store 12, routing 8, eval 5, composition 6) green.
- **M4** (eval ladder): the PR-4 canonical table re-runs unchanged in its
  success column (controls pass; probes resolve per condition), with overhead
  numbers re-measured under the hardened ranker and re-recorded if they move.
- **M5** (skill tier frozen): `score_candidate` behavior unchanged — verified
  by a G3 proof asserting identical selection/score/reasons on a fixture set
  exercising aliases (`spreadsheet`/`excel`, `deploy`/`release`) at the
  **skill** level.

## 5. Implementation plan

### PR-1 — The anchor (Option D) — `vesper-memory/tests/chunk_routing_eval.rs`

**Mandate:** write the failing cross-talk fixture BEFORE the fix.

- Extend the eval corpus (or the closest existing harness) with a case whose
  **only** overlap is alias-manufactured: prompt contains slug token `deploy`
  (alias source, row :1384); chunk `rollback`'s description contains literal
  `release` (row :1385 related); **no** literal token shared between prompt
  and any chunk routing text.
- Assert the pre-fix outcome: rollback loads/ranks on manufactured overlap —
  fail on the current tree, recording the 1,560-pt evidence.
- The fixture must also carry a no-alias control (same shape, no alias-source
  slug token) proving the mechanism is the alias fan-out, not general overlap.
- Gate: `cargo test -p vesper-memory chunk_routing_eval` shows the new case
  **failing for exactly the audited reason** (manufactured top-3 selection),
  all prior cases green.

### PR-2 — The decoupling (Option A) — `skill_orchestrator.rs`

**Mandate:** `rank_chunks` builds chunk pools from raw stemmed tokens,
bypassing the alias loop (`:1300-1303`) for the chunk tier only.

- Add a raw-token variant (e.g. `semantic_tokens_raw`) or a scoped
  `semantic_tokens` parameter — implementation detail; the contract is: chunk
  `entry_tokens` = stem + stop-words, no alias expansion. Prompt pool
  (`:422`) and skill tier (`:804-809`) are untouched.
- Keep `semantic_tokens` itself unchanged for the skill tier; no changes to
  `score_candidate`, activation thresholds, or selection/budget logic.
- The PR-1 fixture flips to passing; the no-alias control stays green.
- Gate: full `cargo test -p vesper-memory` green; `cargo xtask verify`;
  architecture + naming-guard clean.

### PR-3 — Re-evaluation under the hardened ranker

- Re-run the PR-4 canonical ladder (`chunk_routing_eval.rs` emit + self-tests)
  under the hardened ranker; re-measure the overhead columns.
- If success columns move: investigate before landing — controls must still
  pass, probes must still resolve per condition; if the D3 verdict basis
  moved, stop and re-run the decision rule, do not proceed.
- Record: eval delta note appended to
  `docs/foundation/context-paging-pr4-eval.md` (or a successor report) citing
  exact before/after numbers.
- Gate: M3 + M4 satisfied; verdict basis intact or re-derived.

## 6. Acceptance criteria

- **AC-1** — The cross-talk fixture exists, failed on the pre-fix tree, and
  passes post-fix (M1); the no-alias control proves the mechanism.
- **AC-2** — Chunk pools are alias-free (G1/M2): a reviewable diff shows the
  chunk pool construction bypassing `:1300-1303`; skill tier untouched (M5).
- **AC-3** — PR-4 eval ladder holds (M4); any overhead drift re-measured and
  recorded.
- **AC-4** — Workspace floor ≥ 2,217/0 (M3); `cargo xtask acceptance` 20/20;
  clippy/fmt clean; architecture + naming-guard clean.
- **AC-5** — Docs: PRD status updated; migration-status row updated;
  evidence-index entry records the fix, fixture, and re-eval.

## 7. Evidence standards and constraints

- **Naming embargo:** all references to the context-upstream material use the
  alias only; `cargo xtask naming-guard` clean, zero baseline erosion.
- **ADR 0028 completion assurance:** every acceptance claim traces to
  current, scope-appropriate evidence; a checklist tick is not evidence.
- **Scope containment (constraint 2):** Option A + D only. No edits to
  `score_candidate`, `AUTO_ACTIVATION-tier logic`, alias table contents, or
  any skill-tier scoring. Any PR that touches skill-tier routing arithmetic
  reopens this PRD.
- **Residual disclosure:** the bounded 520-pt prompt-side residual is a
  recorded, accepted limitation (§1 Residual), not a hidden one.

## 8. Risks and mitigations

- **R1 — Eval ladder drifts post-fix.** The corpus's only alias source is
  `release` in rollback's description, and no query contains its expansions —
  recon proved the expansion is dead weight there. Mitigation: PR-3 re-run +
  re-measured overhead; stop rule if controls move.
- **R2 — Hidden coupling breaks composition tests.** Recon E7 proved zero
  shipped assertions depend on chunk-side expansion. Mitigation: full-suite
  gate in PR-2; composition suite (`context_paging_composition.rs`) included.
- **R3 — Fixture invalidity.** The anchor fixture must fail for the audited
  reason, not incidental mismatch. Mitigation: no-alias control case + M2
  assertion of exact post-fix cap (≤520).
- **R4 — Scope creep into B/C.** One-parameter change, reviewable diff;
  PR boundary: any table/arithmetic edit = reopen PRD.

## 9. Open questions

- Q1: Should the latent defect classes from recon §5 (stem-form alias
  asymmetry `releases→releas`; dead hyphenated rows) be folded into this PRD
  or tracked separately? **Default: separately** — they are skill-tier alias
  table concerns, out of scope here (constraint 2), recorded in the recon
  report for a future initiative.
- Q2: Does the cosine noise floor (recon §5.3) warrant a chunk-tier
  qualification floor above `score > 0`? **Default: not in this PRD** — needs
  measurement; noted for the eval harness to surface data on.
