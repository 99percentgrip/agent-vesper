# Ranker Hardening PR-1 Execution Record (Option D — the anchor)

- **Work unit:** `docs/ranker-hardening-prd.md` §5 PR-1
- **Status:** COMPLETE — locally verified 2026-09-13
- **Scope:** `crates/vesper-memory/tests/chunk_routing_eval.rs` (fixture only; zero production-code changes)

## 1. Objective

Write the failing cross-talk fixture BEFORE the Option A fix, pinning the alias
cross-talk defect so remediation is an observable delta, per the PRD (M1, G2):

- a case whose **only** prompt↔chunk overlap is alias-manufactured,
- asserting the **desired** behavior (which fails on the unhardened tree for
  exactly the audited reason),
- plus a no-alias control proving the mechanism is the alias fan-out.

## 2. Methods and commands

```bash
# line-anchor verification (grep -n; the harness grep tool returned empty for
# multi-pattern queries — tool quirk, shell grep used instead):
grep -n "fn rank_chunks|SEMANTIC_ALIASES|entry_tokens|prompt_tokens =" \
  crates/vesper-memory/src/skill_orchestrator.rs
# → :422 prompt_tokens, :858 rank_chunks, :868 entry_tokens (alias-expanded),
#   :869 intersection, :873 ×520, :1375 SEMANTIC_ALIASES, :1384/:1385 rows

# fail-then-pass demonstration (the deliverable):
cargo test -p vesper-memory --test chunk_routing_eval cross_talk -- --ignored

# gates
cargo fmt -p vesper-memory && cargo clippy -p vesper-memory --all-targets --all-features
cargo test -p vesper-memory          # 43 + 12 + 7(6 pass/1 ignored) green
cargo test --workspace --all-features
cargo xtask acceptance
cargo xtask naming-guard
```

## 3. Files

| File | Change |
|------|--------|
| `crates/vesper-memory/tests/chunk_routing_eval.rs` | + pin fixture builder (`build_pin_store`), probe helper (`routed_positions`), the pin test, the no-alias control test |

## 4. Evidence — exact commands and results

### 4.1 The pin fails for exactly the audited reason

```
$ cargo test -p vesper-memory --test chunk_routing_eval cross_talk -- --ignored

---- cross_talk_pin_deploy_slug_manufactures_rollback_overlap stdout ----
assertion `left == right` failed:
CROSS-TALK PIN - pre-fix failure expected on the unhardened tree:
rollback displaced the honest target migrate via alias-manufactured
overlap ({deploy, publish, release} = 1,560 pts at :873).
Option A (raw chunk pools) must flip this assertion green.
positions: ["rollback", "migrate", "certificates"]
  left: Some("rollback")
 right: Some("migrate")
```

`rollback` first — riding the manufactured 3-token fan-out (1,560 pts) past
`migrate`'s honest two-token key_elements overlap (1,040 pts). The exact
mechanism and ranking displacement the PRD problem statement documents, now
executable on demand.

### 4.2 The no-alias control passes

```
$ cargo test -p vesper-memory --test chunk_routing_eval cross_talk_control
test cross_talk_control_no_alias_slug_routes_honestly ... ok
```

Identical fixture shape and probe, alias-free slug (`cutover-runbook`) →
`migrate` first. The displacement in 4.1 is caused by the alias fan-out, not
general overlap.

### 4.3 Gates

```
cargo test --workspace --all-features  → WORKSPACE PASSED: 2219
cargo xtask acceptance                 → 20 exact cases passed (offline)
cargo xtask naming-guard               → clean (18 hits, all frozen in baseline)
cargo clippy -p vesper-memory          → clean (warnings would fail CI -D)
cargo fmt -p vesper-memory             → clean
```

## 5. Deviations

1. **`#[ignore]` on the pin.** The pin asserts the *desired* behavior, which
   the unhardened tree cannot satisfy — a permanently-red main tree would
   break every floor gate. The anchor is executable on demand
   (`-- --ignored`) and its failure receipt is recorded here; PR-2 removes
   the attribute when Option A turns it green. The PRD's fail-then-pass
   mandate (M1) is satisfied by the recorded demonstration in §4.1, not by a
   red CI.
2. **Tool quirks worked around.** The harness `grep` tool returned empty for
   multi-pattern patterns (shell `grep -n` used instead); `edit_file`
   whitespace mismatches (two failed edits) were resolved with anchored
   Python replacement.
3. **Self-caught fixture defect.** The first written version had two
   compile-level placeholder errors (invented helpers) and the Python
   heredoc double-escaped `\n` sequences in the manifest string, producing a
   single-line manifest that parsed as chunk-less (both tests failed with
   `positions: []` — not the audited reason). Diagnosed via `cat -A`,
   fixed surgically, re-run. The intermediate defective states never left
   the local tree and are not evidence of anything except that the fix
   ordering in §4.1 is the genuine post-repair result.

## 6. Unresolved items

- PR-2 (Option A): build chunk pools from raw stemmed tokens; remove the
  pin's `#[ignore]` when it turns green; re-verify the no-alias control and
  the full D3 ladder (M2–M5).
- The 520-pt literal residual (prompt-side `deploy→release` expansion ×
  rollback's literal `release`) is accepted scope per the PRD; it will be
  visible post-Option-A as rollback scoring exactly 520 + cosine vs
  migrate's 1,040 — the M2 bound.

## 7. Readiness effect

The defect is pinned. Any change that claims to fix cross-talk must flip
`cross_talk_pin_deploy_slug_manufactures_rollback_overlap` green without
degrading the control or the D3 corpus results — the anchor now exists, is
recorded, and is one `-- --ignored` away from any future verification.
