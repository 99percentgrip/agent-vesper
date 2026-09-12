# Reconnaissance: SEMANTIC_ALIASES Cross-Talk in the Skill Chunk Ranker

- **Date:** 2026-09-12
- **Mode:** read-only reconnaissance (swarm Driver + Navigator synthesis); no
  production code was modified
- **Scope:** `crates/vesper-memory/src/skill_orchestrator.rs` ranking
  arithmetic and the chunk-routing test harness
  (`tests/skill_routing.rs`, `tests/chunk_routing_eval.rs`)
- **Provenance alias:** on-demand context-paging architecture discovered in the
  `context upstream` recon mission (`docs/architecture/recon_context_paging.md`)
  — referenced here only by that alias per the naming embargo
- **Verification:** every claim below is anchored to line numbers in
  `crates/vesper-memory/src/skill_orchestrator.rs` (re-read at synthesis time);
  stem behavior was additionally confirmed by compiling the `stem` function
  (lines 1309–1320) standalone and running the six forms below
  (`deploy→deploy`, `deploys→deploy`, `deploying→deploy`, `release→release`,
  `releases→releas`, `releasing→releas`)

## 1. Purpose

PR-4's evaluation (`docs/foundation/context-paging-pr4-eval.md`) discovered
that the skill slug `deploy-runbook` routed an unrelated chunk through the
alias table: the slug token `deploy` expanded to `{publish, release,
production}` and matched the word `release` inside a *different* chunk's
description. The eval corpus was renamed to `cutover-runbook` to avoid the
trap, and the underlying production behavior was deliberately left untouched
pending this reconnaissance. This document maps the exact mechanism, the
blast radius, and the remediation options with their risk to the 2,217-test
floor.

## 2. Exact mechanism — where the slug token bleeds

The skill slug does **not** enter the chunk pool through the routing text.
The bleed is at the intersection of two separately-built token pools:

- **Prompt pool (lines 421–422):** the whole prompt — including any slug the
  user typed ("use skill deploy-runbook") — is normalized and tokenized.
  `semantic_tokens` (1292) splits on non-alphanumerics, stems each token
  (1295), then expands aliases (1300–1303) on every token that equals an
  alias *source*.
- **Chunk pool (lines 867–868):** `rank_chunks` builds `entry_tokens` from
  the chunk's routing text only — `description` (+ `summary` /
  `key_elements` per the D3 condition, lines 59–77). The slug, the skill
  name, and the chunk name are **absent** from this pool.
- **The join (line 869):** `prompt_tokens ∩ entry_tokens`. When the prompt
  contains a slug token that is an alias source (`deploy`, line 1384), the
  prompt pool gains `{publish, release, production}`. Any chunk whose
  description contains a *related* token (`release`, itself a source at
  1385) gains `{publish, deploy, version}`. The shared expansions
  (`publish`, and `deploy↔release` directly) manufacture overlap where the
  literal texts share nothing.

Incident reconstruction (corrected): the PR-4 write-up called this "520
overlap points." The intersection is actually **three tokens** —
`{deploy, publish, release}` — scoring **1,560** points at line 873, plus a
nonzero cosine term. Removing alias expansion from one side drops it to 520;
removing it from both sides drops it to **∅**, the chunk fails the
`score > 0` filter (885), and never loads. The incident exists *only* in the
both-sides-expanded configuration.

Three structural facts shape every remediation option:

1. **Expansion is source-equal only (line 1301):** a related token never
   pulls in its source. "review" in a prompt does not activate the `pr` row
   (1379); only a literal `pr` token does.
2. **Hyphenated sources are unreachable (1294):** tokens are split on
   non-alphanumerics, so `pull-request` (1378) can never be *produced* by
   tokenization. Two of the ten rows are dead on the expansion side.
3. **`hashed_cosine` (1322–1347) adds a noise floor:** with 64 dimensions
   and signed hashing, two *disjoint* token sets of realistic size (~8–12
   tokens after alias expansion) still produce a small positive expected
   cosine (~1–3 signed collisions), which the `max(0.0)` clamp at 1345
   converts into a one-sided `×2,200` bonus at line 876. Alias expansion
   grows pool sizes on both sides, raising this floor.

## 3. Blast radius over the shipped test harness

Method: expansion (1300–1303) is purely additive (`extend`), so one-sided
ablation can only shrink the intersection by *expansion-only* tokens. An
assertion breaks only if its outcome depends on an expansion-only overlap, an
expansion-inflated cosine reordering, or alias-inflated overhead (which is
alias-symmetric at lines 96–97 and therefore invariant).

**Verdict: zero shipped assertions break under either one-sided ablation**
(prompt-side-only or routing-text-side-only alias expansion), and none break
under full removal:

- `skill_routing.rs:121–146` — decided by verbatim description≡prompt overlap
  (`rollback`, `failed→fail`, `staging→stag`, `deploy`) + the chunk-name
  phrase bonus (880–881); margin ≈ 5,580 vs 520.
- `skill_routing.rs:173–212` — explicit selection (782–783) bypasses skill
  scoring; budget math (645–680) is score-independent.
- `skill_routing.rs:224–253, 260–276, 363–386` — fail-closed paths that
  reject *before* `rank_chunks` ever runs.
- `skill_routing.rs:323–356` — qualifies on direct overlap alone (2,080).
- `skill_routing.rs:400–437` — flag-semantics proof; neither `telemetry` nor
  `diagnostics` is an alias source, expansion never fires.
- `skill_routing.rs:449–475` — `spreadsheet` fires row 1376 on *both* sides,
  redundant with the verbatim token; the file-extension bonus (822–829)
  dominates.
- `chunk_routing_eval.rs` — the only alias source in the corpus is `release`
  (rollback's description, lines 78/90), and **no corpus query contains any
  of its expansions** (`deploy`, `publish`, `version`), so the expansion is
  dead weight; success margins are direct-token-driven and exceed the
  2,200-point cosine ceiling.

The corollary is the important finding: **the cross-talk class itself is
untested.** Every fixture is constructed so its decisive tokens overlap
verbatim, which is exactly why the PR-4 incident — where the *only*
intersection was alias-manufactured — was invisible to the harness until a
live eval tripped it. One fixture whose *sole* overlap is a manufactured
`deploy↔release` pair would pin the behavior and is missing today
(remediation prerequisite, §5).

## 4. Decoupling options — ranked

The mandate: decouple chunk `summary`/`key_elements` evaluation from the
parent skill's alias expansion **without rewriting the ranking engine**
(that is, keep `semantic_tokens` + `hashed_cosine` + the 520/2,200/3,500
arithmetic untouched at lines 872–882).

| # | Option | Mechanism | Risk to skill-tier routing | Risk to 2,217 floor | Eval-bar implications |
|---|--------|-----------|------------------------------|--------------------|-----------------------|
| A | **Chunk-pool alias abstinence** | `rank_chunks` builds `entry_tokens` from raw tokens (stem + stop-words, no 1300–1303 loop) while `prompt_tokens` keeps expansion | None — skill tier (lines 804–818) untouched | **Zero** (§3 verdict) | Changes chunk routing for alias-linked vocab; needs a PR-4-style re-eval of the ladder |
| B | **Scoped expansion both sides** | New `semantic_tokens_scoped(text, scope)` where chunk-tier expansion uses a chunk-scoped alias subset (e.g. drop `deploy`/`release`/`pr` — the domain-coupled rows) | None | Zero | Preserves unrelated-domain aliases; subset choice needs its own evidence |
| C | **Symmetric-source gating** | Only expand when the *source token is present verbatim in the routing text* (i.e. rollback qualifies via its own `release`, and the prompt's `deploy` may then match) — expansion becomes text-anchored rather than table-anchored | Subtle: changes skill-tier behavior too if applied globally | Low but nonzero: skill-tier fixtures relying on cross-domain expansion (none found in §3) | Most conservative semantically; hardest to justify without a corpus |
| D | **Defer with a guard test** | Ship nothing; add the missing cross-talk fixture to `chunk_routing_eval.rs` asserting current behavior is *known and pinned* | None | Zero | Blocks silent behavior drift; defers the decision honestly |

Recommendation: **A + D now, B never without new evidence.** Option A is the
minimal, engine-preserving decoupling: the chunk tier's pool stops
inheriting expansion it never authored, the skill tier is untouched, and §3
proves no shipped assertion depends on chunk-side expansion. Option D is its
prerequisite anyway — the missing cross-talk fixture must exist *before* any
remediation lands so the change is observable as a deliberate delta, not a
silent drift. Any change that alters *skill-tier* expansion (C) or invents
alias subsets (B) is a new hypothesis requiring its own PRD-scale eval —
out of scope for a hardening PR.

## 5. Follow-up defect classes found during recon (no code changed)

1. **Stem-form sensitivity of alias firing (confirmed empirically):**
   `releases → releas` (stem strips `es` before `s`), so the alias row at
   1385 never fires for the plural, while `deploys → deploy` fires correctly
   for the `-s` plural. Any future alias row keyed on an `-e`-final word
   inherits this asymmetry. The alias table should be expressed in stem
   space, or rows should avoid `-e`-final sources.
2. **Dead alias rows (1378, and the hyphenated `pull-request` side):**
   hyphenated sources can never be produced by the tokenizer (1294). Rows
   should use stem-space, non-hyphenated keys or the row is unreachable.
3. **Cosine noise floor loads unrelated chunks:** disjoint pools still
   receive a small positive expected cosine (§2.3), so `score > 0` (885)
   is a weaker filter than intended; unrelated chunks can qualify on hash
   noise alone when their routing text is long. Any remediation should
   consider whether the chunk tier wants a higher qualification floor
   (e.g. requiring overlap > 0, not merely score > 0).
4. **`phrase_matches` is unanchored (1349–1352):** chunk name `logs`
   matches "cata**logs**"; independent of aliases but the same class of
   incidental match.

## 6. Verification

- `cargo xtask naming-guard` — clean, 18 hits, all frozen in baseline (zero
  erosion; this document adds none)
- `git status --porcelain` — exactly one new untracked file (this document
  and the companion foundation report); zero modifications to existing
  files (read-only scope held)
- Anchor lines re-read at synthesis time; stem behavior verified by
  standalone compilation (`rustc` single-file harness) rather than asserted

## 7. Status

Findings complete. Remediation (Option A + D) is **not implemented** — this
recon recommends it as a scoped follow-up PR with its own eval and the
missing cross-talk fixture as its acceptance anchor.
