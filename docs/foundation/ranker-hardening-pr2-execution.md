# Ranker Hardening PR-2 Execution Record (Option A — the decoupling)

- **Work unit:** `docs/ranker-hardening-prd.md` §5 PR-2
- **Status:** COMPLETE — locally verified 2026-09-13
- **Scope:** `crates/vesper-memory/src/skill_orchestrator.rs` (chunk-tier
  tokenization only); `crates/vesper-memory/tests/chunk_routing_eval.rs`
  (unignore + message update). Skill-tier arithmetic untouched.

## 1. Objective

Decouple chunk-tier evaluation from `SEMANTIC_ALIASES` expansion (Option A):
chunk pools built from raw stemmed tokens; prompt-side pool keeps expansion;
the disclosed 520-pt literal residual remains by design. Flip the PR-1 anchor
green and prove M2–M5.

## 2. The change (one behavioral line + its twin function)

```rust
// skill_orchestrator.rs — rank_chunks (was :868)
- let entry_tokens = semantic_tokens(&routing_text);
+ let entry_tokens = raw_semantic_tokens(&routing_text);

// new twin, adjacent to semantic_tokens:
fn raw_semantic_tokens(text: &str) -> BTreeSet<String> {
    // identical: split on non-alphanumerics, stem(), len>=2, STOP_WORDS
    // omitted:    the SEMANTIC_ALIASES expansion loop (:1300-1303 logic)
}
```

Scope fence honored: `semantic_tokens` itself is byte-identical (verified by
diff — only a new sibling function was added above it and one call site in
`rank_chunks` changed); `score_candidate`, activation thresholds, the alias
table, and the prompt pool at :422 are untouched.

## 3. Evidence — exact commands and results

### 3.1 The anchor flips green (M2)

```
$ cargo test -p vesper-memory --test chunk_routing_eval cross_talk
test cross_talk_pin_deploy_slug_manufactures_rollback_overlap ... ok
test cross_talk_control_no_alias_slug_routes_honestly ... ok
```

The `#[ignore]` from PR-1 is removed; the pin now asserts the post-fix
invariant (any regression to two-sided chunk-tier expansion fails it). The
control passes unchanged — honest overlap is not degraded.

### 3.2 The D3 eval ladder holds (M4)

```
$ cargo test -p vesper-memory --test chunk_routing_eval
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured
```

Includes `harness_fixture_determinism`, `harness_condition_isolation`,
`harness_metric_invariants`, `full_metrics_run_emits_canonical_table`,
`d3_verdict_follows_the_decision_rule` — the PR-4 corpus results are
unperturbed by the chunk-tier decoupling (its fixtures never depended on
chunk-side alias expansion; the recon's zero-blast-radius finding E7 held).

### 3.3 Skill tier frozen (M5)

```
$ cargo test -p vesper-memory --lib
test result: ok. 43 passed; 0 failed
```

The unit suite includes the alias-exercising skill-tier fixtures
(`ranks_file_type_and_semantic_matches_without_loading_every_skill` —
spreadsheet/excel expansion at the SKILL level; deploy/release rows).

### 3.4 Gates

```
cargo test --workspace --all-features → WORKSPACE PASSED: 2220 (0 failed)
cargo xtask acceptance                → 20 exact cases passed
cargo xtask naming-guard              → clean (18 hits, all frozen in baseline)
cargo clippy --workspace --all-targets --all-features → 0 warnings
cargo fmt -p vesper-memory            → clean
```

Floor: 2,219 → 2,220 (the unignored pin now counts toward the floor; no test
removed or weakened).

## 4. Deviations

None. The implementation is the PRD's Option A exactly; the residual
(520-pt literal match possible when a prompt token expands into a literal
chunk token) is the PRD's disclosed accepted scope, observable in the pin
fixture as rollback scoring exactly one 520-pt overlap and losing to
migrate's honest two-token 1,040.

## 5. Unresolved items

- PR-3: re-run the D3 eval under the hardened ranker and re-record the
  canonical table + overhead numbers if they move (M4 re-measurement); D3
  stop rule applies if controls move or probes stop resolving.
- Layer-2 five-target CI verification is a push/release concern (per
  `verify-with-xtask-verify`); this record is local-Linux evidence.

## 6. Readiness effect

The cross-talk defect class is closed in production: chunk pools can no
longer inherit alias expansion, the anchor guards the invariant
permanently, and the pin/condition/D3/skill-tier suites all hold on the
hardened tree. PR-3 re-measures and closes the initiative.
