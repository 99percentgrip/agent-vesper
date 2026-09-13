# Recon: Chunked-Skill Exemplar Candidate

- **Date:** 2026-09-13
- **Mission:** Identify the first native chunked-skill exemplar in the seed library
- **Scope:** Read-only reconnaissance; no workspace file modified
- **Status:** COMPLETE — candidate selected, refactoring blueprint below
- **Related:** `recon_context_paging.md` (patterns), `advanced-context-paging-prd.md` (tier
  contract), `skills/AGENTS.md` (chunk-tier rules), `ranker-hardening-prd.md` (raw pools)

## Purpose

The context-paging initiative shipped a complete chunk tier (PR-1..PR-5 plus audits and
the ranker hardening) but the seed library still contains **zero chunked skills** — the
tier is exercised only by test fixtures. PR-5's report listed a chunked seed exemplar as
the one deliberately-open item, gated on a curation decision. This reconnaissance
identifies the best candidate and produces the refactoring blueprint, without touching
any file.

## The Target

**`research-paper-writing`** (file `skills/skills/research-paper-writing.md`, 74,109 bytes,
27 `##` sections, 2,229 lines — the largest single-skill body in the library).

Supporting evidence — library-wide scan (95 seed files; sizes and `##`-section counts
computed with `du -b` / `grep -c '^## '`):

| rank | file | bytes | `##` sections |
|---|---|---|---|
| 1 | research-paper-writing.md | 74,109 | 27 |
| 2 | ascii-video/references/effects.md | 72,928 | 13 |
| 3 | ascii-video/references/shaders.md | 50,435 | — |
| 4 | touchdesigner-mcp/references/network-patterns.md | 33,603 | — |
| 5 | humanizer.md | 34,603 | 15 |
| 6 | *(CLI-orchestration skill, 34,092 B)* | 34,092 | 23 |
| 7 | *(design-review skill, 25,059 B)* | 25,059 | 30 |

Ranks 2–4 are `references/` sidecar files — already outside the injected body, so
"knowledge bloat" there is benign. (Two seed skills carry third-party names; per the
naming embargo they are cited here by role, not by slug.) Ranks 5–7 are 25–35 KB
bodies; the rank-6 CLI-orchestration skill (23 sections) is the runner-up and a strong
future candidate.

## The Justification — why it fits the chunking heuristic perfectly

The PR-5 authoring heuristic (`skills/AGENTS.md`, `vesper-skill-authoring`): *use chunks
when knowledge density requires bounding the context — selectively-read reference
material that would otherwise force a bloated body or an artificial skill split; keep one
body when everything is needed on every activation.*

Four independent measurements make this skill the textbook case:

1. **The body is 3.1× the injection cap.** The orchestrator hard-caps every injected
   body at `MAX_SKILL_CONTEXT_CHARS` (24,000) via
   `truncate_chars(&body, maximum)` (`skill_orchestrator.rs:619-623`). A 74,109-byte body
   means **only the first 24,000 chars — 7 of 27 sections — are ever injected.**
2. **The invisible knowledge is the deepest knowledge.** The 20 unreachable sections
   include Phase 5 (paper drafting), Phase 6 (self-review/revision), Phase 7 (submission,
   10,176 B), Phase 8 (post-acceptance), paper types, workshop papers, agent integration,
   and the experiment-log template. The agent currently executes Phases 5–8 of a
   research paper **blind**, with the actual procedures sitting unreachable in the file.
3. **The skill already maintains a shadow manual-paging system.** Ten `references/*.md`
   sidecars (~1.3 MB total) exist precisely because the body cannot hold the depth —
   with in-body `read_file` pointers (7 of which lie beyond the 24K cut). This is
   the `context upstream` two-tier pattern discovered in recon, re-implemented by hand:
   a root file + per-topic files, loaded on demand. The chunk tier is the native
   mechanism the author was forced to improvise.
4. **Distinct-topic sections, cleanly divisible.** 27 `##` sections at clear boundaries
   (per-section byte spans measured and tabled below). Multiple independent topics
   (phases, paper types, reviewer rubrics) — not a persona/core-loop skill where every
   activation needs everything. It passes the always-needed filter: the front matter +
   pipeline diagram + When-To-Use + Core Philosophy (~5–6 KB) is always needed; the rest
   is phase- or topic-scoped reference material.

Runner-up: the CLI-orchestration skill (34 KB / 23 sections) has the same truncation
problem (body is 1.4× cap) but the depth already lives in section-local tables, making
the chunking win smaller. The chosen target maximizes measurable harm per byte.

## The Refactoring Blueprint

Target shape: a lean always-on body (~5–6 KB: frontmatter + diagram + When-To-Use + Core
Philosophy + a phase→chunk routing map) plus `chunks/` files carrying the phase- and
topic-scoped knowledge. Constraint check up front: **32 chunks × 24,000 B** — the
library's 10 largest sections total ~92 KB as raw sections; after de-duplication and
removal of body-redundant material, the plan fits comfortably under both caps.

### Stage A — extraction

Extract these `##` sections to `skills/skills/research-paper-writing/chunks/<name>.md`
(byte spans measured from the current file; "→" means "merges with"):

| chunk file | source section(s) | size (B) |
|---|---|---|
| `phase0-setup.md` | Phase 0: Project Setup | 4,793 |
| `phase1-literature-review.md` | Phase 1: Literature Review | 4,345 |
| `phase2-experiment-design.md` | Phase 2: Experiment Design | 5,074 |
| `phase3-execution-monitoring.md` | Phase 3: Execution & Monitoring | 3,843 |
| `phase4-analysis.md` | Phase 4: Result Analysis + experiment-log bridge block | 6,014 |
| `phase5-drafting.md` | Phase 5: Paper Drafting | 469 (pointer) |
| `phase6-review-revision.md` | Phase 6: Self-Review & Revision | 7,089 |
| `phase7-submission.md` | Phase 7: Submission Preparation (incl. its Setup / Reproduction / Citation sub-headings) | 10,176 |
| `phase8-post-acceptance.md` | Phase 8: Post-Acceptance Deliverables | 2,235 |
| `paper-types.md` | Paper Types Beyond Empirical ML | 2,915 |
| `workshop-short-papers.md` | Workshop & Short Papers | 1,600 |
| `iterative-refinement.md` | Iterative Refinement: Strategy Selection | 4,626 |
| `agent-integration.md` | the agent Integration (incl. the `Experiment: <name>` log template that follows it, 864 B) | 7,866 |
| `reviewer-criteria.md` | Reviewer Evaluation Criteria | 1,012 |
| `common-issues.md` | Common Issues and Solutions | 1,843 |
| `reference-docs.md` | Reference Documents | 4,078 |

Folds: the experiment-log template block (Contribution / Experiments Run / Figures /
Failed Experiments / Open Questions — five short `##` headings, 1,351 B total, the
Step 4.6 bridge content) folds into `phase4-analysis.md` (4,663 + 1,351 = 6,014 B).

**Totals: 16 chunk files ≈ 67,978 B; lean body ≈ 6,131 B** (74,109 − chunked).
Worst-case activation: ~6.1 KB body + ≤3 routed chunks (~12 KB) ≈ **19 KB — inside the
24 KB per-skill cap**, versus today's fixed 24 KB truncated slice that can never
reach past Phase 4.

### Stage B — manifest

Add to frontmatter a `chunks:` block, one entry per file, per
`skills/AGENTS.md` (name unique-slug required; description ≤240 chars required; summary
≤480 chars optional; key_elements ≤8 optional). Routing post-ADOPT ranks `summary` and
`key_elements` (`skill_orchestrator.rs:795-812`), so:

- descriptions describe *what task state triggers this chunk* ("Load when the user is
  drafting…" rather than restating the title).
- key_elements carry distinctive vocabulary (post-hardening, chunk pools are raw
  stemmed tokens — `ranker-hardening-prd.md` Option A — so no alias expansion rescues
  weak key_elements; they must be literally present to score).
- chunk vocabularies must be distinct from the slug `research-paper-writing` and from
  each other (PR-4 pitfall: echoing slug tokens boosts every chunk equally).

### Stage C — the lean body

Keep in-body: frontmatter (+ chunks manifest), pipeline diagram, When-To-Use, Core
Philosophy, and a new **phase→chunk routing map** (a compact table: phase number →
chunk name → one-line purpose), then a per-phase skeleton that says "for full phase
procedure, chunk `phaseN` routes here when triggered." Remove from body: everything now
in chunks. The `references/*.md` sidecars remain untouched (they are the manual-paging
layer the chunk tier formalizes; deeper consolidation is a separate curation decision).

### Stage D — verification

1. `validate_chunk_manifest` green: 16 entries, all files exist, every file ≤24,000 B
   (max actual: 10,176).
2. Routing check across the phase vocabulary (one synthetic prompt per chunk topic):
   target chunk ranks first for its phase; body always injects; total per-activation
   injection stays ≤24,000 (per-skill) — i.e., lean body + ≤3 routed chunks ≈ 5–6 KB +
   ≤3 × ~5 KB ≈ 20 KB, inside the cap.
3. PRD M-gates re-run: eval ladder unchanged, floor intact.
4. G5: a prompt with no chunk vocabulary routes the lean body only — chunk-less
   behavior byte-identical to any other skill.

## Risks & mitigations

- **Knowledge loss through over-extraction.** Mitigation: keep the always-needed core
  (philosophy, workflow philosophy, when-to-use) in body; only phase/topic reference
  goes to chunks.
- **Routing misses make knowledge unreachable via native paging.** The body retains the
  explicit phase→chunk map so the agent can `read_chunk`-equivalent (the
  `read_section`/`read_skill` tools) deliberately as fallback.
- **Curation churn in a shipped seed library.** The exemplar must be re-verified after
  the seed-mirroring flow (global copy updated in lockstep).

## Sources

- Library scan: `du -b` + `grep -c '^## '` over all 95 seed files (2026-09-13)
- Per-section spans: byte-offset table computed from the candidate file
- Body-cap mechanics: `crates/vesper-memory/src/skill_orchestrator.rs:605-623`
- Chunk caps: `crates/vesper-memory/src/types.rs:27-38`
- Chunk-tier rules: `skills/AGENTS.md` (Local Contracts)
- Always-needed filter: `vesper-skill-authoring` "On-demand chunks" section
