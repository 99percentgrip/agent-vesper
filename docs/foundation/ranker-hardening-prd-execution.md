# Ranker Hardening PRD Generation — Execution Report

- **Date:** 2026-09-12
- **Directive:** FAST-TRACK RANKER HARDENING PRD GENERATION — draft
  `docs/ranker-hardening-prd.md` from the alias cross-talk recon; update the
  DOX chain
- **Deliverables:**
  - `docs/ranker-hardening-prd.md` (new; ~11 KB)
  - `docs/AGENTS.md` (PRD ownership bullet + status refresh on the
    context-paging bullet)
  - `docs/migration-status.md` (new PLANNING row)
  - `docs/foundation/evidence-index.md` (this record's entry)
- **Workspace code changes:** none — documentation PRD generation only

## 1. Objective

Convert the read-only recon findings (`architecture/recon_alias_crosstalk.md`)
into the formal phased requirements document for eliminating the chunk-tier
alias cross-talk, fenced to Option A + Option D, with honest acceptance
criteria tied to the measured incident.

## 2. Methods and commands

- Grounded in the recon report's line-anchored ledger (mechanism
  `skill_orchestrator.rs:421-422/:867-869/:1300-1303/:1384-1385`; blast
  radius zero; residual analysis) — all anchors re-verified against source
  this session.
- PRD house format mirrored from `advanced-context-paging-prd.md`
  (status header, evidence table, goals/metrics, numbered PRs, ACs,
  constraints, risks, open questions).
- DOX chain: `docs/AGENTS.md` PRD ownership bullet; `migration-status.md`
  PLANNING row inserted after the context-paging row.

## 3. Files

- Created: `docs/ranker-hardening-prd.md`
- Modified: `docs/AGENTS.md`, `docs/migration-status.md`,
  `docs/foundation/evidence-index.md`

## 4. Exact evidence

- `cargo xtask naming-guard` → `naming-guard: clean (18 hits, all frozen in
  baseline)` — zero erosion.
- Anchor verification: all six `skill_orchestrator.rs:` line citations in the
  PRD re-checked in-tree (grep count 6; `:869` intersection and
  `:1292-1303` expansion loop confirmed present verbatim).
- `git status --porcelain` → this mission's footprint is
  `?? docs/ranker-hardening-prd.md` + the three modified doc files; every
  other listed file is a prior approved work unit's deliverable.

## 5. Methods deviation record

- The directive's problem statement named "chunk text"; the PRD keeps the
  recon's precise framing (prompt-pool expansion joining chunk pools at
  `:869`) because the routing text itself is never expanded with slug
  vocabulary — the bleed is at the join, and that distinction is what makes
  Option A the correct minimal fix.
- One drafting defect self-caught before shipping: a malformed AC id
  (`-3` → `AC-3`); a suspected broken cross-reference in M5 was investigated
  and found to be a false alarm.

## 6. Unresolved items

- Q1 (tracked in-PRD): latent recon defect classes (stem-form alias
  asymmetry, dead alias rows) default to a separate future initiative —
  skill-tier concerns, out of scope here.
- Q2 (tracked in-PRD): cosine noise floor qualification threshold —
  measurement task, default not-in-this-PRD.
- PR-1..PR-3 of this PRD are not yet executed; this was planning only.

## 7. Readiness effect

The hardening initiative is now formally scoped: mechanism line-anchored,
scope fenced to A+D with the skill tier frozen, the bounded residual
disclosed rather than hidden, the anchor-fixture-first sequencing mandated
(fail-then-pass, per the full audit's regression-first discipline), and the
D3-verdict stop rule written into PR-3 so eval drift cannot slip through.
Ready for PR-1 (the anchor fixture) on directive.
