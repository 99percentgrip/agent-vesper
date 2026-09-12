# Advanced Context Paging — PR-2 Execution Report (Two-Level Routing)

Status: **COMPLETE — locally verified** (2026-09-12)
Scope: `docs/advanced-context-paging-prd.md` §5 PR-2, §6 AC-2 only
Owner: `vesper-memory`
Directive correction honored: the dispatch explicitly included the
PRD-mandated G5 routing-phase test in `skill_routing.rs` (the requirement
the prior directive missed).

## 1. Objective

Implement two-level routing: after skill selection, a bounded second pass
ranks chunk manifest entries and loads at most `MAX_CHUNKS_PER_SELECTION`
bodies inside the existing per-skill/total budgets; `summary`/
`key_elements` stay ranking-neutral behind a feature flag (D2); chunk-less
skills route byte-identically (G5); fail-closed everywhere (no truncation).

## 2. Methods and command record

| Gate | Command | Result (receipt) |
|---|---|---|
| New-test suite | `cargo test -p vesper-memory --test skill_routing` | `test result: ok. 8 passed; 0 failed` |
| Full crate suite | `cargo test -p vesper-memory` | `ok. 43 passed` / `ok. 12 passed` / `ok. 8 passed` / doc-tests `0` |
| Lint | `cargo clippy -p vesper-memory --all-targets --all-features -- -D warnings` | clean (`Finished … 0.25s`) |
| Format | `cargo fmt -p vesper-memory` (+ check) | clean |
| Workspace tests | `cargo test --workspace --all-features` | `TOTAL: 2202 passed, 0 failed` |
| Acceptance | `cargo xtask acceptance` | `20 exact cases passed in 5627 ms` |
| Architecture | `cargo xtask architecture` | `27 packages` |
| Naming embargo | `cargo xtask naming-guard` | `clean (18 hits, all frozen in baseline)` |
| Canonical verify | `cargo xtask verify` | 183/183 `test result: ok` lines, 0 FAILED |

Test floor: PR-1 closed at 2,197. PR-2 closes at **2,202** (net +5: three
new routing tests + budget-boundary + strengthened flag-off; one prior
assertion superseded by the distinguishing no-load proof).

## 3. Inspected / created / modified files

Modified: `crates/vesper-memory/src/skill_orchestrator.rs`,
`src/lib.rs`, `crates/vesper-memory/AGENTS.md`,
`crates/vesper-memory/tests/skill_routing.rs`,
`docs/advanced-context-paging-prd.md`, `docs/migration-status.md`.
Created: `docs/foundation/context-paging-pr2-execution.md` (this report).
Not touched: every other crate; `Cargo.toml` unchanged (no new deps).

## 4. Implementation summary

- **`MAX_CHUNKS_PER_SELECTION = 3`** — §9 Q2 fully resolved (aligns with
  the three-skill selection ceiling; budget pools remain primary).
- **`CHUNK_METADATA_ROUTING_ENABLED = false`** — compile-time honest
  default; flag-off ranking feeds on `description` only. The
  summary/key_elements feeding code exists in `rank_chunks` so a future
  D3-gated flip changes exactly the routing-text assembly, nothing else.
- **`LoadedChunk { skill, name, body }` + `SkillRoutingReport.chunks`** —
  routing-tier payload; emission into the model envelope is PR-3 scope.
- **`rank_chunks`** — reuses skill-level arithmetic unchanged
  (`semantic_tokens` overlap ×520 capped at 8, `hashed_cosine` ×2,200,
  name-match +3,500); zero-overlap entries never qualify, so the tier
  stays proportional to the query.
- **Second pass in `orchestrate()`** — for each selected inline skill with
  a valid manifest: rank → take(3) → per-chunk allowance =
  `min(per-skill remaining, total remaining)`; over-budget → skip with
  `chunk exceeds context budget`; unreadable/oversize → skip with
  `chunk unreadable or over byte cap`; loaded bodies deduct from both
  pools. Isolated (`context: fork`) skills never load chunks.

## 5. AC-2 traceability

| AC-2 clause | Test (skill_routing.rs) | Asserted evidence |
|---|---|---|
| two-level routing selects bounded chunks | `two_level_routing_selects_ranked_chunks_within_budgets` | prompt-overlapping chunks load; ranked order puts `rollback` first; ≤3 loaded; per-skill body+chunk sum ≤24K and ≤60K |
| chunks count against per-skill/total | `chunk_over_per_skill_budget_is_rejected_by_budget_not_byte_cap` | chunk **under** the 24K byte cap but **over** the remaining per-skill allowance → rejected with exactly `chunk exceeds context budget`; skill body still loaded untruncated — the distinguishing proof that budgets bind chunk loads |
| fail-closed oversized | `oversized_chunk_is_rejected_not_truncated` | oversize file → manifest-level rejection (`invalid chunk manifest: …`) at catalog time; no chunks load; `read_chunk` byte cap is defense-in-depth |
| flag-off: metadata provably does not affect routing | `metadata_fields_are_ranking_neutral_while_flag_off` | two skills with identical descriptions, divergent `summary`/`key_elements`; a prompt overlapping ONLY the summary terms loads nothing extra and never reorders — flag-off ⇒ description/name-driven ranking only |
| G5: chunk-less skills byte-identical (routing phase) | `chunk_less_skills_route_byte_identically_at_routing_phase` | identical selection, exact body bytes, non-zero score preserved; `report.chunks` empty; zero chunk-related rejections; `context()` envelope contains no chunk markers |

## 6. Deviations from the directive

1. **Compile-time flag, not a config surface.** The directive said
   "feature-flagged off." Implemented as a `pub const` in
   `vesper-memory` rather than a runtime/config toggle, because the PRD
   reserves activation changes for the D3 gate and a runtime toggle would
   pre-empt that evaluation (and contradict the project rule that feature
   activation belongs in native Settings). The flag is public and its
   doc comment marks flipping it a D3-gated change.
2. **Test-count delta vs the directive's floor wording.** Directive says
   "ensure the 2,197+ test floor holds"; actual close is 2,202 (+5). No
   test was removed; the PR-1 `existing_workspace_tests_unchanged`
   regression proxy in `chunk_store.rs` remains, and one weaker flag-off
   assertion was superseded by a stricter one in the same file.
3. **`report.context()` emission deliberately unchanged.** The directive
   listed no envelope change and the PRD assigns composition to PR-3;
   `LoadedChunk` is exposed on the report but not yet rendered into the
   model-facing envelope. Flagged so PR-3 doesn't look like a gap.

## 7. Unresolved items (honest scope)

- **PR-3:** composition/injection of `report.chunks` into the model
  envelope (`context()`), cross-host wiring, transience proofs.
- **D3 gate unchanged:** `CHUNK_METADATA_ROUTING_ENABLED` stays `false`
  until the PR-4 eval verdict; the summary/key_elements feed path is
  present but dormant.
- **CI:** local evidence only; canonical/MSRV/five-target workflows not
  run for this tree.
- **Seed library:** still 0/326 skills using `chunks:`; routing tier
  exercised by fixtures only (authoring guidance is PR-5).

## 8. Readiness effect and status

PR-2 complete and locally verified; AC-2 satisfied with traceable
evidence; all three constraints (D2 honest default, E5 budgets, G5
byte-identity) hold with distinguishing proofs. PRD updated (PR-2
IMPLEMENTED, §9 Q2 fully resolved), migration-status updated
(IMPLEMENTING — PR-1+PR-2), crate AGENTS.md contract extended. Next: PR-3
— composition and cross-host injection in `vesper-agent`.

Per ADR 0028: every claim above traces to the commands and tests in §2
and §5, runnable against the current tree.
