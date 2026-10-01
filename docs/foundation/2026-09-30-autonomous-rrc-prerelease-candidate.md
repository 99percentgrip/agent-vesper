# Autonomous RRC Prerelease Candidate Build — Execution Report

## Objective

Build fresh Linux x86-64 release-profile `agent-vesper-tui` and `agent-vesper-acp` binaries from the exact current `rrc-autonomy-repair` working-tree snapshot, preserve them outside every Cargo `target/` directory, and record immutable artifact and source-provenance identities for Alex-operated manual acceptance. Do not release, bump a version, tag, push, install, or execute the manual acceptance.

## Source snapshot

- Source worktree: `/home/Alex/Projects/agent-vesper/.worktrees/rrc-autonomy-repair`
- Branch: `rrc-autonomy-repair-20260930`
- Source HEAD: `01d2df045a7c30253bea8e613e9840b9f62d053b`
- Tracked diff SHA-256: `548a6d9ae62db59dccf36dc887fb3155b7fe81e0e48901908d81f093d82b26d3`
- Status inventory SHA-256: `4e112630f926240bb34cea968b83479598bad976d223019f99ed77403e9c7542`

The tracked-diff digest is the SHA-256 of raw `git diff --binary HEAD -- .` bytes. The status-inventory digest is the SHA-256 of raw `git status --porcelain=v2 -z --untracked-files=all` bytes. Both were captured before compilation and recomputed unchanged immediately after artifact preservation. This report and its index/PRD links were written only after that invariant check, so they are not build inputs and do not alter the recorded candidate provenance.

## Methods and commands

A new empty external Cargo target directory forced a fresh release-profile compilation while leaving the source worktree's `target/` unused:

```text
CARGO_TARGET_DIR=/tmp/agent-vesper-rrc-fresh-build-20260930T004057Z-01d2df045a7c \
CARGO_INCREMENTAL=0 \
  cargo build --release -p agent-vesper-tui -p agent-vesper-acp
```

The resulting binaries were copied—not installed—to:

```text
/home/Alex/Projects/agent-vesper-prerelease-candidates/rrc-autonomy-repair-20260930T004057Z-01d2df045a7c/
```

Artifact verification commands:

```text
sha256sum agent-vesper-tui agent-vesper-acp
agent-vesper-tui --version
agent-vesper-acp --version 2>&1
file agent-vesper-tui agent-vesper-acp
sha256sum -c SHA256SUMS
```

Source invariance was checked with:

```text
git status --porcelain=v2 -z --untracked-files=all | sha256sum
git diff --binary HEAD -- . | sha256sum
git rev-parse HEAD
```

## Files and artifacts

Persistent candidate directory:

`/home/Alex/Projects/agent-vesper-prerelease-candidates/rrc-autonomy-repair-20260930T004057Z-01d2df045a7c`

It contains:

- `agent-vesper-tui`
- `agent-vesper-acp`
- `CANDIDATE-MANIFEST.txt`
- `SHA256SUMS`
- `source-head.txt`
- `tracked-diff.patch`
- `tracked-diff.sha256`
- `status-inventory.porcelain-v2-z`
- `status-inventory.sha256`
- `tui-version.txt`
- `acp-version.txt`

Documentation closeout files written after the source-invariance checkpoint:

- `docs/foundation/2026-09-30-autonomous-rrc-prerelease-candidate.md`
- `docs/foundation/evidence-index.md`
- `docs/foundation/AGENTS.md`
- `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md`

No production source file was edited by this build work unit.

## Exact evidence

Fresh build receipt:

```text
Finished `release` profile [optimized] target(s) in 1m 55s
```

TUI artifact:

```text
Path:    /home/Alex/Projects/agent-vesper-prerelease-candidates/rrc-autonomy-repair-20260930T004057Z-01d2df045a7c/agent-vesper-tui
SHA-256: 99630923449bcaa07be0004e9678436187b8f7dd1357cea01559b22561ff262a
Version: agent-vesper-tui 0.24.4
File:    ELF 64-bit LSB pie executable, x86-64, dynamically linked, stripped
```

ACP artifact:

```text
Path:    /home/Alex/Projects/agent-vesper-prerelease-candidates/rrc-autonomy-repair-20260930T004057Z-01d2df045a7c/agent-vesper-acp
SHA-256: 04e7cdfa7c1c280f5c97487e0f4d5f0d980f7bcfcb96f5002bbfd4bd29030315
Version: agent-vesper-acp 0.24.4
File:    ELF 64-bit LSB pie executable, x86-64, dynamically linked, stripped
```

Checksum verification:

```text
agent-vesper-tui: OK
agent-vesper-acp: OK
```

Pre/post-build provenance identity:

```text
SOURCE_HEAD=01d2df045a7c30253bea8e613e9840b9f62d053b
TRACKED_DIFF_SHA256=548a6d9ae62db59dccf36dc887fb3155b7fe81e0e48901908d81f093d82b26d3
CURRENT_DIFF_SHA256=548a6d9ae62db59dccf36dc887fb3155b7fe81e0e48901908d81f093d82b26d3
STATUS_INVENTORY_SHA256=4e112630f926240bb34cea968b83479598bad976d223019f99ed77403e9c7542
CURRENT_STATUS_INVENTORY_SHA256=4e112630f926240bb34cea968b83479598bad976d223019f99ed77403e9c7542
source head unchanged: yes
```

## Included behavior confirmation

Current source inspection and the already-recorded focused regression receipts in [`2026-09-30-release-intent-autonomy-repair.md`](2026-09-30-release-intent-autonomy-repair.md) establish that this exact candidate build includes:

- **Natural-language release intent admission:** `classify_release_intent` admits direct ordinary-language imperatives, including `Release all completed RRC work as the next patch.`, while failing closed for questions, negation, deferred requests, and release-process discussion.
- **Automatic clean release-worktree selection:** `resolve_release_source` searches repository worktrees for one maximal clean completed descendant, and `create_release_worktree` creates and validates a clean detached execution workspace.
- **Dirty primary workspace preservation:** dirty active worktrees are never selected as mutation workspaces; the regression preserves exact private dirty bytes.
- **Ambiguity clarification:** multiple maximal completed candidates return one bounded clarification before creating controller state, a worktree, or a worker.
- **Automatic RRC progression:** successful admission persists `LocalVerification`, launches `spawn_release_worker_with_factory`, and reports that local verification started automatically.
- **No slash-command requirement:** both TUI and ACP intercept admitted free text before provider dispatch; `/release`, manual `cd`, worktree selection, and `/release resume` are not required for the normal release request.

These are source/regression confirmations, not a claim that Alex's manual acceptance has passed.

## Constraints held

- No release or publication was started.
- No version was changed; both binaries report `0.24.4`.
- No tag was created.
- No branch or production change was pushed.
- No binary or dependency was installed.
- No installer was run.
- The manual acceptance phrase was not submitted to either candidate.
- No release state or isolated acceptance worktree was created by this build work unit.
- The ordinary primary checkout was read only for discovery; it was not reset, cleaned, stashed, committed, or otherwise mutated.

## Deviations

- `agent-vesper-acp --version` writes its version line to stderr, so version capture used `2>&1`; this did not start ACP protocol service or mutate state.
- Full local verification and cross-platform CI were not rerun for this artifact-only build. The exact source snapshot already carries focused host/harness and `cargo xtask acceptance` receipts in the repair report, while Alex's requested manual acceptance remains deliberately unexecuted.
- Documentation closeout necessarily changed documentation inventory after the candidate snapshot was sealed. The returned tracked-diff and status-inventory hashes remain the build-input identities, and the preserved raw snapshot files make that boundary explicit.

## Unresolved items

- Alex-operated manual acceptance is **NOT RUN**. Its expected natural-language input remains: `Release all completed work as the next patch.`
- No publication authorization exists. Even if manual acceptance passes, the real release must wait for Alex.
- This is a Linux x86-64 candidate build; no fresh five-target artifact matrix was requested or executed.

## Readiness effect

The exact prerelease candidates are preserved and identity-verified for Alex's manual test from his normal dirty primary checkout. Artifact construction is complete. Release readiness is still gated by Alex's unexecuted manual acceptance and subsequent explicit authorization; this report does not authorize or claim a release.
