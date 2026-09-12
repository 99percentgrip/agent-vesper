# External Reconnaissance: On-Demand Context Paging & Structured Knowledge Extraction (context upstream)

Read-only reconnaissance into the on-demand knowledge-paging and structured
knowledge-extraction patterns of an external repository, referred to
exclusively as **context upstream**. No production code was read, written, or
modified in either the external source or the Vesper workspace. Sources were
analyzed from documentation and schema definitions only (`.md`, fixture
`.json`) to bound context ingestion; the upstream's Python implementation
(62 files) was never opened.

Alias `context upstream` was assigned by Alex in the mission directive, per
the alias-request contract in `docs/architecture/AGENTS.md`.

## Evidence pinning

| Source | Commit | License | Constraint on reuse |
|---|---|---|---|
| context upstream (document→agent-skill converter) | `01f8a742aeb9022df59ad61870469d07f299c479` (shallow HEAD, 2026-09-11) | **MIT** (holder recorded in the pinned commit's `LICENSE.md`) | Concepts mappable; no code or text copied into Vesper. |

Citations below are repository-relative doc paths in `.tmp_recon/` (deleted
after this report; re-clone at the pinned commit to re-verify).

## Mission scope and constraint compliance

- **Read-only:** the only file created in the workspace is this report. The
  temporary clone lived under `.tmp_recon/` (ignored via `.git/info/exclude`,
  chosen over a `.gitignore` edit because the mission also forbade modifying
  existing files) and was deleted at closeout.
- **No code ingestion:** Drivers read `.md` files and the two schema fixture
  `.json` files only. No `.py` file was opened by any agent in this mission.
- **Naming embargo:** upstream identifiers do not appear in this report. This
  report added zero new `cargo xtask naming-guard` hits (verified §Trace
  verification).

## Driver ledger index

Two bounded Drivers ran in parallel. Their full ledgers are the evidence base
for every claim in this report; each synthesized claim below carries `[D1-n]`
or `[D2-n]` trace tags.

- **Driver 1** (concepts; `README.md`, `docs/index.md`, `docs/how-it-works.md`,
  `docs/usage.md`): findings D1-1 … D1-30.
- **Driver 2** (structure, templates, schemas; directory inventory,
  `SKILL.md`, `docs/architecture.md`,
  `docs/research/progressive-disclosure-evals.md`,
  `evals/fixtures/pd03-metadata.json`): findings D2-1 … D2-39.

Quotes in the ledgers were spot-checked verbatim against the pinned clone
before synthesis (README.md:44, docs/how-it-works.md:76, SKILL.md:250,
SKILL.md:388 all confirmed).

---

## Part 1 — What context upstream is (concept level)

**R1.** Context upstream is a document→agent-skill converter: a hybrid of a
deterministic Python extractor and a spec-driven generator, where the agent
follows a `SKILL.md` specification to convert a book-length source into a
packaged, paged skill artifact. [D1-3, D2-1, D2-26]

**R2.** Its pipeline runs input resolution → per-format tool selection →
deterministic extraction → structure analysis → per-chapter synthesis →
supporting artifacts → install, with four operating modes gating how many
stages run (Full Conversion, Analyze Only, Generate from Prior Analysis,
Update/Fold-in). [D1-1..D1-7, D2-12, D2-13]

**R3.** The governing philosophy is *extract structure, not summaries*:
sources are decomposed into a fixed taxonomy — named frameworks, actionable
principles, techniques, anti-patterns, and voice calibration — with the rule
that an author's precise formulations are not interchangeable with paraphrase.
[D1-5, D2-11, D2-25]

**R4.** The packaged output is a directory of five artifact classes:
`SKILL.md` (core mental models + chapter/topic index), `chapters/chNN-*.md`
(one file per chapter), `glossary.md`, `patterns.md`, `cheatsheet.md`, each
with an explicit token budget. [D1-26, D2-20, D2-21]

---

## Part 2 — On-demand knowledge paging: mechanism anatomy

This is the mechanism the mission targeted. Synthesized from both ledgers, the
paging design has six load-bearing elements:

**R5. Two-tier representation.** An *always-loaded tier* (root `SKILL.md`:
description, core frameworks, chapter index, topic index — ~4,000 tokens) and
an *on-demand tier* (per-chapter files, ~1,000 tokens each, loaded only when
the topic becomes relevant). The at-rest footprint is bounded and small; the
per-query cost is proportional to the question, not the book. [D1-9, D1-10,
D1-11, D1-12, D2-27]

**R6. Index as page table.** The topic index ("**<Term>** → ch<N>") is the
routing table that maps a user query to exactly one chunk file path; the agent
reads that file and answers from real content rather than hallucinating.
[D1-11, D1-14, D2-21]

**R7. Front-loading against compaction.** Because host-side compaction
truncates from the end, the most important content is placed first in the
always-loaded file, sized to fit inside the observed ~5,000-token compaction
survival window. [D1-13, D1-22, D2-21, D2-26]

**R8. Density-over-completeness as the context-economy rule.** A 1,000-token
synthesis beats a 10,000-token excerpt; raw source text is never embedded in
the artifact. [D1-15, D2-25]

**R9. Amortized structuring cost ("discovery-loop tax").** A PDF-reading agent
re-navigates the source every turn (re-fetch ToC, backtrack, re-process);
context upstream pays the structuring cost once at conversion so every later
query stays proportional to the answer. This is stated as the core design
principle: compile-time over runtime. [D1-16, D2-26]

**R10. Corpus paging during generation.** For sources over ~50k tokens, the
spec prescribes REPL-style access to the extracted corpus — treat
`full_text.txt` as a queryable corpus, not a single read — using word counts,
chapter-heading greps, and windowed line-range reads instead of whole-file
loads. [D2-16]

**R11. Explicit metadata schema for routing.** The research plan defines a
one-level paging representation: root metadata with a book-level
`description`, plus per-chunk `description` (required), `summary` (required),
and `key_elements` (optional) — the exact schema realized in the fixture:

```json
{
  "title": "...", "description": "...",
  "chunks": [
    { "description": "...", "summary": "...", "key_elements": ["..."] },
    { "description": "...", "summary": "..." }
  ]
}
```

`description`/`summary`/`key_elements` live in the always-loaded index and
gate *routing*; raw chunk payloads are gated behind on-demand load. [D2-30,
D2-38, D2-39]

**R12. Measured claim: 24×–51× token reduction** versus dumping the book into
context to answer one question, "measured on real books." **Verification gap,
preserved:** the methodology and per-book tables live in
`docs/performance.md`, which neither Driver read; the measuring instrument is
a Python tool no agent in this mission opened. Treat R12 as an upstream claim,
not a verified number. [D1-17, D1-24]

---

## Part 3 — The structured extraction framework specification

**R13. Stage machine.** `SKILL.md` prescribes Steps 0–11: out-of-scope check →
input validation (extension allowlist) → content-type classification
(`BOOK_TYPE ∈ {technical, text}`) → extraction → pre-flight cost estimate →
REPL-style corpus access for large books → structure analysis → purpose
question (`DEPTH ∈ {reference, study}`) → skill naming → directory creation →
chapter synthesis → supporting files → master file → security scan →
cleanup/report → optional publish. [D2-13]

**R14. Cost transparency before spend.** Step 2.5 mandates a pre-flight
estimate (sources, pages, words, tokens, input/output token estimates, time,
files to be generated) with explicit formulas (input ≈ estimated tokens × 1.3;
output ≈ chapters × budget + fixed supporting-file costs) and a human proceed
gate — while forbidding hardcoded dollar figures. [D2-15]

**R15. Two-axis depth matrix.** Per-chapter token budgets are a function of
`BOOK_TYPE × DEPTH` (800–1,200 up to 2,000–3,000 tokens), stated as targets
not hard caps, with a "use the lower budget when in doubt" rule; `study` depth
is content-gated (must reproduce a worked example) rather than padded to a
number. [D2-18]

**R16. Template skeleton per chapter.** A fixed ordered section contract
(`Core Idea`, `Frameworks Introduced`, `Key Concepts`, `Mental Models`,
`Anti-patterns`, conditional `Code Examples`/`Reference Tables` for technical
books, conditional `Worked Example` for study depth, `Key Takeaways`,
`Connects To`) with per-section gating rules. [D2-19]

**R17. Safety gates in the pipeline:** an advisory prompt-injection scan of
generated artifacts with a stop-and-ask-human rule on non-zero exit; publish
visibility hard-gating (third-party copyrighted sources must stay private);
extraction-side sanitization (zero-width and Unicode tag-block stripping,
DOCX DTD/entity rejection before parsing, absolutized subprocess paths).
[D2-23, D2-28]

**R18. Fold-in workflow.** The artifact is mutable: new sources fold into an
existing skill by parsing the existing chapter/topic indexes, numbering new
chapters after the highest existing one, merging supporting files under raised
caps, and regenerating the master file. [D1-28, D2-24]

---

## Part 4 — Mapping to Vesper primitives

Ground truth from current workspace evidence. Mappings marked **gap** or
**proposal** do not exist today and are not claimed to.

**M1 — Amortized structuring ↔ offline skill authoring.** R9's
compile-time-over-runtime split maps directly onto Vesper's authoring-time
skill construction: `learn_skill` distills procedure into
`.agent-vesper/memory/skills/<slug>.md` (root AGENTS.md; `skills/AGENTS.md`),
and at query time the harness injects only ranked metadata plus bounded inline
instructions for the active request. The structuring cost is paid once at
authoring, exactly the amortization argument in R9. [R9 → D1-16, D2-26]

**M2 — Two-tier representation ↔ existing two-tier skill loading.** Vesper
already implements the R5 split natively:
`crates/vesper-memory/src/skill_orchestrator.rs` ranks only bounded metadata
(description-token overlap via `semantic_tokens` + `hashed_cosine`,
skill_orchestrator.rs:516–527), composes at most `MAX_SELECTED_SKILLS = 3`
skills (:13), and enforces `MAX_SKILL_CONTEXT_CHARS = 24_000` /
`MAX_TOTAL_SKILL_CONTEXT_CHARS = 60_000` (:15, :17) — the always-loaded tier is
bounded by construction, and bodies are injected only for the active provider
request (root AGENTS.md skill-orchestration contract). [R5 → D1-9/D1-10,
D2-27]

**M3 — Index as page table ↔ skill-level metadata routing (gap: per-chunk
routing metadata).** Vesper's routing tier is skill-level bounded metadata —
`SkillMetadata` already fields `description`, `tags`, `triggers`, `exclusions`
and more, all ranked against prompt tokens (`skill_orchestrator.rs:54-72`,
`:516-527`). Context upstream's optional per-chunk `summary`/`key_elements`
fields (R11) are the missing axis — and notably its own eval plan refuses
to promote them without repeated measured benefit plus reported context
overhead (PD-07, and
the explicit forbidden claim "`KEY_ELEMENTS` improves routing" — used, never
ablated). **Proposal, gated the same way:** any richer routing metadata for
Vesper skills must clear a repeated-benefit bar before adoption; flat
single-level routing remains the default to beat. [R11 → D2-30, D2-31, D2-32,
D2-36]

**M4 — Front-loading against compaction ↔ token-pressure compaction.** R7's
compaction-survival design maps onto
`crates/vesper-agent/src/compaction.rs`, where automatic compaction is
token-pressure driven against the active model's advertised window
(`context_pressure`, compaction.rs:134), preserves system instructions and
complete recent tool transactions, and keeps the human-visible TUI transcript
intact (root AGENTS.md compaction contract). Vesper's analog of "front-load"
is the contractual preservation of system instructions and instructions-bearing
prefix; body order within a skill is author guidance, not a runtime mechanism.
[R7 → D1-13, D1-22, D2-26]

**M5 — Corpus paging during generation ↔ bounded workers.** R10's
REPL-style access to a large corpus maps onto existing Vesper primitives for
keeping bulk material out of the main context: `context: fork` bodies run
inside a bounded worker (root AGENTS.md skill-orchestration contract), and
`delegate_task` investigations and windowed file reads perform the same
probe-then-window discipline. No new primitive required. [R10 → D2-16]

**M6 — Cost transparency ↔ bounded budgets and honest claims.** R14's
pre-flight estimate with hard ceilings and stop-don't-exceed maps onto
Vesper's bounded-budget discipline in the swarm layer (VRO-15 repair scope,
`docs/foundation/vro15-gap-audit.md`) and onto ADR 0028's completion-assurance
contract (`docs/adr/0028-native-implementation-acceptance.md`): both systems
refuse to spend silently and refuse to claim without evidence. [R14 → D2-15]

**M7 — Eval discipline ↔ Vesper's verification culture.** The upstream
research plan's causal-axis decomposition (change one axis, hold the rest,
record confounds rather than claiming isolation — R-invoked D2-29), run
manifests with hashed inputs and declared ceilings (D2-33), a four-bucket
metric taxonomy (D2-34), an explicit verdict enum with INSUFFICIENT EVIDENCE
(D2-36), and forbidden-claims lists are directly consumable design input for
**VRO-16 governance evaluation design** — the sibling report
`docs/architecture/recon_vro16_governance.md` notes the F01–F03 repairs are
prerequisites for any governance layer, and this eval scaffolding is how gate
effects could be measured honestly once those land. No Vesper primitive is
claimed; this is reusable methodology. [D2-29, D2-33, D2-34, D2-36]

**M8 — Document-ingestion security ↔ vesper-security (gap).** R17's
extraction sanitization (zero-width/tag-block stripping, XXE guards,
absolutized subprocess paths) would matter to Vesper only if a
document-to-skill ingestion feature ever existed. `crates/vesper-security`
exists today, but no document-ingestion surface does. **Gap, explicitly not
planned.** [R17 → D2-23, D2-28]

---

## Part 5 — Gaps and open items

- **G1 (preserved, not smoothed):** the 24×–51× headline (R12) and all
  benchmark tables are upstream claims whose methodology file was not read in
  this mission; re-verification requires a bounded read of
  `docs/performance.md` at the pinned commit.
- **G2 (upstream-internal limitation, factual):** chapter auto-detection
  requires explicit `Chapter N`-style headings; two of four showcase
  conversions could not auto-segment. [D1-20]
- **G3 (Vesper gap):** no per-chapter file granularity exists inside a Vesper
  skill today — a skill is one body plus optional resource directories
  (`vesper-skill-authoring` skill). If a knowledge-dense skill class ever
  needs paging, M3's gated proposal is the entry point, not a commitment.

## Trace verification

| Report claim | Traces to | Workspace anchor verified |
|---|---|---|
| R1–R4 | D1-3, D1-5, D1-26; D2-1, D2-11, D2-12, D2-13, D2-20, D2-21, D2-25, D2-26 | — (upstream docs) |
| R5–R9 | D1-9..D1-16, D1-22; D2-21, D2-26, D2-27 | — (upstream docs) |
| R10 | D2-16 (SKILL.md:250 verified verbatim) | — |
| R11 | D2-30, D2-38, D2-39 | — |
| R12 | D1-17, D1-24 (gap preserved) | — |
| R13–R18 | D2-13..D2-24, D2-28 | — |
| M1 | D1-16, D2-26 | `skills/AGENTS.md` exists |
| M2 | D1-9, D1-10, D2-27 | `crates/vesper-memory/src/skill_orchestrator.rs` constants at :13/:15/:17/:516/:527 |
| M3 | D2-30, D2-31, D2-32, D2-36 | same file, metadata-only ranking |
| M4 | D1-13, D1-22, D2-26 | `crates/vesper-agent/src/compaction.rs` `context_pressure` :134 |
| M5 | D2-16 | root AGENTS.md `context: fork` contract |
| M6 | D2-15 | `docs/adr/0028-native-implementation-acceptance.md`, `docs/foundation/vro15-gap-audit.md` exist |
| M7 | D2-29, D2-33, D2-34, D2-36 | `docs/architecture/recon_vro16_governance.md` exists |
| M8 | D2-23, D2-28 | `crates/vesper-security/` exists |

Post-write checks: `cargo xtask naming-guard` zero baseline erosion; scratch
clone absent; `git status` shows only this new file.
