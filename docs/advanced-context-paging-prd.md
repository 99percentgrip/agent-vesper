# PRD — Advanced Context Paging (Bounded Skill Chunks & Rich Routing Metadata)

Status: **Complete — all five PRs implemented and locally verified; D3
verdict ADOPT recorded** (2026-09-12). Implementation evidence per PR in
`docs/foundation/context-paging-pr{1,2,3,5}-execution.md` and
`docs/foundation/context-paging-pr4-eval.md`.
Target release: TBD (post-VRO-16 line; no release commitment)
Owner: `vesper-memory` (skill store + orchestrator) + `vesper-agent`
(composition/injection)
Related: `docs/architecture/recon_context_paging.md` (recon evidence source),
ADR 0024 (provider-neutral skill orchestration), ADR 0023 (token-aware
compaction), ADR 0028 (native implementation acceptance)

Upstream provenance: all mechanism definitions below derive from the
reconnaissance of **context upstream** (`docs/architecture/recon_context_paging.md`,
pinned commit and MIT license recorded there). No upstream code, text, or
branding enters Vesper; the alias rule holds throughout.

## 1. Problem

Vesper skills are single-file documents. A skill is one markdown file
(`crates/vesper-memory/src/skills.rs:5-8`: "one markdown file per skill") with
a 200 KB body ceiling (`MAX_SKILL_BYTES`, skills.rs:21), and when the
orchestrator selects a skill it injects a bounded slice of that one body
(`MAX_SKILL_CONTEXT_CHARS = 24_000` per skill, 60_000 total,
`skill_orchestrator.rs:15,17`). There is no granularity between "whole skill
body" and "one `##` heading window" (`read_section`, skills.rs:195-210).

For knowledge-dense skills this forces a bad trade: either the body is kept
small (knowledge lives outside the skill) or every activation pays for
knowledge the current query does not need. The context upstream recon
(`docs/architecture/recon_context_paging.md`, Part 2) documents the
alternative — a two-tier representation in which a small always-loaded index
routes queries to on-demand chunk files, so per-query cost is proportional to
the question, not the corpus — and Vesper already implements the load-bearing
half of that pattern natively (bounded metadata-only ranking, M2), while
lacking the chunk tier entirely (recon G3).

### Current-state evidence

| # | Site | Fact |
|---|------|------|
| E1 | `crates/vesper-memory/src/skills.rs:5-8,21` | One markdown file per skill; `MAX_SKILL_BYTES = 200_000`; no `chunks/` convention exists anywhere in `vesper-memory` or `skills/AGENTS.md`. |
| E2 | `crates/vesper-memory/src/skill_orchestrator.rs:54-72` | `SkillMetadata` is already multi-field (`description`, `tags`, `triggers`, `exclusions`, `file_extensions`, `platforms`, …). Skill-level routing is richer than a single description; what does **not** exist is per-chunk routing metadata. |
| E3 | `crates/vesper-memory/src/skills.rs:195-210` | `read_section` reads one `##` heading window from a single body — section reading, not file-granularity paging of separate chunk files. |
| E4 | `crates/vesper-memory/src/skill_orchestrator.rs:500-535` | Ranking scores `description + tags + name + triggers` against prompt tokens (`semantic_tokens`, `hashed_cosine`). Chunk-level terms have no representation today. |
| E5 | `crates/vesper-memory/src/skill_orchestrator.rs:13-17` | Composition ceilings: `MAX_SELECTED_SKILLS = 3`, 24 K chars per skill, 60 K total. Any chunk injection must land inside these budgets, not beside them. |
| E6 | ADR 0023 | "Front-loading against compaction" is not a runtime mechanism in Vesper: compaction preserves system instructions and complete recent tool transactions by contract. Skill body ordering is author guidance only. |
| E7 | ADR 0024 | Skill orchestration is provider-neutral and shared by TUI/ACP direct, VRO, and ReAct paths. Any chunk feature must land in the shared crate and reach both hosts through it — no host-specific duplication. |
| E8 | `docs/architecture/recon_context_paging.md` M3/G3 | Recon verdict: Vesper implements the bounded always-loaded tier already; chunk granularity and richer routing metadata are the gaps, and routing-metadata enrichment carries an explicit eval gate before adoption. |

## 2. Goals

1. **G1 — Two-tier skill representation.** A skill consists of a bounded
   always-loaded tier (frontmatter + routing metadata + primary body slice,
   exactly as today) plus an optional on-demand chunk tier. When chunks exist,
   activation cost tracks the chunks a query needs, not the whole skill body
   (recon R5).
2. **G2 — Bounded file-granularity chunks.** Chunks are separate files under
   the skill's resource directory, enumerated by a manifest, capped in count
   and size, loaded only when routed to, and counted against the existing
   E5 budgets (recon G3 → D1).
3. **G3 — Rich per-chunk routing metadata.** Each manifest entry carries
   `description` (required), `summary` (optional), `key_elements` (optional
   list), mirroring the schema shape documented in recon R11. Additive and
   default-off for automatic routing until the D3 gate clears (→ D2).
4. **G4 — Strict evaluation gating.** No change to default automatic routing
   from chunk metadata until a repeated-benefit evaluation, run offline on
   deterministic fixtures, shows improvement on more than one corpus/task
   family **with the metadata overhead reported** (→ D3; adapted from recon
   M3/D2-31, D2-36).
5. **G5 — Zero regression without chunks.** Skills without a chunk manifest
   parse, rank, compose, and inject byte-identically to today. The chunk tier
   is invisible to every existing skill and test.
6. **G6 — Cross-host parity by construction.** Chunks land in `vesper-memory`
   routing and `vesper-agent` composition only; both hosts inherit the
   behavior through the shared path (ADR 0024 precedent; root contract on
   host parity).

## 3. Non-goals

- **Document-ingestion security (recon M8) — rejected.** Vesper is an
  execution engine, not a PDF converter. No extraction sanitization, DOCX
  XXE guards, subprocess path hardening, or any document→skill conversion
  pipeline. The upstream's security layers solve a supply chain Vesper does
  not open. Existing hosted-tool security (`vesper-security`, VRO-13
  firewall/sandbox) is untouched and out of scope here.
- **No document→skill conversion.** Chunk authoring is a skill-authoring
  concern (human or agent via `learn_skill`), not an ingestion pipeline.
- **No RAG/embedding store.** Routing stays bounded-metadata token matching
  over the manifest (root AGENTS.md: "ranks only bounded metadata"). Vector
  retrieval is a different initiative with its own evidence bar.
- **No compaction changes.** ADR 0023 stays authoritative. Chunk bodies are
  turn-transient injections, not persisted conversation state.
- **No budget-ceiling changes.** `MAX_SELECTED_SKILLS`, 24 K/60 K ceilings,
   and file caps move only with their own evidence, not as a rider here.
- **No multi-level chunk hierarchy.** One routing level (manifest → chunk),
   per recon M3/D2-32: flat is the default to beat; nested routing is an
   experiment Vesper has no evidence to run.

## 4. Architectural decisions (from recon)

### D1 — Bounded chapter granularity (recon G3, R5, R6)

Extend the skill structure with on-demand chunk files instead of only a
single body file.

- **Layout:** `<skill-root>/<slug>/chunks/<name>.md` — chunk files live in a
  resource directory beside the skill document, consistent with the existing
  `references/`-style resource convention (`vesper-skill-authoring` skill).
- **Manifest:** the skill document's frontmatter declares a `chunks:` list;
  each entry is `{ name, description, summary?, key_elements? }`. A skill
  with no `chunks:` key has no chunk tier (G5).
- **Bounds:** new constants in `vesper-memory` — `MAX_CHUNKS_PER_SKILL` and
  `MAX_CHUNK_BYTES` (proposed 32 and 24_000; see §9 open questions) — enforced
  at parse time, fail-closed: an over-limit chunk manifest is a routing
  rejection reason, not a silent truncation.
- **Rationale:** recon R5/R6 — the always-loaded index (manifest) is the page
  table; chunks are loaded only when a query routes to them.

### D2 — Rich routing metadata (recon M3, R11)

Extend the routing metadata schema with per-chunk `summary` and
`key_elements` for precision routing.

- **Schema** follows recon R11 exactly: `description` required per chunk,
  `summary` optional, `key_elements` optional list of strings. Fields absent
  from a manifest entry parse as absent — never guessed.
- **Ranking** extends E4's existing arithmetic: chunk terms join the prompt
  token comparison (`semantic_tokens` over `description + summary +
  key_elements`) once chunk routing is active. No new ranking machinery.
- **Honest default → ADOPT (2026-09-12):** `summary`/`key_elements` were
  parsed from day one but routing-neutral until the D3 gate. The PR-4 eval
  (`docs/foundation/context-paging-pr4-eval.md`) returned **ADOPT** —
  improvement repeated across both families with measured overhead — so
  `CHUNK_METADATA_ROUTING_ENABLED = true` now ships; automatic chunk
  routing feeds on `description` + `summary` + `key_elements`. A future
  superseding REJECT re-eval flips it back.

### D3 — Strict eval gating (recon M3 proposal, D2-31/D2-32/D2-36)

The new routing metadata must clear a repeated-benefit evaluation bar before
it replaces the current flat description routing.

- **Conditions:** `FLAT_DESCRIPTION` (status quo) vs `SUMMARY_ONLY` vs
  `SUMMARY_KEY_ELEMENTS`, one axis varied, all else held fixed (upstream's
  causal-axis discipline, recon D2-29).
- **Metrics:** routing success (target chunk selected; irrelevant chunks
  loaded before target) and metadata token overhead, reported together —
  never routing success alone (recon D2-31).
- **Decision rule:** adopt only if improvement repeats on **more than one**
  corpus/task family; verdicts are `ADOPT` / `KEEP EXPERIMENTAL` / `REJECT` /
  `INSUFFICIENT EVIDENCE` (recon D2-36).
- **Forbidden claims** (adapted): "key_elements improves routing" without
  the ablation; "flat always beats rich" without the counterfactual; any
  token-reduction multiple extrapolated from upstream measurements (recon
  R12 preserved the 24×–51× figure as an **unverified upstream claim**; this
  PRD claims nothing from it).
- **Verdict location:** the eval report lands in
  `docs/foundation/` per evidence-index convention, and
  `docs/migration-status.md` flips only on the recorded verdict.

## 5. Implementation plan

Phases are ordered storage → routing → composition → gate → docs. PR-2
ships chunk routing **behind a default-off automatic path**; the D3 gate in
PR-4 decides whether metadata-based chunk routing becomes the default.

### PR-1 — Chunk storage and manifest (`vesper-memory`) — **IMPLEMENTED**

- `SkillStore` enumerates `chunks/` directories and validates manifests at
  catalog build; `parse_metadata` (skill_orchestrator.rs:559) gains the
  `chunks:` list. Caps from D1 enforced fail-closed.
- Files: `crates/vesper-memory/src/skills.rs` (enumeration),
  `crates/vesper-memory/src/skill_orchestrator.rs` (manifest parse),
  `crates/vesper-memory/src/types.rs` (chunk types).
- Tests: new `crates/vesper-memory/tests/chunk_store.rs` — manifest parse,
  cap enforcement, absent-manifest no-op (G5), malformed-entry rejection.
- **Evidence (2026-09-12):** 12 exact tests in
  `crates/vesper-memory/tests/chunk_store.rs` covering AC-1: manifest
  parse with all fields, enumerate+select parity, global-layer fallback,
  oversize-file rejection (both at validation and read time), missing-file
  rejection, count-cap rejection, malformed/duplicate rejection, G5
  byte-identity in catalog and routing, cap constants. Workspace
  `cargo test --workspace --all-features` 2,197 passed / 0 failed
  (floor 2,185+ holds; +12 new), `cargo xtask verify` green,
  `cargo xtask acceptance` 20/20, `cargo xtask architecture` 27 packages,
  `cargo xtask naming-guard` clean (18 frozen hits, zero erosion).
  Full execution record with per-gate receipts, AC traceability, and
  deviations: `docs/foundation/context-paging-pr1-execution.md`.

### PR-2 — Two-level routing (`vesper-memory`) — **IMPLEMENTED**

- After skill selection, a bounded second pass ranks chunk manifest entries
  for each selected skill and loads at most `MAX_CHUNKS_PER_SELECTION`
  (**3**, §9 Q2 resolved) chunk bodies inside the existing E5 budgets.
- Automatic chunk-metadata routing (`summary`/`key_elements` participation)
  is feature-flagged **off** via `CHUNK_METADATA_ROUTING_ENABLED = false`;
  description-only chunk routing runs because it adds no unevaluated
  metadata axis (D2 honest default). Flipping the constant is a D3-gated
  change.
- Chunk ranking reuses the skill-level arithmetic (`semantic_tokens`
  overlap ×520, `hashed_cosine` ×2,200, name-match +3,500); zero-overlap
  chunks never auto-load, keeping the tier proportional to the query.
- Budget model: chunks draw from the per-skill pool (`24K − body`) and the
  shared 60K total; an over-budget or unreadable chunk is **skipped** with
  an explicit `skill::chunk` rejection reason — never truncated. Isolated
  (`context: fork`) skills never load chunks (main context stays
  identity-only).
- **Evidence (2026-09-12):** 8 tests in
  `crates/vesper-memory/tests/skill_routing.rs` covering all four
  PRD-mandated categories — selection+budget math (ranked order, ≤3 cap,
  per-skill/total sums), a distinguishing budget-boundary case (chunk
  under the 24K byte cap but over the remaining per-skill allowance →
  rejected *by budget*, body untruncated), fail-closed oversize (manifest
  layer + read-time defense-in-depth), flag-off neutrality (summary-only
  prompt overlap loads nothing; description/name routing unaffected), and
  G5 routing-phase byte-identity (same selection/body/score; no chunk
  rejections; `context()` envelope unchanged, no chunk markers). Workspace
  `cargo test --workspace --all-features` 2,202 passed / 0 failed (floor
  2,185+ holds), `cargo xtask verify` 183/183 suites, `cargo xtask
  acceptance` 20/20, clippy `-D warnings` clean, fmt clean,
  `cargo xtask architecture` 27 packages, `cargo xtask naming-guard`
  clean. Full record:
  `docs/foundation/context-paging-pr2-execution.md`.

### PR-3 — Composition and cross-host injection (`vesper-agent`) — **IMPLEMENTED**

- `LoadedSkill` gains selected chunk bodies; inline-instruction composition
  (active provider request only, transient, never persisted — root contract)
  emits the primary slice followed by routed chunks, each wrapped in a named
  `<agent-vesper-skill-chunk skill name>` block for provenance.
- Parity by construction (E7): the envelope is host-appended content through
  the existing `orchestrate_skills` → `context()` seam; zero TUI/ACP-specific
  routing code. Direct, VRO, and ReAct dispatch all consume the same
  host-composed history, so every path carries the chunks.
- **Architecture note:** composition tests live in
  `crates/vesper-harness/tests/context_paging_composition.rs` (not
  `vesper-agent/tests/`) because `vesper-agent` is deliberately
  skill-unaware — `cargo xtask architecture` correctly rejects a
  `vesper-agent → vesper-memory` edge in any position. The harness is the
  composition boundary that legitimately depends on both and already hosts
  this test pattern; the suite still drives the real `AgentLoop` end-to-end
  through `FakeProviderSession`.
- **Evidence (2026-09-12):** 6 tests in
  `crates/vesper-harness/tests/context_paging_composition.rs`: composition
  (primary slice precedes chunk blocks; provenance attributes; envelope
  closes), transience (envelope reaches the dispatched request; restored
  history is byte-identical to pre-turn original with zero chunk/skill
  bodies), budget adherence (per-skill/total sums held; routed bodies verbatim
  in envelope), no-regression (chunk-less envelope byte-identical, zero chunk
  markers), and shared-path proofs (direct dispatch carries the envelope at
  the provider request; two dispatch shapes through the same seam both carry
  it — the VRO/ReAct parity obligation). Workspace
  `cargo test --workspace --all-features` 2,208 passed / 0 failed (floor
  2,185+ holds), `cargo xtask acceptance` 20/20, clippy `-D warnings`
  clean, fmt clean, `cargo xtask architecture` 27 packages (the initial
  dev-dep placement was caught by this gate and corrected to the harness),
  `cargo xtask naming-guard` clean. Full record:
  `docs/foundation/context-paging-pr3-execution.md`.

### PR-4 — Deterministic eval harness and the D3 gate — **IMPLEMENTED; VERDICT ADOPT**

- Offline fixture corpus (2 task families × 2 probes each: a
  description-sufficient control and a metadata-discriminating probe) with
  deterministic expected targets; strict ablation via
  `ChunkRoutingCondition` (`FlatDescription` / `SummaryOnly` /
  `SummaryKeyElements`) as the only varied axis; runs entirely offline
  against the router (no provider, network, or process I/O; the
  provider-seam leg is the PR-3 composition suite). Harness:
  `crates/vesper-memory/tests/chunk_routing_eval.rs` (self-tests:
  determinism, condition isolation, metric invariants, canonical table,
  verdict/flag consistency); public seam
  `SkillStore::orchestrate_with_condition` +
  `chunk_routing_metrics`.
- **Verdict: ADOPT** — improvement repeated across BOTH families with
  distinct marginal-value carriers (factual probe resolvable only via
  `summary`; procedure probe only via `key_elements`), measured overhead
  SUMMARY_ONLY 13–14 / SUMMARY_KEY_ELEMENTS 16 tokens per skill
  (audit-corrected metric), zero regression on description-sufficient
  controls. Full measured table, leakage analysis, forbidden-claims check,
  and limitations: `docs/foundation/context-paging-pr4-eval.md`.
- **Flag flipped in the same change** per directive 3:
  `CHUNK_METADATA_ROUTING_ENABLED = true`, citing that report. The PR-2
  flag-off proof was superseded by a flag-state proof asserting whichever
  state ships (`metadata_fields_ranking_follows_the_shipped_flag_state`),
  and the harness verdict test asserts flag/verdict equality in both
  directions.
- **Material harness finding (recorded, not fixed in prod):** the
  production `SEMANTIC_ALIASES` table (`deploy → release` …) can bridge an
  activation marker's slug token into an unrelated chunk's routing text,
  outvoting honest key-elements matches (520-pt overlap per aliased token).
  The corpus was hardened against it; any production remedy is future work
  with its own evidence.
- **Evidence (2026-09-12):** workspace `cargo test --workspace
  --all-features` 2,213 passed / 0 failed (floor 2,185+ holds), `cargo
  xtask acceptance` 20/20, clippy `-D warnings` clean, fmt clean,
  `cargo xtask architecture` 27 packages, `cargo xtask naming-guard`
  clean.

### PR-5 — Documentation and authoring surface — **IMPLEMENTED**

- `skills/AGENTS.md` and the global `vesper-skill-authoring` skill (seed
  copy + mirrored to `~/.agent-vesper/memory/skills/`, version 1.1.0) now
  own chunk authoring guidance: manifest format, caps, fail-closed
  semantics, post-ADOPT routing truth, the when-chunks-beat-one-body
  heuristic, and the vocabulary-competition pitfall.
- `docs/using-vesper.md`: evaluated per AC-5; **no user-visible surface
  changed** (zero `apps/` modifications across the initiative — no new
  commands, settings, or panels; chunk authoring is an author-facing
  surface owned by the authoring skill). No note added, per the PRD's own
  "if any" wording.
- `docs/migration-status.md` status moved to its verdict-recorded end
  state: COMPLETE — all five PRs landed.
- **Evidence (2026-09-12):** seed/global mirror byte-identical
  (`diff` clean); `cargo xtask acceptance` 20/20; clippy `-D warnings`
  clean; `cargo xtask naming-guard` clean (18 frozen hits, zero erosion);
  workspace floor 2,213 passed / 0 failed intact. Execution record:
  `docs/foundation/context-paging-pr5-execution.md`.

## 6. Acceptance criteria

Per ADR 0028 (`docs/foundation/completion-assurance-execution.md`): every
criterion below must trace to a current, scope-appropriate artifact at claim
time; passing unrelated tests or plan checkmarks is not evidence.

- **AC-1 (PR-1):** fixture skills with chunk manifests enumerate and parse;
  cap violations are rejections with reasons; chunk-less skills are
  byte-identical in catalog and routing reports (G5 proof).
- **AC-2 (PR-2):** two-level routing selects bounded chunks inside E5
  budgets; `summary`/`key_elements` provably do not affect automatic routing
  while the flag is off (test asserts flag-off ranking equals status quo).
- **AC-3 (PR-3):** composition injects primary slice + routed chunks
  transiently; persisted session artifacts contain no chunk bodies; both
  hosts exhibit the behavior through the shared path with no host-specific
  routing code.
- **AC-4 (PR-4):** eval report records conditions, per-family results,
  metadata overhead, and a verdict from the D3 enum; any default flip cites
  it. INSUFFICIENT EVIDENCE is an acceptable, non-blocking verdict — the
  flag simply stays off.
- **AC-5 (PR-5):** docs and authoring guidance match shipped behavior;
  `docs/migration-status.md` reflects the true state.
- **AC-6 (always):** `cargo xtask acceptance` green; `cargo xtask
  naming-guard` zero baseline erosion; no live provider calls in
  verification; workspace architecture and dependency direction unchanged.

## 7. Evidence standards and constraints

- Naming embargo: the alias **context upstream** is the only permitted
  reference; `cargo xtask naming-guard` must show zero new hits for the
  lifetime of this initiative.
- No invented behavior: this PRD claims no token-reduction numbers; any
  future claim requires Vesper's own measurements (upstream figures stay
  quarantined as recon R12).
- Verification is offline and deterministic; `vesper-provider-synthetic`
  covers end-to-end paths.
- Production crates never depend on `vesper-testkit`, frontend crates, or
  `spikes/` (root contract).

## 8. Risks and mitigations

- **Budget squeeze:** chunks inside 24 K/60 K could crowd out primary bodies.
  Mitigation: per-selection chunk cap (PR-2) plus budget-fail-closed tests;
  ceilings do not move without evidence.
- **Metadata bloat:** rich manifests grow the always-loaded catalog prefix.
  Mitigation: metadata token overhead is a first-class PR-4 metric; adoption
  requires it reported, and the manifest sits in the bounded catalog prefix
  (`MAX_SKILL_CATALOG_PREFIX_BYTES`, skills.rs:27).
- **Two-level ranking complexity:** a second ranking pass adds failure
  modes. Mitigation: chunk routing reuses E4's arithmetic unchanged; no new
  scoring machinery.
- **Scope creep toward ingestion:** the M8 rejection is a hard boundary
  (§3); any proposal to ingest documents reopens a different PRD.

## 9. Open questions

1. Manifest placement: frontmatter `chunks:` list vs a sidecar
   `chunks.md`/JSON. Frontmatter preferred (single source, parsed where
   metadata already is); sidecar only if prefix budget forces it.
2. Defaults for `MAX_CHUNKS_PER_SKILL` / `MAX_CHUNK_BYTES` /
   `MAX_CHUNKS_PER_SELECTION` (proposed 32 / 24_000 / 3) — confirm against
   real seed-library skill shapes in PR-1.
   **Resolved (PR-1):** confirmed against the 326-file seed library —
   max 9 `##` sections per skill, zero `chunks:` usage, so 32 chunks is
   3.5× headroom; `MAX_CHUNK_BYTES = 24_000` matches
   `MAX_SKILL_CONTEXT_CHARS` (a larger chunk could never be injected
   whole). `MAX_CHUNKS_PER_SELECTION` remains open until PR-2 routing.
   **Fully resolved:** PR-2 set `MAX_CHUNKS_PER_SELECTION = 3` (aligns with
   the three-skill selection ceiling; chunk bodies additionally bounded by
   per-skill/total budgets, so the count cap is a secondary guard).
3. Whether `read_section` (E3) stays as-is or gains a deprecation path once
   chunks exist; no change proposed now (G5).
4. Whether the D3 eval harness lives as crate tests or an `xtask` command —
   decided in PR-4 by the evidence-index convention at that time.
   **Resolved (PR-4):** crate integration tests in
   `crates/vesper-memory/tests/chunk_routing_eval.rs` — deterministic,
   offline, self-testing, and emitting the canonical results table under
   `-- --nocapture`; no xtask command needed for the current corpus scale.
