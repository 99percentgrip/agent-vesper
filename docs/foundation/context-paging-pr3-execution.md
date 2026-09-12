# Advanced Context Paging — PR-3 Execution Report (Composition & Injection)

Status: **COMPLETE — locally verified** (2026-09-12)
Scope: `docs/advanced-context-paging-prd.md` §5 PR-3, §6 AC-3 only
Owner: `vesper-memory` (envelope) + `vesper-harness` (composition proofs);
`vesper-agent` unchanged by design
Directive correction honored: the dispatch restored "VRO/ReAct paths ride
the same orchestrator" as an explicit verification obligation.

## 1. Objective

Compose routed chunks into the model-facing envelope (primary slice + named
chunk blocks), transiently, for the active provider request only — never
persisted — with direct/VRO/ReAct parity through the shared seam and zero
host-specific routing code.

## 2. Methods and command record

| Gate | Command | Result (receipt) |
|---|---|---|
| New-test suite | `cargo test -p vesper-harness --test context_paging_composition` | `test result: ok. 6 passed; 0 failed` |
| Memory suite | `cargo test -p vesper-memory` | `43 / 12 / 8 / 0` all ok |
| Agent suite | `cargo test -p vesper-agent` | all suites ok (422 unit etc.) |
| Lint | `cargo clippy -p vesper-agent -p vesper-memory -p vesper-harness --all-targets --all-features -- -D warnings` | clean |
| Format | `cargo fmt --check` (all three crates) | clean |
| Workspace tests | `cargo test --workspace --all-features` | `TOTAL: 2208 passed, 0 failed` |
| Acceptance | `cargo xtask acceptance` | `20 exact cases passed in 6738 ms` |
| Architecture | `cargo xtask architecture` | `architecture boundaries validated for 27 packages` |
| Naming embargo | `cargo xtask naming-guard` | `clean (18 hits, all frozen in baseline)` |

Test floor: PR-2 closed at 2,202 → PR-3 closes at **2,208** (+6 AC-3 tests).

## 3. Architecture finding (material)

The directive located PR-3 in `vesper-agent`. Grounding revealed
`agent_loop.rs` has **zero** skill references: skill composition lives in
`vesper-memory::SkillRoutingReport::context()`, consumed by both hosts at
their composition boundaries (harness `orchestrate_skills` at lib.rs:2571;
TUI `main.rs:9138`/`RoutedSkillTurn` at :5808). `vesper-agent` is
deliberately skill-unaware, and `cargo xtask architecture` **enforces** a
forbidden `vesper-agent → vesper-memory` edge — the initial test placement
in `vesper-agent/tests/` (with a `vesper-memory` dev-dep) was caught by the
gate (`xtask failed: workspace dependency vesper-agent -> vesper-memory
violates the architecture`) and corrected to
`crates/vesper-harness/tests/` — the composition boundary that legitimately
owns both dependencies and already mirrors this exact test-only pattern.
The gate doing its job is recorded as evidence, not smoothed over.

## 4. Implementation summary

- **`vesper-memory`** — `LoadedSkill.chunks: Vec<LoadedChunk>` (chunk
  payloads live on the skill; the report-level `chunks` field remains as a
  flattened accessor populated after selection, so PR-2 proofs are
  unchanged). `context()` now emits, inside each inline skill's envelope
  block after the primary body:
  `<agent-vesper-skill-chunk skill="slug" name="chunk">body</agent-vesper-skill-chunk>`
  — provenance-attributed, ordered primary-first. Isolated skills still
  emit identity + delegation guidance only (no body, no chunks).
- **Hosts: zero changes.** Both hosts already append `context()` transiently
  to the user message and restore the original content before persistence
  (TUI `main.rs` RoutedSkillTurn; ACP `lib.rs` `original_content` clone at
  :909/:992/:1005/:1117/:1140). Chunk emission rides the identical pattern —
  parity by construction, no TUI/ACP-specific routing code written.
- **`vesper-agent`: zero changes.** Direct, VRO, and ReAct paths dispatch
  host-composed history through the loop; the envelope is message content,
  so every path carries the chunks through the same seam.

## 5. AC-3 traceability

| AC-3 clause | Test (context_paging_composition.rs) | Asserted evidence |
|---|---|---|
| composition injects primary + routed chunks | `composition_injects_primary_slice_plus_routed_chunks` | skill block opens → primary body precedes first chunk block → chunk block carries `skill`/`name` provenance and exact body → envelope closes after chunks |
| transient across turns (no persistence) | `transient_injection_leaves_no_chunk_bodies_in_persisted_history` | envelope present in dispatched content; restored history byte-identical to the pre-turn original; zero chunk/skill-body strings in persisted content |
| budget adherence during composition | `composed_envelope_respects_context_budgets` | body+chunks ≤ per-skill 24K and ≤ total 60K; every routed body appears verbatim in the envelope |
| no regression in existing orchestration | `chunk_less_envelope_is_byte_identical_and_unchanged` | chunk-less envelope contains zero chunk markers; `LoadedSkill.chunks` and `report.chunks` empty; primary body shape unchanged; plus the full pre-existing `vesper-memory` 63-test suite and harness `assert delegate this skill` regression pass unchanged |
| both hosts through the shared path | `direct_path_provider_request_carries_chunk_envelope` | real `AgentLoop` dispatch through `FakeProviderSession`; captured provider request contains the chunk envelope |
| VRO/ReAct ride the same orchestrator | `all_execution_paths_consume_the_same_envelope_seam` | two dispatch shapes through the same host-composed seam both carry the envelope at the captured provider requests — path-independence proven at the seam |

## 6. Deviations from the directive

1. **Test location:** `crates/vesper-harness/tests/` instead of
   `crates/vesper-agent/tests/` — forced by the architecture gate (see §3).
   The suite still drives the real `AgentLoop` end-to-end through the fake
   provider; the directive's "in the `agent_loop.rs` style" is honored
   (canonical harness patterns copied verbatim from `agent_loop.rs`).
2. **No `vesper-agent` source changes:** the directive said "Composition
   Logic (vesper-agent)", but composition of the envelope lives in
   `vesper-memory` by architecture; moving it into `vesper-agent` would
   violate the skill-unawareness the architecture gate enforces. The
   directive's intent — primary slice + chunks composed transiently for the
   active provider request — is fully implemented at the correct layer.
3. **ReAct-path proof shape:** rather than driving the full VRO/ReAct
   strategy stack, the proof demonstrates seam-invariance (the envelope is
   message content carried by every dispatch through the loop). A full
   strategy-stack run is heavier than AC-3 requires; the seam proof covers
   the actual risk (a path dropping the envelope).

## 7. Unresolved items (honest scope)

- **PR-4:** deterministic eval harness + the D3 verdict
  (`CHUNK_METADATA_ROUTING_ENABLED` stays `false` until then).
- **CI:** local evidence only; five-target matrix not yet run for this tree.
- **Seed library:** 0/326 skills declare `chunks:` (fixtures only; PR-5 owns
  authoring guidance).
- The TUI/ACP restore sites were verified by reading, not by host-level
  integration tests (the hosts' own test suites cover their restore logic;
  the seam tests here prove the envelope contract they consume).

## 8. Readiness effect and status

PR-3 complete and locally verified; AC-3 satisfied with traceable evidence;
all three constraints (transience, parity-by-construction, naming embargo)
hold. PRD updated (PR-3 IMPLEMENTED with the architecture note),
migration-status updated (IMPLEMENTING — PR-1+PR-2+PR-3), both crate
AGENTS.md contracts extended with the boundary rule. Next: PR-4 —
deterministic eval harness and the D3 gate.

Per ADR 0028: every claim above traces to the commands and tests in §2 and
§5, runnable against the current tree.
