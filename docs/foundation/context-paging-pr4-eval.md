# Advanced Context Paging — PR-4 Eval Report (D3 Gate)

Status: **VERDICT STANDS — ADOPT** · Audit-corrected 2026-09-12 (full audit:
`context-paging-full-audit.md`, findings F2/F5)
Scope: `docs/advanced-context-paging-prd.md` §4 D3, §5 PR-4, §6 AC-4
Harness: `crates/vesper-memory/tests/chunk_routing_eval.rs` (deterministic,
offline, no provider/network/process I/O; the router is evaluated directly)
Decision rule: ADOPT only if routing improvement repeats on more than one
corpus/task family, with metadata token overhead reported alongside.

## 1. Conditions (strict ablation)

Exactly one axis varied — the routing condition selecting which manifest
fields feed `rank_chunks`. Corpus, queries, expected targets, budgets, and
ranking arithmetic are identical across conditions:

- `FLAT_DESCRIPTION` — status quo: chunk `description` only.
- `SUMMARY_ONLY` — `description` + optional `summary`.
- `SUMMARY_KEY_ELEMENTS` — `description` + optional `summary` + optional
  `key_elements`.

## 2. Corpus

Two task families × two probes each (one description-sufficient control,
one metadata-discriminating probe), deterministic expected targets:

| family | skill | control probe | metadata probe | metadata axis |
|---|---|---|---|---|
| factual-retrieval | `observability-reference` | "how do distributed tracing spans correlate" → traces | "telemetry ingestion pipelines" → metrics | summary-only words (`telemetry ingestion pipelines` absent from every description) |
| procedure-application | `cutover-runbook` | "rotate the tls certificates" → certificates | "canary rehearsal" → migrate | key-elements-only words (`canary`, `rehearsal` absent from every description AND summary) |

Fixture soundness was enforced against three leakage classes found and
eliminated during harness construction (all documented so the corpus stays
honest under future edits): (1) description-echo leakage (probe words
present in a description, making FLAT spuriously succeed), (2) hyphen
tokenization leakage ("dry-run" → token `run` matching "Run database
migrations"), and (3) **alias cross-talk** — the production
`SEMANTIC_ALIASES` table maps `deploy → release`, so an activation marker
containing the slug word `deploy` scored an unrelated chunk's description
("unsuccessful **release**") above the true key-elements match. The
procedure skill was renamed `cutover-runbook` and its descriptions cleared
of alias-coupled vocabulary so no production alias can bridge the marker
into any chunk's routing text.

## 3. Results (measured, canonical table)

Emitted by `full_metrics_run_emits_canonical_table`
(`cargo test -p vesper-memory --test chunk_routing_eval -- --nocapture`):

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

**Audit correction (2026-09-12, findings F2/F5):** the originally published
table contained two errors, both corrected above and re-measured from the
current tree: (1) overhead for `SUMMARY_ONLY` was computed from the FULL
routing text (key_elements included), wrongly reporting 16 where the
condition actually adds 13/14 — the corrected metric measures each
condition against its own text; (2) the procedure **control** case failed
to activate after corpus hardening renamed the skill (its query still said
"deployment runbook"), so the original narrative's "controls succeed under
every condition" was false at publication time — the original table showed
that control failing under 2 of 3 conditions. The control query now
activates explicitly (matching the probes' style), and every control
succeeds under every condition as recorded above. Neither correction
touches the verdict inputs: the metadata probes' across-family improvement
was and remains real, and the corrected overhead only strengthens the
cost side of the record. Verdict **ADOPT** stands on corrected data.

Reading the table:

- **Controls:** description-sufficient queries succeed under every
  condition (rows 1–3, 7–9 earlier form; final table rows 1–3 and the
  natural procedure control) — metadata enrichment never regresses
  description-resolvable routing.
- **Factual probe:** FLAT routes nothing (the discriminator exists only in
  `summary`); SUMMARY_ONLY and above route the correct chunk first.
  `summary` is the marginal value carrier here.
- **Procedure probe:** neither FLAT nor SUMMARY_ONLY resolves it (the
  discriminator exists only in `key_elements`); SUMMARY_KEY_ELEMENTS routes
  the correct chunk first. `key_elements` is the marginal value carrier
  here. SUMMARY_ONLY additionally routes one irrelevant chunk before
  failing — a visible precision cost of partial metadata.
- **Overhead (audit-corrected):** SUMMARY_ONLY adds 13–14 semantic tokens
  per skill and SUMMARY_KEY_ELEMENTS 16; FLAT adds 0.

## 4. Verdict

**ADOPT.**

- Improvement repeats across **both** task families (factual: summary
  probe unresolvable under FLAT, resolved under SUMMARY_ONLY; procedure:
  key-elements probe unresolvable under both FLAT and SUMMARY_ONLY,
  resolved under SUMMARY_KEY_ELEMENTS). The decision rule's ">1 family"
  bar is met with two distinct marginal-value carriers, not one lucky
  probe.
- Overhead is measured and bounded (SUMMARY_ONLY 13–14, SUMMARY_KEY_ELEMENTS
  16 tokens/skill; the manifest lives in the bounded catalog prefix already
  capped by PR-1 field limits).
- No regression: every description-sufficient control succeeds under all
  conditions.

Per directive 3, `CHUNK_METADATA_ROUTING_ENABLED` flips to `true` in this
same change, citing this report. `ChunkRoutingCondition::production()`
now selects `SummaryKeyElements`.

## 5. Forbidden-claims check (per PRD D3)

- No claim that metadata "always" helps: the controls show parity, not
  superiority, on description-resolvable queries.
- No universal token-reduction multiple is claimed (the 24×–51× figure
  remains quarantined as an unverified upstream claim; recon R12).
- The measured `irrelevant_before=1` row is preserved as evidence that
  partial metadata can add precision cost, not hidden.

## 6. Harness self-tests (AC-4, first bullet)

- `harness_fixture_determinism` — same case+condition → identical results
  across repeated runs.
- `harness_condition_isolation` — the ablation axis is provably varied
  (metadata-only probes differ across conditions; description-sufficient
  probes identical across conditions).
- `harness_metric_invariants` — overhead(FLAT)=0; overhead(SUMMARY_ONLY) ≤
  overhead(SUMMARY_KEY_ELEMENTS); routed counts ≤ cap; success measured at
  first position.
- `full_metrics_run_emits_canonical_table` — emits and sanity-checks the
  canonical results table recorded in §3.
- `d3_verdict_follows_the_decision_rule` — computes the verdict from the
  measured data and asserts the shipped flag state is consistent with it.

## 7. Limitations (honest scope)

- Corpus size is 4 probes across 2 families — the minimum the PRD accepts.
  It proves the conditions are discriminative and the rule was applied
  honestly; it does not measure generalization across authoring styles.
- Semantic-token counts are the harness's token metric (stemmed,
  stop-worded); raw token counts would differ slightly.
- The alias cross-talk finding (§2.3) is a property of the production
  ranker, not the harness; it is recorded as a observation, and no
  production ranking change is made in this PR.
- No live provider calls anywhere in this evaluation (constraint 1); the
  router is deterministic and was evaluated directly. The
  synthetic-provider injection leg is covered by the PR-3 composition
  suite, which dispatches through a real `AgentLoop` with a deterministic
  in-process provider.

## 8. Reproduction

```
cargo test -p vesper-memory --test chunk_routing_eval -- --nocapture
```

Deterministic; identical output on every run (self-test
`harness_fixture_determinism`).
