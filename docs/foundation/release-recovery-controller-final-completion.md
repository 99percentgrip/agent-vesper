# Release Recovery Controller — final completion and PRD audit

**Date:** 2026-09-29
**Status:** COMPLETE WITHIN THE AUTHORIZED NON-PUBLISHING SCOPE
**Candidate tested:** `60eacc5c8ef53f4dba1b4eed7d274fa26ec95284`
**Controlled repository:** private `99percentgrip/agent-vesper-rrc-acceptance`

## Objective

Close the four remaining Release Recovery Controller (RRC) blockers without
reopening accepted PR-1–PR-7 foundations unless inspection found a concrete
contract defect:

1. runtime-observed bounded AgentLoop repair handoff;
2. epoch-bound recovery worktree and fail-closed promotion;
3. controlled live GitHub acceptance;
4. native five-target lifecycle/process acceptance.

Then re-audit every PRD acceptance criterion, run repository verification and
complete DOX/evidence closeout. The authorized boundary prohibited publishing a
release, modifying Agent Vesper `main`, tags, releases, registry state or
credentials, installing a build, or touching VRO-19.

## Implementation and audit methods

- Re-read the root, `.github`, `crates`, `crates/vesper-harness`, `apps`, `docs`,
  `docs/foundation` and `xtask` DOX contracts applicable to changed paths.
- Inspected the RRC reducer, executor, ledger, repair worker, TUI/ACP routes,
  exact-SHA retry behavior, controlled workflow and prior PR-1–PR-7 evidence.
- Reused the accepted controller architecture. One concrete defect was found:
  `NativeReleaseExecutor::command` killed only its direct child, not the owned
  process tree required by cancellation semantics.
- Replaced direct spawning with `command-group` ownership: POSIX process groups
  on Unix and a Job Object on Windows. Cancellation kills the owned group/job and
  waits for the leader before returning a truthful cancellation error.
- Added
  `release_worker_cancel_restart_process_acceptance`, which starts a real child
  and descendant, waits for the descendant-start receipt, cancels the release
  command, rejects a delayed descendant-survival canary, persists an in-progress
  gate with exact run/job IDs, and starts a fresh process that reloads the same
  epoch/run/job identity.
- Enrolled that test in `cargo xtask acceptance`, the normal five-target
  foundation workflow, and a read-only manually dispatched private acceptance
  matrix.
- Created an isolated candidate commit from the intended RRC files because the
  primary workspace already had unrelated artifacts and a pre-existing Git
  sequencer. No unrelated file was deleted, staged or attributed to this work.
- Pushed only candidate branch `rrc-lifecycle-60eacc5` to the authorized private
  acceptance repository. Added the read-only dispatcher to that repository's
  `main`; no Agent Vesper branch was pushed.

## Commands and exact receipts

### Regression-first final audit

The lifecycle regression was run against a temporary pre-fix variant that kept
all test logic but restored direct `Command::spawn` plus direct-child kill/wait:

```text
preaudit_exit=101
release cancellation leaked a descendant process
FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 156 filtered out; finished in 4.35s
```

The production fix then passed locally:

```text
cargo test -p vesper-harness release_executor::tests::release_worker_cancel_restart_process_acceptance -- --exact --test-threads=1 --nocapture

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 156 filtered out; finished in 4.36s
```

### Focused RRC verification

```text
cargo test -p vesper-harness --lib release_
31 passed; 0 failed

cargo test -p vesper-harness --test release_repair_dispatch --test release_repair_retry
release_repair_dispatch: 1 passed; 0 failed
release_repair_retry: 1 passed; 0 failed

cargo clippy -p vesper-harness --all-targets -- -D warnings
Finished `dev` profile ...

cargo test -p vesper-harness --doc
0 failed
```

### Acceptance and complete repository verification

```text
cargo xtask acceptance
Acceptance regression gate: 30 exact cases passed in 12175 ms.
Offline fixture model cost: zero; live-model effectiveness is not measured.

cargo xtask verify
exit 0
```

The complete `verify` receipt includes `cargo fmt --all --check`, workspace
all-target/all-feature Clippy with `-D warnings`, the full workspace test suite,
fixtures/contracts/provider/runtime/ACP/session verification, architecture and
acceptance. Expected environment-gated container tests remained ignored by their
existing contracts; no failed test was hidden.

### Controlled real GitHub evidence

The already-preserved read-only PR-7 workflow evidence remains current and
scope-appropriate at controlled commit
`e149b2e0f57ec9962ad56e217b1a5571b9fb1c23`:

- complete-matrix red: run `36586801959`;
- focused-repair green: run `36586812935`;
- same-fingerprint red: run `36586826432`;
- post-release-main red: run `36586840308`.

All five logical jobs settled in each run, and the controlled repository retained
zero tags and zero releases.

### Five-target native lifecycle/process evidence

Manual read-only workflow:

```text
repository: 99percentgrip/agent-vesper-rrc-acceptance (private)
workflow commit on controlled main: 119dbf62c9331cb03436ecb698b4a44a1b4bf11c
candidate branch: rrc-lifecycle-60eacc5
candidate SHA: 60eacc5c8ef53f4dba1b4eed7d274fa26ec95284
run: 36594339290
conclusion: success
URL: https://github.com/99percentgrip/agent-vesper-rrc-acceptance/actions/runs/36594339290
```

| Native target | Job ID | Conclusion |
|---|---:|---|
| Windows x86_64 | `109495271470` | success |
| macOS Apple Silicon | `109495271707` | success |
| Linux x86_64 | `109495271946` | success |
| macOS Intel | `109495272086` | success |
| Linux ARM64 | `109495272101` | success |

#### Provenance clarification

GitHub's run-level `head_sha` is
`119dbf62c9331cb03436ecb698b4a44a1b4bf11c` because `workflow_dispatch` loaded
the workflow definition from the controlled repository's `main` branch at that
fixture commit. It identifies the workflow fixture, not the source left in the
job workspace after `actions/checkout`.

The Agent Vesper source was assembled in an isolated Agent Vesper Git worktree,
committed as `60eacc5c8ef53f4dba1b4eed7d274fa26ec95284`, and pushed to branch
`rrc-lifecycle-60eacc5` in the same private repository. The dispatch input
`candidate_ref` was that exact 40-character SHA. The workflow validated the
input, then every matrix job ran `actions/checkout@v6` with:

```text
ref: 60eacc5c8ef53f4dba1b4eed7d274fa26ec95284
persist-credentials: false
```

`actions/checkout` fetched that commit by SHA, force-checked it out in detached
HEAD state, and independently emitted `git log -1 --format=%H`. The retained log
receipts are:

| Native target | Checkout `git log -1 --format=%H` receipt |
|---|---|
| Windows x86_64 | `60eacc5c8ef53f4dba1b4eed7d274fa26ec95284` at `2026-09-29T15:59:24.3162170Z` |
| macOS Apple Silicon | `60eacc5c8ef53f4dba1b4eed7d274fa26ec95284` at `2026-09-29T15:59:18.1436600Z` |
| Linux x86_64 | `60eacc5c8ef53f4dba1b4eed7d274fa26ec95284` at `2026-09-29T15:59:14.8772441Z` |
| macOS Intel | `60eacc5c8ef53f4dba1b4eed7d274fa26ec95284` at `2026-09-29T15:59:09.5122990Z` |
| Linux ARM64 | `60eacc5c8ef53f4dba1b4eed7d274fa26ec95284` at `2026-09-29T15:59:22.1663552Z` |

Thus all five jobs compiled and tested the same Agent Vesper source SHA;
`119dbf62...` supplied only the dispatchable workflow definition. No replacement
acceptance run is required.

Each job then ran the named lifecycle test body. Each log records both the fresh
child-host pass and the parent pass, including:

```text
test release_executor::tests::release_worker_cancel_restart_process_acceptance ... ok
test result: ok. 1 passed; 0 failed
```

After the run, GitHub API checks returned `0` tags and `0` releases in the
controlled repository. Agent Vesper `origin/main` remained
`ba9e5d163f8ffd9f19033732e390b5f8805f6994`; no production ref was pushed by
this work.

## Committed source and fresh prerelease candidates

The implementation was committed before candidate compilation, as required:

```text
commit: 60eacc5c8ef53f4dba1b4eed7d274fa26ec95284
subject: feat(release): complete recovery controller lifecycle
parent: 0b5630d271965e7d9df3a0a09c116c7f8c44042e
source tree before build: clean
```

Fresh optimized TUI and ACP binaries were then built from that exact commit without
installation or publication:

```text
CARGO_TARGET_DIR=/tmp/agent-vesper-rrc-candidates-60eacc5/target \
  cargo build --release -p agent-vesper-tui -p agent-vesper-acp
Finished `release` profile [optimized] target(s) in 1m 53s
BUILD_EXIT=0
```

| Candidate | Bytes | SHA-256 |
|---|---:|---|
| `agent-vesper-tui` | `24085264` | `d4a0d8ff7d6819604c0220c711d55c63ad8a07a77a868a35ef9c0a8fdccad1c4` |
| `agent-vesper-acp` | `23822480` | `d08b9aa8da9a15e85bdfb0ccf635bb012a34f6e1496d6a79f32d1c39a186c374` |

`sha256sum -c` returned `OK` for both files. Both are x86-64 Linux ELF PIE
executables. The preserved local copies are under
`/tmp/agent-vesper-rrc-candidates-60eacc5/artifacts/`; they are prerelease
candidates, not installed packages or published release assets. The first copy
step expected the obsolete destination name `agent-vesper`; the Cargo manifest's
actual TUI binary name is `agent-vesper-tui`. Packaging was corrected, hashes were
regenerated from the two actual binaries, and no rebuild or source change was
needed.

A post-commit targeted safe-boundary test also passed:

```text
test release_executor::tests::production_orchestrator_reaches_publication_only_through_settled_gates ... ok
test result: ok. 1 passed; 0 failed
```

A later redundant `cargo xtask acceptance` rerun was attempted after candidate
creation. It did not produce a replacement acceptance receipt: the local `/tmp`
filesystem was at 80% with only 2.8 GiB available, and linking first terminated
with signal 7, then reported `Disk quota exceeded`; after candidate intermediates
were removed, a further acceptance child link again terminated with signal 7.
This resource failure is preserved here rather than represented as a test pass.
It occurred after the recorded 30/30 acceptance and complete `cargo xtask verify`
receipts on the identical committed source and after the five-target native run;
no source changed between those receipts and this redundant attempt.

## Files changed by this completion slice

- `crates/vesper-harness/src/release_executor.rs` — owned process-group/Job
  Object cancellation and the native lifecycle/restart regression.
- `crates/vesper-harness/Cargo.toml`, `Cargo.lock` — direct `command-group`
  dependency for the release executor.
- `xtask/src/main.rs`, `xtask/AGENTS.md` — 30th mandatory acceptance case.
- `.github/workflows/platform-foundation.yml` — explicit lifecycle step on all
  five production target families.
- `.github/workflows/release-recovery-lifecycle-acceptance.yml` — manual,
  read-only exact-SHA private acceptance matrix.
- `.github/AGENTS.md`, `crates/vesper-harness/AGENTS.md`, `docs/AGENTS.md`,
  `docs/foundation/AGENTS.md` — updated durable ownership/evidence contracts.
- `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md` — current completion
  status and final evidence link.
- `docs/foundation/evidence-index.md` — completion entry.
- This report — exact methods, evidence, deviations and readiness effect.

The broader PR-1–PR-7 file set and its methods remain owned by the three earlier
RRC execution reports; this final report does not relabel unrelated workspace
artifacts as new work.

## Full PRD acceptance audit

| Requirement | Verdict | Current evidence |
|---|---|---|
| AC-01 partial matrix blocks repair/rerun | PASS | `partial_matrix_blocks_retry`; complete-matrix controlled run. |
| AC-02 first causal evidence, not wrapper exit | PASS | parser/classification regressions and controlled causal-log fixture. |
| AC-03 identical fingerprint without change blocks retry | PASS | reducer regression and run `36586826432`. |
| AC-04 admitted retry records hypothesis and focused proof | PASS | typed `RepairAttempt`; observed worker tool receipts. |
| AC-05 platform repair uses focused proof before full matrix | PASS | verified-repair reducer and fresh-SHA dispatch regressions. |
| AC-06 retry budget is bounded | PASS | typed budgets and exhaustion regressions. |
| AC-07 changed fingerprint opens new diagnosis | PASS | reducer regression. |
| AC-08 compare with last green | PASS | exact immutable comparison regression. |
| AC-09 deterministic failure cannot become outage from community data | PASS | production never requests community telemetry; regression. |
| AC-10 official degradation + infrastructure evidence + repository exclusion pauses | PASS | bounded health adapter and hierarchy regressions. |
| AC-11 community reports alone cannot pause | PASS | deterministic regression. |
| AC-12 mixed health is unconfirmed | PASS | health-classifier regression. |
| AC-13 paused release resumes same checkpoint | PASS | pause/reopen exact acceptance. |
| AC-14 red later main does not move publication | PASS | immutable-publication reducer and controlled run `36586840308`. |
| AC-15 UI distinguishes Published from degraded main | PASS | shared status projection regression. |
| AC-16 provider-neutral controller | PASS | two-provider production orchestrator fixture; no provider branch in RRC core. |
| AC-17 TUI/ACP same active state | PASS | both hosts delegate to shared persisted controller; host suites green. |
| AC-18 stale SHA cannot advance | PASS | exact-SHA reducer regression. |
| AC-19 credential canaries excluded | PASS | redaction regression and controlled projection evidence. |
| AC-20 restart preserves epoch/run/job IDs | PASS | fresh-process native lifecycle test on all five targets, run `36594339290`. |
| AC-21 local cancellation is truthful and does not claim remote cancellation | PASS | real descendant reaping on five targets plus persisted cancellation wording. |
| AC-22 stagnation watchdog | PASS | six-action watchdog regression. |
| AC-23 published release plus later red main cannot loop | PASS | immutable reducer and controlled post-main-red run. |
| State-machine/retry/cross-host/provider-neutral/security requirements | PASS | 30-case acceptance, focused tests and complete `xtask verify`. |
| Controlled real GitHub workflow acceptance | PASS | four preserved PR-7 runs with exact run/job identities. |
| Native five-target process lifecycle | PASS | run `36594339290`, all five native jobs successful. |
| Production release publication | NOT RUN — EXPLICITLY EXCLUDED | User prohibited publication; no tag/release/registry/installation mutation occurred. This is not counted as a pass. |

## Deviations and failures

1. The final audit found a real contract defect after PR-7: the release executor
   killed only the direct process. The pre-fix regression failed with the leaked
   descendant canary; process-group/Job Object ownership repaired it.
2. The primary workspace had a pre-existing Git sequencer and extensive unrelated
   tracked/untracked artifacts. Candidate assembly and commit occurred in an
   isolated Git worktree; those artifacts were not deleted or committed.
3. The lifecycle workflow's dispatch commit is the controlled repository's
   `119dbf62...`, while its checkout input is the exact tested source
   `60eacc5c...`. This separation is deliberate and recorded in the run title and
   checkout step.
4. The macOS Apple Silicon runner emitted a capacity-delay annotation, but its job
   executed and passed. No lane was skipped or cross-compiled.
5. No real GitHub outage was manufactured. Official-health behavior remains
   deterministically verified; inventing a vendor incident would be false evidence.
6. Production release publication was not run because it was explicitly
   prohibited. It remains a future release event governed by the normal exact-SHA
   canonical/MSRV/five-target/web-driver gates.

## Unresolved items

- None within the authorized RRC implementation and acceptance scope.
- A future actual Agent Vesper release must still run its ordinary exact-commit
  gates and publication workflow. This report does not authorize or pre-claim that
  future release.

## Readiness effect

All four named RRC completion blockers now have current evidence. The controller
has runtime-observed bounded repair, epoch-confined fail-closed promotion,
controlled live GitHub behavior and real five-target host cancellation/restart
proof. The complete local verification and final hand audit support marking the
RRC implementation complete within the authorized non-publishing scope. No
production release state, credential, registry entry, installation or VRO-19
artifact changed.
