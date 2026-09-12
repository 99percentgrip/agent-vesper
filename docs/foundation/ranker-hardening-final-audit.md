# Ranker Hardening Final Audit (PR-1 → PR-3) — Execution Record

- **Work unit:** final audit of the ranker-hardening initiative
  (`docs/ranker-hardening-prd.md` PR-1..PR-3), mirroring the
  context-paging full-audit standard (F1–F5 precedent).
- **Status:** COMPLETE — 1 finding, repaired with fail-first proof
- **Date:** 2026-09-12 (post-release, on top of v0.22.0)

## 1. Objective and method

Audit the three landed PRs as a whole: scope-fence integrity, anchor
non-vacuity (sabotage run), metric/ranker consistency (the F2 class),
arithmetic re-derivation (M2), and docs-vs-live-evidence reconciliation.
Every check executed against the current tree, not read from prior reports.

## 2. Checks and results

### 2.1 Scope fence (constraint 1 of PR-2) — VERIFIED

`git diff 403404d..HEAD -- crates/vesper-memory/src/skill_orchestrator.rs`
contains exactly two functional changes: the `rank_chunks` call-site swap
(`semantic_tokens` → `raw_semantic_tokens`) and the new twin function. The
`semantic_tokens` alias loop itself: **zero diff lines**. Skill-tier call
sites (`:422` prompt pool, `:804` description pool): unchanged. The
`#[ignore]` removal in the pin test and its message flip are the only other
diffs. Fence held.

### 2.2 Anchor non-vacuity — VERIFIED (sabotage run)

```
revert call site → semantic_tokens:  cross_talk_pin ... FAILED
restore raw_semantic_tokens:         cross_talk_pin ... ok (2 passed)
```

The pin genuinely bites: with the old behavior restored it fails; with the
fix it passes. This is the proof M1's fail-then-pass receipt implied, now
demonstrated bidirectionally on the shipped tree.

### 2.3 Metric/ranker consistency — **FINDING A1 (latent), FIXED**

`metadata_token_overhead` (`:96-97`) tokenized both sides with
`semantic_tokens` (alias-expanded) while the routing tier, post-Option-A,
consumes raw pools. A metadata word that is an alias source would be
counted as 4 overhead tokens (itself + 3 expansions) when the ranker
actually holds 1. **Impact on shipped evidence: none** — the eval corpus
metadata contains no alias-source words, so the recorded 13/14/16 ladder
was unaffected (re-run after the fix: table byte-identical). The defect
class is exactly F2 (metric tokenizer ≠ ranker tokenizer); it was luck of
vocabulary, not correctness, that kept the numbers honest. **Fix:** both
sides now `raw_semantic_tokens` (measure what the ranker holds). All
7 eval tests green post-fix; table unchanged.

### 2.4 M2 arithmetic re-derivation — VERIFIED

Independent Python re-implementation of `stem()` + the alias table over the
pin scenario: rollback ∩ expanded-prompt = `{release}` → exactly the
bounded 520-pt residual; migrate = `{canary, rehearsal}` → 1,040;
certificates = ∅. The PRD's predicted post-fix geometry is exactly what
ships, and the residual is visible, bounded, and accepted scope.

### 2.5 Docs vs live evidence — VERIFIED

- Release `v0.22.0` exists with 16 assets; tag resolves to `9b56a9e` —
  matches the report's §4.1 receipts.
- `registry/agent.json` version 0.22.0 + all five archive URLs point at
  the v0.22.0 release; PR #539 head `55973fe` carries the same manifest.
- Workspace version 0.22.0; PRD status COMPLETE; migration-status row
  records the release evidence; report §4.1 contains only earned
  receipts (the placeholder-draft issue was caught pre-commit and is
  recorded as a deviation).

## 3. Finding summary

| ID | Severity | Description | Resolution |
|----|----------|-------------|------------|
| A1 | Medium (latent; zero impact on shipped evidence) | Overhead metric tokenized with expanded pools post-Option-A; could over-report by alias fan-out in future corpora | Fixed: both sides raw; corpus table byte-identical (re-run); all suites green |

No other findings. The per-PR gates had already covered floor/acceptance/
naming-guard/architecture; this audit adds the cross-PR invariants they
can't see: fence integrity, anchor bidirectionality, metric consistency,
arithmetic truth, evidence reconciliation.

## 4. Honest boundary

This audit ran **post-release** (on top of v0.22.0). The shipped binaries
contain the A1 metric defect (latent; affects the eval metric only, never
routing behavior — routing pools were already raw). The fix lands on
`main` for the next release. Routing correctness, which is what the
initiative promised, was verified intact in the shipped tree via §2.2.

## 5. Deviations

None. The sabotage run temporarily reverted production code in the working
tree and restored it immediately (verified by the passing run and the
final diff review).

## 6. Readiness effect

The ranker-hardening initiative now carries the same audit standard as
context paging: one latent metric defect found and fixed with a
demonstrated fail-first property, all cross-PR invariants verified against
the live tree, and the shipped release reconciled against every recorded
claim.
