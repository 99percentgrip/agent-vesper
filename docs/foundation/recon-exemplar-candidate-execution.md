# Execution Report: Chunked-Skill Exemplar Reconnaissance

- **Work unit:** Swarm reconnaissance — exemplar hunt (read-only)
- **Directive source:** Fast-track directive "SWARM RECONNAISSANCE (EXEMPLAR HUNT)",
  2026-09-13
- **Status:** COMPLETE
- **Artifacts:** `docs/architecture/recon_exemplar_candidate.md` (new)
- **Prior work:** none this unit (clean tree at `75c1a55` on start)

## 1. Objective

Find the best seed-library candidate for the first native chunked skill; produce a
refactoring blueprint; touch nothing.

## 2. Methods / commands

- Library scan: `du -b` sizes, `grep -c '^## '` section counts over all 95 seed files;
  top-20 by size, top-25 by section density.
- Candidate reading: `grep -n '^#\{1,3\} '` heading maps + targeted reads of the top
  three candidates (`research-paper-writing` and the two runner-up skills, cited by
  role elsewhere per the naming embargo).
- Cap mechanics verification: `sed -n` reads of `skills.rs:245-280,540-560` (load paths:
  `read` full-file vs `read_catalog_prefix` bounded) and
  `skill_orchestrator.rs:605-700` (injection path: `truncate_chars(&body, maximum)` at
  619-623 with `maximum = MAX_SKILL_CONTEXT_CHARS.min(remaining)`).
- Per-section byte spans: Python byte-offset walk over the candidate file.
- Cap constants: `grep -n "MAX_CHUNK"` over `types.rs:27-38`.

## 3. Files

- Created: `docs/architecture/recon_exemplar_candidate.md` (this unit's only file)
- Modified: none (constraint 1 held; `git status` clean apart from this report + the
  recon doc)

## 4. Exact evidence

- Library scan (top of both rankings): `research-paper-writing.md` 74,109 B / 27 `##`
  sections — largest single-skill body in the library.
- Truncation proof: body injected is `truncate_chars(body, 24,000)` —
  `skill_orchestrator.rs:619-623`; 7 of 27 sections reachable; 20 sections
  (incl. Phases 5–8, paper types, reviewer rubrics) never injected.
- Shadow-paging evidence: 10 `references/*.md` sidecars (~1.3 MB total,
  `du -sb skills/skills/research-paper-writing/` = 1,337,276 B) with in-body
  `read_file` pointers, 6 of 10 pointing past the 24K cut.- Per-section spans: measured byte-offset table (recon doc §Refactoring Blueprint).
- Runner-up comparison: CLI-orchestration skill 34,092 B / 23 sections (1.4× cap) —
  same disease, smaller dose.

## 5. Deviations

- None. Read-only scope, naming embargo, and heuristic constraints all held. The
  directive's "326+ files" premise was corrected in-flight: the seed library is 95
  files (the 326 figure was the whole-tree `.md` count from an earlier session's
  survey); scan covered all 95.
- Self-caught documentation defects (F5 discipline): the first draft of the recon doc
  contained a duplicated table row, a broken source line, and miscounted chunk totals
  (17 vs 16). All three fixed before verification; see §6.

## 6. Unresolved items

- The 10 `references/*.md` sidecars (~1.3 MB) are intentionally left out of scope:
  consolidating them into chunks would blow the 32-chunk / byte caps and is a separate
  curation decision.
- The blueprint is a plan, not a change; implementing it is a future work unit
  (candidate: "exemplar migration" PR).
- Routing-quality verification (Stage D step 2) is specified but not executed — by
  design, read-only.

## 7. Readiness effect

The last deliberately-open item from the context-paging PR-5 report (no chunked seed
exemplar) now has a concrete, measured migration target. Executing the blueprint would
give the shipped chunk tier its first real-world usage, convert a 3.1×-over-cap body into
a ~6 KB lean body + 16 bounded chunks, and make Phases 5–8 of the research pipeline
reachable at all.

## 8. Traceability appendix
| recon doc claim | evidence anchor |
|---|---|
| 95 seed files | `ls skills/*.md skills/**/*.md` count 95 |
| 74,109 B / 27 sections | `du -b` + `grep -c '^## '` on candidate |
| body injection capped at 24,000 | `skill_orchestrator.rs:619-623` (`truncate_chars`) |
| 7 of 27 sections reachable | byte-offset walk vs cap |
| 20 sections never injected | same |
| 10 reference sidecars / ~1.3 MB | `du -sb skills/skills/research-paper-writing/` |
| 6 of 10 sidecar pointers past cap | byte offsets of the 10 pointer lines |
| 32-chunk / 24 KB caps | `types.rs:27-38` |
| chunk pool = raw tokens | `ranker-hardening-prd.md` Option A; audit |
| routing ranks summary/key_elements | post-ADOPT, `skill_orchestrator.rs:795-812` |
| totals: 16 chunks ≈ 67,978 B; body ≈ 6,131 B | section-span arithmetic |
