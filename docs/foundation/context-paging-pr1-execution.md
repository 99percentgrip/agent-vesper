# Advanced Context Paging — PR-1 Execution Report (Storage & Manifest)

Status: **COMPLETE — locally verified** (2026-09-12)
Scope: `docs/advanced-context-paging-prd.md` §5 PR-1, §6 AC-1 only
Owner: `vesper-memory`
Verification: all gates re-ran green on 2026-09-12 at the post-DOX-closeout
tree state; receipts quoted verbatim below. No CI or release execution —
remote gates remain pending, as with every locally-verified PR in this
workspace.

## 1. Objective

Implement chunk storage enumeration, manifest parsing, and fail-closed
capacity enforcement in `vesper-memory` (PRD D1 storage layer + D2 schema
parse), with the G5 zero-regression guarantee for chunk-less skills. No
routing behavior change (PR-2), no composition change (PR-3), no eval
harness (PR-4).

## 2. Methods and command record

| Gate | Command | Result (receipt) |
|---|---|---|
| Crate build | `cargo build -p vesper-memory` | `Finished dev profile … in 1.32s` (clean after iteration) |
| New-test suite | `cargo test -p vesper-memory --test chunk_store` | `test result: ok. 12 passed; 0 failed` |
| Full crate suite | `cargo test -p vesper-memory` | `ok. 43 passed` / `ok. 12 passed` / `ok. 3 passed` / `ok. 0 passed` (doc-tests) |
| Lint | `cargo clippy -p vesper-memory --all-targets --all-features -- -D warnings` | `Finished … 0.23s`, no diagnostics |
| Format | `cargo fmt --check -p vesper-memory` | clean |
| Workspace tests | `cargo test --workspace --all-features` | summed `TOTAL PASSED: 2197`, zero failures |
| Canonical verify | `cargo xtask verify` | 183/183 `test result: ok` lines, no FAILED, acceptance line `20 exact cases passed in 17977 ms` |
| Acceptance gate | `cargo xtask acceptance` | `Acceptance regression gate: 20 exact cases passed in 5685 ms` |
| Architecture | `cargo xtask architecture` | `architecture boundaries validated for 27 packages` |
| Naming embargo | `cargo xtask naming-guard` | `naming-guard: clean (18 hits, all frozen in baseline)` |

Test-floor contract: baseline 2,185+ (VRO-16 closeout, migration-status row)
→ 2,197 passed = floor holds with **+12 new tests** (the AC-1 suite).

## 3. Inspected / created / modified files

Modified (all previously existing):
`crates/vesper-memory/src/types.rs`, `src/skills.rs`,
`src/skill_orchestrator.rs`, `src/lib.rs`, `crates/vesper-memory/AGENTS.md`,
`docs/advanced-context-paging-prd.md`, `docs/migration-status.md`.

Created:
`crates/vesper-memory/tests/chunk_store.rs` (12 tests, ~13 KB).

Not touched: every other crate; `apps/`; `xtask/`; no dependency changes
(`vesper-memory` Cargo.toml unchanged — still only `vesper-domain` +
`vesper-security` + serde/thiserror/serde_json, tempfile dev-only, so the
architecture allowlist never needed re-validation inputs).

## 4. Implementation summary

- **`types.rs`** — `SkillChunkManifestEntry { name, description,
  summary: Option, key_elements: Option<Vec> }` with public `validate()`;
  caps `MAX_CHUNKS_PER_SKILL = 32`, `MAX_CHUNK_BYTES = 24_000`,
  `MAX_CHUNK_DESCRIPTION_CHARS = 240`, `MAX_CHUNK_SUMMARY_CHARS = 480`,
  `MAX_CHUNK_KEY_ELEMENTS = 8`.
- **`skill_orchestrator.rs`** — `SkillMetadata` gains `chunks` +
  `chunk_manifest_error`; nested `chunks:` block is split from the body
  **before** flat frontmatter parsing (the flat parser cannot express
  nesting — an indented `description:` inside the block would be promoted
  to a top-level key and corrupt the skill's own `description` field);
  fail-closed parse (`parse_chunk_entries`) with duplicate-name and
  count-cap rejection; `chunk_manifest_error` feeds `ineligible_reason` as
  `invalid chunk manifest: …` making the skill routing-ineligible.
- **`skills.rs`** — `chunks/` path resolution under
  `<root>/skills/<slug>/chunks/` with global-layer fallback;
  `validate_chunk_manifest` (manifest-vs-disk: existence + byte size);
  `read_chunk` (bounded read; a file that grew past `MAX_CHUNK_BYTES`
  after validation still cannot enter context); `read_bounded` helper.

## 5. AC-1 traceability

PRD AC-1: *"fixture skills with chunk manifests enumerate and parse; cap
violations are rejections with reasons; chunk-less skills are byte-identical
in catalog and routing reports (G5 proof)."*

| AC-1 clause | Test (chunk_store.rs) | Asserted evidence |
|---|---|---|
| manifests enumerate/parse | `valid_manifest_parses_with_all_fields` | 2 entries, name/description/summary/key_elements parsed; absent optionals stay `None`; skill-level `name`/`description`/`tags` uncorrupted by the nested block |
| manifests enumerate/parse | `manifest_skill_enumerates_and_selects_like_any_other` | catalog lists the chunked skill; routing selects it; `read_chunk` returns exact chunk bytes |
| storage fallback | `chunk_read_falls_back_to_global_layer` | global-root chunk read via `open_with_global` |
| cap violations → rejections | `oversize_chunk_file_is_rejected_not_truncated` | routing rejection with the exact reason string; `read_chunk` also refuses the oversize file |
| cap violations → rejections | `missing_chunk_file_is_rejected` | declared-but-absent chunk → `file not found under chunks/` |
| cap violations → rejections | `over_limit_chunk_count_is_rejected` | 33 declared chunks → `chunk_manifest_error` set, routing rejects |
| cap violations → rejections | `malformed_entries_reject_with_explicit_reasons` | invalid names → reason contains `chunk name`; all declared entries retained for diagnostics |
| cap violations → rejections | `duplicate_chunk_names_reject` | `duplicate chunk name \`setup\`` |
| G5 byte-identity | `chunk_less_skills_are_byte_identical_in_catalog_and_routing` | identical `SkillSummary` set (slug+headline only); identical routing report incl. exact `LoadedSkill.body` bytes (the orchestrator loads the full raw skill file — pre-existing behavior, preserved); `declares_chunks() == false` |
| G5 byte-identity | `existing_workspace_tests_unchanged` | pre-PR-1 routing scenario still selects `xlsx` |
| constants | `chunk_caps_match_prd_defaults` | 32 / 24_000 |
| public surface | `chunk_entry_validation_is_public_for_future_prs` | `validate()` exported for PR-2/PR-4 use |

**Sabotage-run honesty note (ADR 0028 discipline):** the failure paths were
proven live, not just asserted — during test-fixture development, chunks
were initially written to `<tempdir>/skills/…` while the store roots at
`<tempdir>/memory-root/skills/…`; the three affected tests then **failed
with the exact fail-closed rejection** (`file not found under chunks/`)
rather than passing vacuously. That is direct behavioral evidence the
manifest-vs-disk validation gates what it claims to gate. The fixture was
corrected; no test in the final suite passes vacuously.

## 6. §9 cap confirmation (PRD obligation)

Survey command: `find skills -name "*.md" | wc -l` → 326 files; per-file
`##` section counts sampled → max 9; `grep -rl "^chunks:" skills/` → 0
usage. Result recorded in the PRD §9 Q2: 32 chunks = 3.5× headroom over
the observed 9-section maximum; `MAX_CHUNK_BYTES = 24_000` aligned to
`MAX_SKILL_CONTEXT_CHARS` (a chunk larger than one skill's whole inline
budget could never be injected whole). `MAX_CHUNKS_PER_SELECTION` remains
open until PR-2 routing (noted in PRD).

## 7. Deviations from the directive

1. **Storage layout interpretation.** The directive said
   "`<skill-root>/<slug>/chunks/`". The workspace reality is
   `<root>/skills/<slug>.md` (files, not per-skill dirs), so chunks resolve
   to `<root>/skills/<slug>/chunks/<name>.md` — a sibling directory named
   after the skill, matching the existing `references/` resource-dir
   convention (e.g. `skills/ascii-video/references/`). This matches the
   PRD's intent and existing disk conventions; flagged for the record.
2. **Added fail-closed field caps** beyond the directive's two named caps
   (240/480 chars, ≤8 key elements). Rationale: unbounded manifest text
   would inflate the always-loaded catalog prefix the PRD itself flags as
   a risk (§8 metadata bloat). Consistent with crate contract "inputs that
   exceed bounds are rejected, not silently truncated."
3. **PRD §9 said "confirm" the caps; the survey found no evidence forcing a
   change**, so 32/24_000 stand as confirmed. Documented in PRD §9 Q2.

## 8. Unresolved items (honest scope)

- **PR-2 open item:** `MAX_CHUNKS_PER_SELECTION` (proposed 3) is not yet
  defined; PR-1 deliberately shipped routing-neutral.
- **D2 neutrality:** `summary`/`key_elements` are parsed and carried but
  deliberately do not participate in ranking; flipping that requires the
  D3 eval verdict (PR-4) — unchanged contract.
- **CI:** all evidence above is local. Canonical/MSRV/five-target workflows
  have not run for this tree; per repo contract they gate releases, not
  local PR completion claims.
- **Seed library:** no seed skill yet uses `chunks:` (0/326); the tier is
  exercised only by fixtures. Authoring guidance is PR-5 scope.

## 9. Readiness effect and status

PR-1 complete and locally verified; AC-1 satisfied with traceable test
evidence; G5 and D1 constraints hold under live failure proof; naming
embargo clean. PRD updated (PR-1 IMPLEMENTED + §9 Q2 resolved);
migration-status updated (IMPLEMENTING — PR-1 landed). Next: PR-2
(two-level routing in `vesper-memory`).

Per ADR 0028: this report's completion claim traces to the exact commands
and tests quoted in §2 and §5, runnable against the current tree.
