# Execution Report: Chunk Score Floor PR-1 (The Noise Anchor)

- **Work unit:** Score-floor PRD PR-1 — noise-floor anchor fixture
- **Directive source:** Fast-track directive "SCORE FLOOR PR-1 (THE NOISE ANCHOR)", 2026-09-13
- **Status:** COMPLETE — anchor pinned with recorded failure receipt
- **PRD:** `docs/chunk-score-floor-prd.md` (§5 PR-1)
- **Floor:** 2,252 → 2,253 (+1 control test; pin is `#[ignore]`d until PR-2)

## 1. Objective

Prove the cosine noise-floor vulnerability exists, on this tree, with a
failing test that asserts the correct behavior — before any fix.

## 2. Methods / commands

- **Exact offline replication of the shipped ranker** (Python): `stem()`
  suffix ladder verbatim (`ing, ments, ment, ations, ation, ers, ies, ed,
  es, s`, first-match, `len > suffix+3`), stop-word list, FNV-1a 64-bit
  signed hashing into 64 dims, cosine. Prompt side alias-expanded
  (`semantic_tokens`), chunk side raw (`raw_semantic_tokens`, post
  ranker-hardening Option A).
- **Fixture search:** 60,000 random disjoint natural-language token-set
  pairs (geography vs compiler vocabularies), maximizing positive hashed
  cosine. Winner: `cos = +0.6547` (prompt {mesa, geyser, lagoon, cove,
  glacier} × chunk {tensor, lattice, assembler, socket, linker, opcode,
  topology}) → **1,440 points from cosine alone, zero overlap, no
  name-match**. This exceeds even the exemplar's observed 678-pt outlier —
  a deliberately strong anchor.
- **Append to** `crates/vesper-memory/tests/chunk_routing_eval.rs`
  (fixture + pin + control), reusing the file's `QueryEnv` idiom.
- Verification: targeted eval runs (`-- --ignored` for the receipt),
  workspace suite, acceptance, clippy, fmt, naming-guard.

## 3. Files

- Modified: `crates/vesper-memory/tests/chunk_routing_eval.rs` (+~120
  lines: `NOISE_MANIFEST`, `noise_store()`, `noise_positions()`, the pin,
  the control)
- No production-code changes (constraint 1 held — zero diffs outside tests
  and docs)

## 4. Exact evidence

### 4.1 The failure receipt (anchor, on this unhardened tree)

```
$ cargo test -p vesper-memory --test chunk_routing_eval -- --ignored --nocapture
test noise_floor_pin_zero_overlap_prompt_routes_nothing ... FAILED
thread '...' panicked at chunk_routing_eval.rs:509:5:
NOISE-FLOOR PIN: zero-overlap prompt routed chunks on pure cosine hash noise.
prompt tokens: {mesa, geyser, lagoon, cove, glacier}
routed: ["bytecode-lexicon"]
Expected: no chunks (no literal signal). Fix: score-floor PRD PR-2 conjunction gate.
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 8 filtered out
```

The vulnerability is now a pinned, executable fact: a prompt sharing zero
tokens (and no name substring) with any routing metadata still loads a
chunk, purely from signed-hash cosine.

### 4.2 The control (passes now, must keep passing after PR-2)

`noise_floor_control_genuine_overlap_still_routes` — prompt "explain the
tensor lattice of opcodes" (3 real overlaps) routes `bytecode-lexicon`.
This is the over-tightening guard: PR-2's conjunction gate must satisfy
both tests simultaneously.

### 4.3 Gate receipts

- `cargo test --workspace --all-features`: **2,253 passed / 0 failed**,
  138 suites (main tree green; the pin is `#[ignore]`d with a pointer to
  the PRD — same pattern as ranker-hardening PR-1)
- `cargo test -p vesper-memory --test chunk_routing_eval` (normal): 8
  passed, 1 ignored
- `cargo xtask acceptance`: **23/23**
- `cargo clippy --workspace --all-targets --all-features`: **0 warnings**
- `cargo fmt --check`: clean (after one `cargo fmt -p vesper-memory` for
  the long `orchestrate_with_condition` line)
- `cargo xtask naming-guard`: **clean (18 hits, all frozen in baseline)**

## 5. Deviations

- **Self-caught near-miss (serious, recorded):** my first attempt used
  `write_file` on the eval file, which **replaces** rather than appends —
  it silently deleted the 434-line D3 harness down to my 114-line stub.
  Caught immediately from the diff stat (`80 insertions, 400 deletions`),
  restored via `git checkout --`, re-appended correctly with `cat >>`.
  The incident is recorded here because it's the kind of tool-misuse that
  would have destroyed the PR-4 evidence surface if unnoticed. Lesson:
  never `write_file` an existing file whose current content matters.
- Fixture texts are natural-language (geography × compiler vocabularies),
  not synthetic tokens — deliberately, so the anchor reads as a real-world
  case rather than an adversarial construction. The offline search found
  stronger synthetic collisions (0.65 with random 3-digit tokens) but the
  natural-language pair at 0.6547 is equally strong and honest.

## 6. Unresolved items

- PR-2 (the conjunction gate) and PR-3 (G5 literal restoration + re-eval)
  await directives.
- The pin stays `#[ignore]`d until PR-2 — an executable demonstration
  exists via `-- --ignored` any time.

## 7. Readiness effect

The PRD's §5 PR-1 is discharged with a fail-then-pass anchor: the defect
is demonstrated on the unhardened tree (receipt above), the control
pre-registers the guard against over-tightening, and the main tree stays
green. PR-2 has everything it needs: the gate to implement, the two tests
it must flip simultaneously, and the full floor to hold.
