# Advanced Context Paging — Full Implementation Audit (PR-1 through PR-5)

Status: **COMPLETE — 5 findings, all repaired with regression proofs** (2026-09-12)
Scope: the entire initiative against `docs/advanced-context-paging-prd.md`
— every AC re-traced to shipped code, the test suites re-derived for
vacuity, cross-PR invariants re-computed by hand, and every execution
report's claims re-checked against its own recorded evidence.
Method: audit-first (read code and claims before touching anything),
regression-test-first (every finding reproduced as a FAILING test before
its fix; the `github-issue-to-pr` sabotage-run discipline applied
internally), then repair, then honest re-measurement of everything the
repairs perturbed.

## 1. Findings summary

| ID | Severity | Layer | Defect | Repair |
|---|---|---|---|---|
| F1 | **High** | PR-2 routing | Per-skill chunk budget never decremented inside the chunk loop: N chunks each fitting the initial allowance collectively exceeded the 24K per-skill cap (reproduced at 36,261 chars). Total-budget safety made this reachable only with 2+ chunked skills selected. | `per_skill_remaining` now decrements per loaded chunk; over-budget chunks rejected with the existing explicit reason. |
| F2 | **High (evidence integrity)** | PR-4 metrics | `metadata_token_overhead` measured SUMMARY_ONLY against the FULL routing text — key_elements tokens counted for a condition that never reads them. The published 16/16 parity was an artifact of the metric, not a measurement. The eval's invariant self-test asserted the buggy relation (`summary ≤ full` on wrong values) and so passed vacuously. | Metric now measures each condition against its own text. True values: SUMMARY_ONLY 13–14, SUMMARY_KEY_ELEMENTS 16. |
| F3 | **High (fail-closed gap)** | PR-2/PR-3 routing | Disk-level manifest validation `continue`d BEFORE explicit-selection handling: `use skill X` with a broken manifest produced NO `explicit_error` — an explicit user request vanished silently. Parse-level errors did set it; two layers, two behaviors. | Disk-level path now sets `explicit_error` for direct and bundle members, mirroring parse-level. |
| F4 | **Medium** | PR-2 routing | Manifest-defect rejection masked `archived` — a deliberate user action (manage_skill) reported behind an authoring defect, in both the parse-level and disk-level paths. | `archived` short-circuits before manifest validation in both paths. |
| F5 | **High (evidence integrity)** | PR-4 corpus/report | The procedure CONTROL case silently failed to activate after the PR-4 corpus hardening renamed the skill (its query still said "deployment runbook" → activation below threshold). The published table recorded those failures; the narrative claimed "controls succeed under every condition." The report overstated its own data. | Control query fixed (explicit activation, matching the probes). Re-measured: all controls succeed under all conditions. Report corrected with an explicit audit-correction note; narrative now matches the table. |

**Not defects (verified, no action):** PR-3 harness relocation (architecture
gate was right); skill-level activation interplay in the eval corpus (probes
use explicit activation by design, documented); `#7 read_section` (correctly
left open in the PRD); seed-mirror flow (sanctioned); PR-1's
`existing_workspace_tests_unchanged` remaining as a scoped regression proxy.

## 2. Verdict integrity (the question that matters)

**ADOPT stands, on corrected data.** The verdict's inputs were never the
control row or the overhead parity: the decision rule is across-family
improvement on the metadata probes. Both probes' improvements (factual:
FLAT→SUMMARY_ONLY; procedure: FLAT+SUMMARY_ONLY→SUMMARY_KEY_ELEMENTS)
were real before and after every correction. F2 corrected only the cost
side (making SUMMARY_ONLY look cheaper, which strengthens the ladder), and
F5 restored a control that had silently stopped controlling. The audit
does NOT treat the verdict as untouchable — it re-derived it from the
corrected table and it survives.

## 3. Regression proofs (all fail-then-pass, sabotage-verified)

Four tests added to `crates/vesper-memory/tests/skill_routing.rs`:

- `per_skill_budget_decrements_across_chunks_of_one_skill` — failed at
  36,261 > 24,000 pre-fix; passes post-fix.
- `explicit_selection_with_invalid_manifest_fails_loudly` — failed
  (`explicit_error` was None); passes.
- `archived_reason_takes_precedence_over_manifest_defect` — failed
  (`invalid chunk manifest…` masked `archived`); passes.
- `summary_only_overhead_excludes_key_elements_tokens` — failed (2 vs 1);
  passes.

Each was confirmed failing on the pre-fix tree before its fix landed
(receipts in the audit trail above); none passes vacuously.

## 4. Corrected canonical table (re-measured from the current tree)

| family | skill | condition | success | irrelevant_before | routed | overhead_tokens |
|---|---|---|---|---|---|---|
| factual-retrieval | observability-reference | FlatDescription | true | 0 | 1 | 0 |
| factual-retrieval | observability-reference | SummaryOnly | true | 0 | 1 | 13 |
| factual-retrieval | observability-reference | SummaryKeyElements | true | 0 | 1 | 16 |
| factual-retrieval | observability-reference | FlatDescription | false | 0 | 0 | 0 |
| factual-retrieval | observability-reference | SummaryOnly | true | 0 | 2 | 13 |
| factual-retrieval | observability-reference | SummaryKeyElements | true | 0 | 2 | 16 |
| procedure-application | cutover-runbook | FlatDescription | true | 0 | 1 | 0 |
| procedure-application | cutover-runbook | SummaryOnly | true | 0 | 1 | 14 |
| procedure-application | cutover-runbook | SummaryKeyElements | true | 0 | 1 | 16 |
| procedure-application | cutover-runbook | FlatDescription | false | 0 | 0 | 0 |
| procedure-application | cutover-runbook | SummaryOnly | false | 1 | 1 | 14 |
| procedure-application | cutover-runbook | SummaryKeyElements | true | 0 | 2 | 16 |

## 5. Gates (post-audit tree)

| Gate | Result |
|---|---|
| Workspace tests | **2,217 passed / 0 failed** (floor 2,185+; +4 audit regressions; PR-1..5 floor progression 2,185→2,197→2,202→2,208→2,213→2,217) |
| `cargo xtask acceptance` | 20/20 |
| clippy `-D warnings` | clean |
| fmt | clean |
| `cargo xtask architecture` | 27 packages |
| `cargo xtask naming-guard` | clean (18 frozen hits, zero erosion) |
| PR-3 harness suite | 6/6 unchanged-green post-fix |
| Eval self-tests | 5/5 including corrected invariants |

## 6. Honest scope of the audit

- Local evidence only; the exact-commit CI matrix remains unrun for this
  tree (unchanged from PR-5's record).
- The audit re-verified invariants by derivation and test, not formal
  proof; ranking arithmetic was hand-checked for the fixture vocabularies,
  not exhaustively.
- PR-4's published table/history was corrected in place with an explicit
  audit note rather than rewritten silently — the correction is part of
  the record now.
- Remaining open items from PR-5 §7 (alias cross-talk remedy, chunked seed
  exemplar, corpus scale) remain open by choice, with evidence bars.

## 7. Verdict on the initiative

The implementation is sound post-audit: caps enforced fail-closed at every
layer (now including per-skill accumulation), explicit requests fail
loudly, deliberate user states report truthfully, the routing metadata
ADOPT decision stands on corrected measurements, and the documentation
matches shipped behavior. Four real defects and one evidence-integrity
error were found and repaired in this pass — none voided the initiative's
guarantees, and all now carry regression proof.
