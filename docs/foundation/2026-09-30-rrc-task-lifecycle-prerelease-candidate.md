# RRC task-lifecycle prerelease candidate — build report

**Date:** 2026-09-30  
**Status:** FRESH RELEASE-PROFILE TUI/ACP CANDIDATES BUILT AND IDENTITY-VERIFIED; LIVE ACCEPTANCE NOT RUN; NO RELEASE PERFORMED

## Objective

Build fresh Linux x86-64 release-profile `agent-vesper-tui` and `agent-vesper-acp` binaries from the exact current repaired source in `/home/Alex/Projects/agent-vesper/.worktrees/rrc-autonomy-repair`, preserve them outside `target/`, and prove that the TUI differs from the rejected byte-identical candidate.

## Constraints held

- No release, publication, tag, push, installation, or installer execution.
- No live release-intent acceptance test was executed.
- No VRO-19 source, documentation, research, or acceptance work.
- The dirty primary checkout was not cleaned, reset, stashed, or used as a build source.
- Build output used a new external Cargo target directory.
- Candidate binaries were copied byte-for-byte into a new external persistent directory.

## Exact source identity

Captured immediately before the fresh build and rechecked after it:

```text
SOURCE HEAD: 01d2df045a7c30253bea8e613e9840b9f62d053b
TRACKED DIFF SHA-256: 74b79361fdf24dccee6caee531dcc9228caea9764a175a0b63b6dab46ec5faed
STATUS INVENTORY SHA-256: 86589fb2dc69039d87fb2b2734804eba49510127e6e2e482862932d193f7476c
```

Identity methods:

```sh
git rev-parse HEAD
git diff --binary HEAD -- . | sha256sum
git status --porcelain=v1 -uall | sha256sum
```

The pre-build and post-build values were byte-identical. The tracked-diff digest covers tracked working-tree changes relative to `HEAD`; the status digest binds the complete tracked/untracked path inventory. Existing untracked evidence files are represented by the status inventory but, as expected, are not Rust compiler inputs.

## Build method

```sh
CARGO_TARGET_DIR=/home/Alex/Projects/agent-vesper-prerelease-builds/rrc-task-lifecycle-20260930T042554Z \
  cargo build --release --locked \
  -p agent-vesper-tui --bin agent-vesper-tui \
  -p agent-vesper-acp --bin agent-vesper-acp
```

Receipt:

```text
Finished `release` profile [optimized] target(s) in 2m 01s
```

The target directory did not exist before this command, so dependencies and both application binaries were compiled into a fresh external target tree.

## Preserved artifacts

Persistent directory:

`/home/Alex/Projects/agent-vesper-prerelease-candidates/rrc-task-lifecycle-20260930T042554Z`

```text
TUI PATH: /home/Alex/Projects/agent-vesper-prerelease-candidates/rrc-task-lifecycle-20260930T042554Z/agent-vesper-tui-rrc-task-lifecycle
TUI SHA-256: 8f15fa72e4288668043c188007188f94b1da7f29ed6d62886ecaf4409ff7fefd

ACP PATH: /home/Alex/Projects/agent-vesper-prerelease-candidates/rrc-task-lifecycle-20260930T042554Z/agent-vesper-acp-rrc-task-lifecycle
ACP SHA-256: de49869ac53de856c6d1c9025c5a0eb3d881434196ec346a1713bc25d12ae67b
```

`cmp -s` verified each preserved file is byte-for-byte identical to its fresh external-target build output. `sha256sum -c SHA256SUMS` returned:

```text
agent-vesper-tui-rrc-task-lifecycle: OK
agent-vesper-acp-rrc-task-lifecycle: OK
```

Executable identities:

```text
agent-vesper-tui-rrc-task-lifecycle: ELF 64-bit LSB pie executable, x86-64, dynamically linked, stripped; BuildID c15217070f0336f010e934ea6d051476ef13417f
agent-vesper-acp-rrc-task-lifecycle: ELF 64-bit LSB pie executable, x86-64, dynamically linked, stripped; BuildID a29e6da5aa085f93173cb70915fd043624436a9b
TUI_VERSION=agent-vesper-tui 0.24.4
ACP_VERSION=agent-vesper-acp 0.24.4
```

## Rejected-candidate comparison

```text
Rejected TUI SHA-256: 99de2f87e50abd696bac8b204539ec752f14ce9a41370ef8d44af7516c545f01
New TUI SHA-256:      8f15fa72e4288668043c188007188f94b1da7f29ed6d62886ecaf4409ff7fefd
Equal: NO
```

The new TUI is not byte-for-byte identical to the rejected candidate.

## Required behavior inclusion trace

The source snapshot compiled into these artifacts contains:

1. **Registry-retained controller task ownership** — `ActiveReleaseWorker` is stored in the process-wide repository-keyed worker registry.
2. **Retained thread/join handle** — `ActiveReleaseWorker::_handle: thread::JoinHandle<()>` remains owned until registry removal at worker settlement.
3. **Registry-owned cancellation token** — `ActiveReleaseWorker::cancelled: Arc<AtomicBool>` is the token signalled by `/release cancel`.
4. **Start barrier before worker execution** — a zero-capacity `sync_channel::<()>(0)` blocks the worker until registry insertion; `start_tx.send(())` admits execution afterward.
5. **Started response after successful registration** — natural-language admission calls the launcher successfully before constructing `NaturalReleaseAdmission::Started` and the `Local verification started in an isolated release workspace.` message.
6. **Runtime/TUI activity from real RRC state** — `active_release_worker_for_workspace` supplies `ViewModel.background_task`; renderer state, TODO, Run, and screen-reader presentation use that snapshot and reject contradictory idle labels.
7. **Observable local-verification child activity** — the real worker calls `advance_release`, persists a local gate as `Running`, executes `NativeReleaseExecutor::run_local_gate`, and updates registry activity from the persisted gate. The production-path regression waits for a real child-command marker before inspecting state.
8. **Stale-epoch reconciliation** — `reconcile_active_epoch` resumes matching safe stages or atomically archives and supersedes obsolete same-objective epochs.
9. **Autonomous source resolution** — `resolve_release_source` selects validated completed-objective provenance and `create_release_worktree` creates the clean detached release workspace without modifying the dirty primary checkout.
10. **Complete atomic version mutation** — `build_version_mutation_plan`, `apply_version_mutation_plan`, `restore_version_mutation`, and `validate_version_mutation` inventory and preflight workspace manifests/Registry data, apply all mutations transactionally, regenerate the lockfile, validate, and restore original bytes on failure.

Candidate-local receipts preserving this trace:

- `source-identity.txt`
- `post-build-source-identity.txt`
- `source-inclusion.txt`
- `tui-string-proof.txt`
- `artifact-identity.txt`
- `file-identities.txt`
- `version-proof.txt`
- `SHA256SUMS`

The stripped TUI still contains both the exact admission sentence and the `vesper-release-controller` worker name, independently confirming those compiled paths are present. Source-level behavior is governed by the production-path and renderer regressions recorded in `2026-09-30-rrc-task-lifecycle-repair.md` and the 36-case acceptance gate.

## Files created or updated

- External build tree: `/home/Alex/Projects/agent-vesper-prerelease-builds/rrc-task-lifecycle-20260930T042554Z/`
- External candidate tree: `/home/Alex/Projects/agent-vesper-prerelease-candidates/rrc-task-lifecycle-20260930T042554Z/`
- This build report.
- `docs/foundation/evidence-index.md` link and candidate verdict.
- `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md` evidence link.
- Applicable documentation DOX ownership indexes.

No production source was changed by this build work unit.

## Deviations

- Alex's live acceptance scenario was deliberately not executed.
- No cross-platform candidate was built; these are local Linux x86-64 artifacts.
- No release workflow, GitHub operation, Registry operation, installer, or local installation was run.
- The source is a dirty, identity-recorded repaired worktree rather than a clean commit. The exact `HEAD`, tracked diff digest, and complete status inventory digest bind that build input state.

## Unresolved items

- Alex must run the exact preserved TUI from the normal dirty primary checkout and submit only `Release all completed work as the next patch.` for live acceptance.
- Live acceptance remains distinct from local compile, source inclusion, and automated lifecycle evidence.
- Publication and release gates remain unexecuted and unauthorized.

## Closeout receipts

```text
agent-vesper-tui-rrc-task-lifecycle: OK
agent-vesper-acp-rrc-task-lifecycle: OK
DOX_AND_ARTIFACT_CLOSEOUT=pass
PRIMARY_HEAD=0b5630d271965e7d9df3a0a09c116c7f8c44042e
PRIMARY_STATUS_SHA256=1261528c0926297bde8d3026c24c7797e3299e03b6d6895aab24945b0ceeca1e
```

Closeout included `git diff --check`, report/index/PRD/DOX link checks, another `sha256sum -c`, and the rejected-hash inequality assertion. The primary status digest matches the pre-build lifecycle-repair closeout receipt, confirming this build unit did not change the primary checkout inventory.

## Readiness effect

A fresh, identity-bound prerelease TUI/ACP pair now exists for Alex-operated live RRC acceptance. The new TUI differs from the rejected candidate and includes the registered-task lifecycle repair plus the previously completed stale-epoch, source-resolution, and atomic version-mutation repairs. This report does not authorize or claim a release.
