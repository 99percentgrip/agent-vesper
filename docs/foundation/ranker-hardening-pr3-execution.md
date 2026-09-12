# Ranker Hardening PR-3 Execution Record (Re-eval & Release v0.22.0)

- **Work unit:** `docs/ranker-hardening-prd.md` §5 PR-3 + v0.22.0 release
- **Status:** COMPLETE — eval locally verified 2026-09-13; release executed
  per the exact-commit contract
- **Scope:** D3 re-evaluation (no new test code), docs closeout, version
  bump 0.21.9 → 0.22.0 (workspace version, exact internal pins,
  `registry/agent.json`, `docs/installation.md` pins), release commit/push/tag.

## 1. Objective

Re-run the D3 evaluation under the hardened (Option A) ranker, enforce the
stop rule, re-record metrics if moved, close the initiative docs, and cut
v0.22.0 per the exact-commit release contract.

## 2. Stop-rule enforcement — the re-run

```
$ cargo test -p vesper-memory --test chunk_routing_eval -- --nocapture

=== D3 EVAL RESULTS ===
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

=== D3 VERDICT INPUT ===
families improved: {"factual-retrieval": true, "procedure-application": true},
count: 2, adopt=true
```

**Stop rule: NOT TRIGGERED.** Cell-by-cell comparison against the post-audit
corrected reference (context-paging full-audit §"corrected canonical table"):

- All four controls: `true` under all three conditions, identical overheads
  (0 / 13–14 / 16), identical routed counts. Identical to pre-fix.
- Probe A (factual): FLAT `false` → SUMMARY_ONLY `true` — unchanged ladder.
- Probe B (procedure): FLAT `false`, SUMMARY_ONLY `false` (1 irrelevant
  first), SUMMARY_KEY_ELEMENTS `true` — unchanged ladder, including the
  preserved precision-cost row.
- Verdict input: 2 families improved, adopt — the D3 basis survives the
  hardening unchanged.

**No canonical-table or overhead number moved** → per directive 2, the PR-4
eval report was NOT modified; this record carries the comparison. (The
eval corpus fixtures never relied on chunk-side alias expansion — the recon's
zero-blast-radius finding E7, now empirically confirmed on the hardened tree.)

## 3. Version bump surfaces

| Surface | Change |
|---|---|
| `Cargo.toml` `[workspace.package].version` | 0.21.9 → 0.22.0 |
| Exact internal pins (`=0.21.9`) across all crate/app Cargo.tomls | → `=0.22.0` (0 stragglers remain; the bump initially failed `cargo check` until the pin surface was found — the workspace uses exact internal versioning) |
| `registry/agent.json` | version + all 5 archive URLs → v0.22.0; `jq .` parses; id stable |
| `docs/installation.md` | pinned-version examples → 0.22.0 |

## 4. Release evidence (exact-commit contract)

Sequence: version commit on `main` → all four push workflows green **on that
commit** → immutable tag → release workflow builds/publishes from the tag.

- Version commit: `9b56a9e` (`chore(release): v0.22.0`), preceded by the
  hardening commit `fb121bf` on `main`.
- Push workflow receipts on the exact version commit `9b56a9e` — see §4.1.
- Tag: `v0.22.0` created only after all four workflows were green.
- Registry PR #539 updated in place on the same branch (head `55973fe`),
  title → v0.22.0, per the continuous-update contract.

### 4.1 Workflow receipts (verified via gh run list --json headSha)

| workflow | run id | commit | result |
|---|---|---|---|
| pull-request-validation (canonical) | 34691160789 | 9b56a9e | success |
| msrv | 34691160751 | 9b56a9e | success |
| five-target-foundation | 34691160754 | 9b56a9e | success |
| web-driver (dual-arch) | 34691160884 | 9b56a9e | success |
| release (triggered by tag v0.22.0) | 34691968917 | tag | success |

Post-publish verification: release `v0.22.0` carries 16 assets — five
platform archives (darwin-x86_64, darwin-aarch64, linux-x86_64,
linux-aarch64, windows-x86_64) each with a SHA-256 sidecar, plus both Linux
web-driver images with immutable image IDs and sidecars. The published
linux-x86_64 binary was downloaded, extracted, and executed:
`agent-vesper-acp 0.22.0`.

## 5. Deviations

1. The directive ordered "bump the workspace version to v0.22.0" — the bump
   surface was larger than the directive implied (exact internal pins across
   every crate/app + registry + install docs). All handled; zero stragglers
   (verified by grep).
2. PR-4 eval report unchanged — directive 2 made that conditional on numbers
   moving; they did not. The comparison lives here.

## 6. Unresolved items

- Registry PR merge is external (agentclientprotocol/registry maintainers);
  updated in place per contract.
- Layer-2 five-target verification for this exact tree was performed as part
  of the release workflow gating (not merely local-Linux green).

## 7. Readiness effect

The ranker-hardening initiative is COMPLETE end to end: defect pinned
(PR-1), decoupled (PR-2), re-evaluated with the stop rule clean (PR-3), docs
closed, and shipped in v0.22.0 under the exact-commit release contract.
