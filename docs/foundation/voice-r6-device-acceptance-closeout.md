# VRO-17 R6 Device Acceptance Closeout

Work unit: 2026-09-24. Documentation and acceptance reconciliation only.
PR-5 was not started.

## Objective

Close R6 from Alex's completed real-device interruption/recovery test, preserve
the latest accepted voice-quality and pronunciation baseline, reconcile stale
R6 status text, and re-derive the remaining VRO-17 closure set without changing
production code or rerunning implementation/device/release gates.

## Baseline and preservation

- Branch: `main`.
- Revision: `8f258ba28f4ea2f749526fb32b5b180ecc7dcead`.
- Working tree: already dirty with the uncommitted VRO-17 implementation,
  documentation, and unrelated `.flm-loop-*` artifacts.
- Before editing, the expanded Git status contained 1,431 non-document dirty or
  untracked file records. Their path/status/content snapshot was stored outside
  the repository at `/tmp/vro17-r6-closeout-nondocs-before.json`, digest
  `1928e4363ed8a7cc14e2a47de7849ca98949400d675a55a933b0159ca372fb1b`.
- This unit edits documentation only. Existing production and unrelated work is
  preserved byte-for-byte and compared again at closeout.

## Evidence used

Current owning records were read before editing:

- `docs/voice-oracle-extraction-prd.md`;
- `voice-vro17-final-completion-audit.md`;
- `voice-r6-device-interruption-acceptance.md`;
- `voice-r6-binding-repair.md`;
- `voice-short-reply-quality-repair.md`;
- `voice-kokoro-pronunciation-investigation.md` and the earlier continuity/
  phoneme repair record;
- `evidence-index.md` and `docs/migration-status.md`;
- the root, documentation, and foundation DOX contracts.

Later evidence supersedes the withdrawn pre-repair acceptance brief. Historical
records remain in place and are labeled as historical rather than deleted.

## Alex's R6 device evidence

| Check | Result |
|---|---|
| A baseline | **PASS** |
| B one-press barge-in | **PASS** |
| B old speech stopped | **PASS** |
| B new turn exactly once | **PASS** |
| B old speech resumed later | **NO** |
| C repeated barge-in | **PASS** |
| D explicit Stop / Ctrl+C | **PASS** |
| D capture started accidentally | **NO** |
| Later F9 remained usable | **PASS** |

Alex explicitly clarified that old speech did not resume, Ctrl+C stopping speech
was correct, and the cancellation message was acceptable in the tested case.
The earlier interpretation of those observations as an R6 failure is
superseded and must not be reused by later audits.

## R6 contract reconciliation

The user-visible device evidence and the existing automated production-entry
evidence jointly satisfy §2.4:

- **Barge-in:** playback stops; synthesis generation is invalidated; runtime
  cancellation is requested through the normal generic path; replacement
  capture opens in the same F9 gesture; exactly one replacement turn is
  submitted; old speech does not resume; repeated interruption recovers.
- **Explicit Stop:** playback and synthesis stop; runtime cancellation uses the
  normal path; no replacement capture opens; later manual F9 still works.
- **Provider neutrality:** voice submission continues through the generic
  runtime/provider path. Existing two-provider automated evidence finds no
  reasoning-provider-specific voice branch.

The device run establishes the tested observable behavior. Internal synthesis
invalidation and runtime-cancel dispatch come from the binding repair's exact
production-entry regression. The evidence does not claim that an interruption
note was audible, prove tool-replay behavior by listening, or cover every
provider or platform.

## Voice-quality and pronunciation acceptance

Alex's latest short-reply result was **“Looks like smooth.”** This is bounded
user acceptance on the tested setup. It is not a numeric acoustic threshold,
universal gap-free claim, or cross-platform acceptance.

The accepted audio baseline identified by the current record is:

- path: `target/voice-candidates/agent-vesper-tui-vro17-voice-quality`;
- SHA-256:
  `f3b736f6f757e387d3969d02718aaec28f0e11c4fc4fca6135e0b801df1a2e14`;
- release features: `voice-flm,voice-kokoro`.

The pronunciation repair remains accepted on the tested path, and Alex reported
no new pronunciation regression during the latest smoothness retest. Universal
pronunciation correctness is not claimed. The R6 behavioral observations did
not include a separate executable checksum, so this closeout does not guess one
or silently attach the audio candidate's hash to the interruption run.

## Requirement accounting

The current R1–R20 matrix has 22 verdict rows because R16 has three sub-rows:

| Verdict | Count | Rows |
|---|---:|---|
| PASS | 17 | R1, R2, R3, R4, R6, R8, R9, R10, R11, R12, R13, R15, R16c, R17, R18, R19, R20 |
| PASS — scoped limitation | 4 | R5, R7, R14, R16a |
| Optional/capability-gated — conformant | 1 | R16b |

All R1–R20 requirements are closed at their approved scopes. PR-5 is a separate
phase gate and the only remaining open VRO-17 item.

## Documentation changes

- `voice-r6-device-interruption-acceptance.md` — final device matrix, Alex's
  clarification, bounded PASS verdict, and historical/superseded markers.
- `voice-r6-binding-repair.md` — subsequent device-acceptance pointer; its
  implementation-unit verdict remains dated history.
- `voice-short-reply-quality-repair.md` and
  `voice-kokoro-pronunciation-investigation.md` — bounded later user acceptance.
- `voice-vro17-final-completion-audit.md` — R6 PASS, corrected requirement
  accounting, R1–R20 closure and PR-5-only remaining set.
- `docs/voice-oracle-extraction-prd.md` — current status, D27 and traceability.
- `docs/migration-status.md`, `evidence-index.md`, and foundation DOX — current
  discoverability and ownership.
- This report — formal work-unit execution record.

## Verification

Only bounded documentation checks are run:

- changed-file scope: **PASS** — the before/after non-document snapshot is
  byte-identical across 1,431 records with unchanged digest
  `1928e4363ed8a7cc14e2a47de7849ca98949400d675a55a933b0159ca372fb1b`;
- relative Markdown links/fragments in the closeout edits: **PASS**;
- changed-document heading/anchor uniqueness: **PASS**;
- trailing whitespace: **PASS**; `git diff --check`: **PASS**;
- R1–R20 accounting: **PASS** — all 22 named matrix rows present and
  `17 + 4 + 1 = 22`; PR-5 separate;
- stale current-status assertions: **PASS** — the PRD, audit, acceptance record
  and migration row agree that R6 is PASS and PR-5 alone remains open;
- accepted audio artifact identity: **PASS** — SHA-256 re-read as
  `f3b736f6f757e387d3969d02718aaec28f0e11c4fc4fca6135e0b801df1a2e14`.

No build, program test, device/NPU workload, provider call, installer, release
workflow or PR-5 action is part of this closeout.

## DOX closeout

`docs/foundation/AGENTS.md` now owns this closeout and the current acceptance
status. The root `AGENTS.md` and `docs/AGENTS.md` were intentionally left
unchanged because this unit changes no repository-wide behavior, user
preference, documentation boundary or child index. Application/crate DOX files
were also left unchanged because production behavior and contracts did not
change.

## Deviations and unresolved items

- No production-code deviation occurred.
- The exact executable identity for the R6 behavioral run was not separately
  supplied; the bounded behavior is accepted without guessing that identity.
- The accepted quality hash applies to the short-reply audio baseline.
- PR-5 remains open: ACP voice decision/exclusion, parity evidence, committing
  the current tree, exact-commit CI/cross-target/release gates, user docs and tag.

## Readiness effect

**R6: PASS on the tested setup.** Real-device barge-in, repeated interruption,
explicit Stop, and post-interruption recovery are user-confirmed. Old canceled
speech did not resume, replacement turns submitted exactly once, and later F9
voice remained usable. R1–R20 are closed at approved scope. VRO-17 remains open
only on PR-5, which was not started.
