# Chunk score-floor Q1 execution: delimiter-bounded chunk-name admission

Date: 2026-09-13. Status: **complete**.
Owner PRD: [Chunk-tier score floor](../chunk-score-floor-prd.md) — §8 Q1,
resolved by owner directive after the PR-2 delivery surfaced it.

## Objective

Close the last single-conjunct admission bypass in the chunk tier: a
chunk name occurring as a **substring** of unrelated prompt text admitted
its chunk (+3,500 ⇒ `literal_signal = true`) with zero routing-text
overlap. Owner directive: "ok then fix it."

## Recon (before any edit)

- `phrase_matches` (`skill_orchestrator.rs:1430`) is **skill-tier
  shared**: 4 call sites (`:770` exclusions, `:780` external-risk terms,
  `:804` slug/name scoring +3,500, `:811` triggers). Only `:904` is the
  chunk tier. Modifying it would perturb skill-tier scoring — scope-fence
  violation. Decision: chunk-tier-local matcher.
- Chunk names are validated at parse time to `[a-z0-9-_]`
  (`types.rs` / orchestrator `:1293-1296`), so a boundary rule over that
  exact alphabet can never miss a legal name.
- The tokenizer splits on non-alphanumeric characters, so the accident
  surface is precisely "name embedded inside a longer word or
  hyphen/underscore chain."

## Changes

| File | Change |
|---|---|
| `crates/vesper-memory/src/skill_orchestrator.rs` | + `chunk_name_matches` (delimiter-bounded; boundary = char outside `[a-z0-9-_]` or string edge); `rank_chunks` `:904` now calls it instead of `phrase_matches`; comment. No other line touched. |
| `crates/vesper-memory/tests/chunk_routing_eval.rs` | + Q1 pin `chunk_name_embedded_substring_does_not_admit` (2 accidental prompts), + control `chunk_name_delimited_still_routes` (4 addressing forms), fixture + helpers |

## Red-first evidence (anchor discipline)

Pin run **before** the fix (verbatim):

```
test chunk_name_embedded_substring_does_not_admit ... FAILED
prompt: reorganize the pcometq chronicle into chapters
routed: ["comet"]
Expected: no chunks (the name `comet` is not a standalone token).
test chunk_name_delimited_still_routes ... ok
```

The failure is the exact predicted accident class: `comet` inside
`pcometq`, admitted with zero overlap. Control green both sides (the fix
does not over-tighten).

## Boundary rule (what counts as "the name stands alone")

An occurrence matches iff neither edge continues the name's own alphabet:

- `comet` in `pcometq` → `p`/`q` adjacency → **no**
- `comet` in `auto-comet-review` → `-` adjacency → **no**
- `comet`, `"comet"`, `read comet,` → delimiter or string edge → **yes**

Skill-tier matching is untouched: `phrase_matches` remains `contains` at
all 4 call sites (its substring behavior is a documented skill-tier
feature — e.g. trigger fragments — not part of this PRD's scope).

## Verification receipts (final)

- Pin + control: `chunk_name_embedded_substring_does_not_admit ... ok`,
  `chunk_name_delimited_still_routes ... ok` (11/11 in the eval binary,
  all pre-existing pins green).
- D3 canonical table: byte-identical to the PR-2 recorded state (both
  metadata-probe rows unchanged; noise-removal rows from PR-2 intact).
- Exemplar: `routing_proofs_target_chunk_ranks_first_for_its_phase`,
  `budget_proof_…`, `manifest_proof_…`, `g5_proof_…` all ok.
- Dollar repair tests: green (`dollar_literals_…`, `dollar_shorthand_…`).
- Workspace: **2,258 passed / 0 failed** (2,256 + 2 new).
- `cargo xtask acceptance`: **23/23** (`15187 ms`).
- Clippy `-D warnings`: clean. `cargo fmt --check`: clean.
- `cargo xtask naming-guard`: **clean (18 hits, all frozen)**.

## Deviations

None. Single-conjunct bypass closed exactly as directed; no re-scoping.

## Unresolved items

- PR-3 (G5 literal restoration + re-eval) remains open, unchanged.
- Skill-tier `phrase_matches` substring semantics (triggers, risk terms,
  slug scoring) are out of scope by fence; no evidence of an accident
  class there, and skill-tier scoring is frozen by PRD.

## Readiness effect

Chunk admission is now fully signal-gated on both conjuncts: cosine
cannot admit (PR-2), and an embedded name cannot admit (this change).
The routing-map contract survives in its documented form — name the chunk
as its own token and it routes deterministically.

## DOX closeout

Owning `crates/vesper-memory/AGENTS.md` updated (delimiter-bounded
chunk-name admission contract bullet); PRD §8 Q1 marked resolved with
evidence link; evidence index updated below. Parent docs unchanged —
crate-local contract, no cross-crate or workflow change.
