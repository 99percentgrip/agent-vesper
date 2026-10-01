# RRC Version-Mutation Prerelease Candidate — Execution Report

## Objective

Build fresh Linux x86-64 release-profile `agent-vesper-tui` and `agent-vesper-acp` binaries from the exact current RRC version-mutation repair source, preserve them outside Cargo `target/`, and provide immutable artifact and source identities for Alex-operated live acceptance. Do not execute the manual acceptance, release, publish, tag, push, install, manually bump production `main`, or touch VRO-19.

## Source snapshot

- Source: `/home/Alex/Projects/agent-vesper/.worktrees/rrc-autonomy-repair`
- Source HEAD: `01d2df045a7c30253bea8e613e9840b9f62d053b`
- Tracked diff SHA-256: `f8360122e4cc1e77c7d744317e3e517cd2a204d41bc9d596dbafe9db19d74ec7`
- Status inventory SHA-256: `970e1c31640d7282dc9088e515ab05786e58e979ab93e1c55f01482dee923239`

The tracked-diff digest is the SHA-256 of raw `git diff --binary HEAD -- .` bytes. The status-inventory digest is the SHA-256 of raw `git status --porcelain=v2 -z --untracked-files=all` bytes. Both were captured before compilation and recomputed unchanged after binary preservation. This report and its documentation links were written only after sealing that build-input identity.

## Methods and commands

A new external target directory forced a fresh release compilation:

```text
CARGO_TARGET_DIR=/tmp/agent-vesper-rrc-version-mutation-build-20260930T014153Z-01d2df045a7c
CARGO_INCREMENTAL=0
cargo build --release -p agent-vesper-tui -p agent-vesper-acp
```

The binaries were copied outside `target/` to:

```text
/home/Alex/Projects/agent-vesper-prerelease-candidates/rrc-version-mutation-20260930T014153Z-01d2df045a7c/
```

Verification used:

```text
sha256sum agent-vesper-tui agent-vesper-acp
sha256sum -c SHA256SUMS
agent-vesper-tui --version
agent-vesper-acp --version 2>&1
file agent-vesper-tui agent-vesper-acp
git rev-parse HEAD
git diff --binary HEAD -- . | sha256sum
git status --porcelain=v2 -z --untracked-files=all | sha256sum
```

Bounded source-presence checks confirmed the exact snapshot contains `classify_release_intent`, `resolve_release_source`, `spawn_release_worker_with_factory`, `VersionMutationPlan`, `generated_lockfile`, `update_registry_manifest`, and `restore_version_mutation`. The live acceptance phrase was not submitted.

## Files and artifacts

Persistent candidate directory:

`/home/Alex/Projects/agent-vesper-prerelease-candidates/rrc-version-mutation-20260930T014153Z-01d2df045a7c`

Contents:

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

## Exact evidence

Build receipt:

```text
Finished `release` profile [optimized] target(s) in 1m 54s
```

TUI:

```text
Path: /home/Alex/Projects/agent-vesper-prerelease-candidates/rrc-version-mutation-20260930T014153Z-01d2df045a7c/agent-vesper-tui
SHA-256: d22670002752b37954d60bc360f70a633d4f3c507a47f7da33effbd6762f02de
Version: agent-vesper-tui 0.24.4
File: ELF 64-bit LSB pie executable, x86-64, dynamically linked, stripped
```

ACP:

```text
Path: /home/Alex/Projects/agent-vesper-prerelease-candidates/rrc-version-mutation-20260930T014153Z-01d2df045a7c/agent-vesper-acp
SHA-256: 02d7329cac27fca00a089386766d3e965f2020df8f135f50bb34dc68294f0b70
Version: agent-vesper-acp 0.24.4
File: ELF 64-bit LSB pie executable, x86-64, dynamically linked, stripped
```

Checksum verification:

```text
agent-vesper-tui: OK
agent-vesper-acp: OK
```

Final closeout audit:

```text
agent-vesper-tui: OK
agent-vesper-acp: OK
agent-vesper-tui 0.24.4
agent-vesper-acp 0.24.4
FINAL_HEAD=01d2df045a7c30253bea8e613e9840b9f62d053b
CURRENT_TRACKED_DIFF=9dca180f98c63790178e700f7e1151ed39f4c4abc9cc92667cc7710175d1b151
FINAL_AUDIT=PASS
```

The current tracked-diff digest differs from the sealed pre-build digest only
because tracked documentation closeout linked this report after compilation;
the sealed raw patch and status inventory remain beside the binaries.

Source invariance:

```text
CURRENT_HEAD=01d2df045a7c30253bea8e613e9840b9f62d053b
CURRENT_DIFF=f8360122e4cc1e77c7d744317e3e517cd2a204d41bc9d596dbafe9db19d74ec7
CURRENT_STATUS=970e1c31640d7282dc9088e515ab05786e58e979ab93e1c55f01482dee923239
```

## Included behavior confirmation

Because these binaries were freshly compiled from the sealed repaired snapshot, they include:

- natural-language release admission in both hosts before provider dispatch;
- autonomous objective source and isolated release-worktree resolution;
- automatic background Release Recovery Controller progression;
- complete typed workspace version mutation through `VersionMutationPlan`;
- fail-closed, transactional propagation of all workspace-owned exact internal dependency pins;
- `Cargo.lock` regeneration after manifest and Registry mutation;
- `registry/agent.json` version and archive-URL propagation;
- restoration of planned files and the original lockfile after mutation, Cargo-preflight, or postcondition failure.

These are source/build confirmations. Alex's manual live acceptance remains unexecuted.

## Constraints held

- No release or publication was started.
- No tag or push was performed.
- No installer or installation was run.
- Production `main` was not manually version-bumped.
- VRO-19 was not touched.
- The live phrase `Release all completed work as the next patch.` was not submitted.
- No `/release` command, manual release-worktree `cd`, worktree selection, or `/release resume` was executed.

## Deviations

- `agent-vesper-acp --version` writes its version line to stderr, so capture used `2>&1`; it did not enter ACP service mode.
- The first checksum verification invocation ran outside the artifact directory and therefore reported missing relative files. It changed nothing. The command was rerun inside the candidate directory and both checks returned `OK`.
- During final closeout, an additional local rebuild check first used the binary name `agent-vesper` as a package selector; Cargo rejected it because the package is named `agent-vesper-tui`. The corrected locked release build for `agent-vesper-tui` and `agent-vesper-acp` completed in `1m 22s`, and reproduced both recorded artifact hashes. This follow-up did not replace the preserved candidate.
- Documentation closeout changes the later working-tree inventory. The returned provenance hashes identify the exact pre-build source snapshot and are preserved as raw files beside the binaries.

## Unresolved items

- Alex-operated live acceptance is **NOT RUN**.
- Publication remains unauthorized and unexecuted.
- This is a Linux x86-64 prerelease candidate; no new cross-platform artifact matrix was requested.

## Readiness effect

The fresh TUI and ACP candidates are preserved and identity-verified for Alex's manual test from his ordinary dirty primary checkout. This build does not authorize publication and does not claim that the manual acceptance has passed.
