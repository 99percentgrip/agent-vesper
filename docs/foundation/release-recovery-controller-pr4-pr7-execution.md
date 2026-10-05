# Release Recovery Controller PR-4 through PR-7 execution

**Date:** 2026-09-29
**Workspace base commit:** `0b5630d271965e7d9df3a0a09c116c7f8c44042e`
**Branch:** `vro18.1/settings-auth`
**Verdict:** **PR-4 THROUGH PR-7 IMPLEMENTED AND VERIFIED WITHIN THE AUTHORIZED NON-PUBLISHING SCOPE. Production publication and live five-target host cancellation/restart remain NOT RUN.**

## Objective

Continue the accepted PR-1 through PR-3 Release Recovery Controller (RRC) without redesigning it, complete production progression and bounded repair under controller authority, add bounded official-health classification, provide equivalent TUI/ACP control surfaces, and perform deterministic plus controlled real-GitHub acceptance without publishing an Agent Vesper release. VRO-19 remained on hold.

## Methods and commands

The work re-read the root and applicable harness, app, workflow, xtask, documentation and foundation DOX contracts; inspected the inherited dirty worktree without deleting or sweeping unrelated files; completed controller/executor and host integration; created an explicitly authorized private test repository for real GitHub evidence; and ran repository verification.

Principal commands:

```text
cargo fmt --all
cargo test -p vesper-harness release_recovery --lib
cargo test -p vesper-harness release_executor --lib
cargo test -p agent-vesper-acp release_ --lib
cargo test -p agent-vesper-tui release_ --lib
cargo test -p vesper-harness --test release_repair_retry --test release_repair_dispatch
cargo xtask acceptance
cargo xtask verify
git diff --check

gh repo create 99percentgrip/agent-vesper-rrc-acceptance --private --source <isolated-temp-root> --remote origin --push
gh workflow run release-recovery-acceptance.yml -R 99percentgrip/agent-vesper-rrc-acceptance -f scenario=<scenario> -f causal_signature=rrc_fixture_assertion_42
gh run view <run-id> -R 99percentgrip/agent-vesper-rrc-acceptance --json ...
gh api repos/99percentgrip/agent-vesper-rrc-acceptance/git/matching-refs/tags
gh api repos/99percentgrip/agent-vesper-rrc-acceptance/releases
```

No Agent Vesper commit, push, tag, registry change, installer, production-workflow mutation or release publication was performed. The private acceptance repository was intentionally preserved for audit, as authorized.

## Implemented files and behavior

### PR-4 — production progression and bounded repair

- `crates/vesper-harness/src/release_recovery.rs`
  - Retains the accepted typed lifecycle, exact-SHA evidence, immutable publication boundary, retry budgets and persisted ledger.
  - Requires complete configured gate matrices before retry or advancement.
  - Blocks the same fingerprint without relevant state change and admits one fresh full-gate retry only after a verified repair.
  - Routes `/release` start/status/resume/cancel/evidence/retry through persisted controller state.
- `crates/vesper-harness/src/release_executor.rs`
  - Runs existing workspace version/lock/registry updates, canonical local gates, candidate commit/push, exact tag push and release/asset verification rather than duplicating release logic.
  - Requires typed `ReleaseMutationAdmission` for every mutating operation.
  - Polls cancellable local subprocesses, kills and waits on cancellation, bounds output, and reloads the ledger so a stale worker cannot overwrite `Cancelled`.
  - Runs one bounded provider-neutral AgentLoop repair in an isolated user-state worktree for proven or strongly supported deterministic source failures.
  - Requires observed successful mutation and post-mutation focused-command receipts, a non-empty diff, a clean controller workspace and a fresh repair commit before promotion.
  - Pushes the repaired SHA to obtain fresh push workflows; it never reruns an old Actions run against the wrong SHA.
- `crates/vesper-harness/src/lib.rs`
  - `WorkerFactory` now accepts the host's ordinary permission port and constructs the repair AgentLoop in `Ask` mode against only the isolated repair workspace.
- `crates/vesper-harness/tests/release_repair_retry.rs` and `release_repair_dispatch.rs`
  - Regress the repair prerequisite and prove that a verified repair pushes a fresh candidate without calling old-run rerun APIs.

### PR-5 — bounded external health

- Official status uses one HTTPS-only request with a ten-second transport cap inside the fifteen-second PRD budget and removes provider-token environment variables.
- Repository-green classification requires a non-empty local-gate set and every local gate settled `Succeeded`.
- Known last-green failures or source-related changes prevent outage admission.
- Production does not query or persist community telemetry.
- A paused epoch rechecks official status and resumes the same exact identity through fresh remote evidence.

### PR-6 — host parity

- `apps/agent-vesper-tui/src/commands.rs` and `main.rs` route `/release patch|minor|major|status|resume|cancel|evidence|retry` to the shared controller rather than a model workflow, append RRC state to `/ci`, and supply the TUI permission port to bounded repairs.
- `apps/agent-vesper-acp/src/lib.rs` serves the same controller in process, uses the ACP workspace root, supplies `AcpHarnessPermissionPort`, and does not persist or dispatch `/release` as ordinary provider chat.
- Both hosts distinguish immutable `Published` state from later degraded `main` state through the shared projection.

### PR-7 — deterministic and real controlled acceptance

- `.github/workflows/release-recovery-acceptance.yml` defines a manual, read-only, non-publishing five-lane fixture with explicit scenario run titles, stable causal output and a synthetic secret canary.
- `xtask/src/main.rs` fail-closed enrolls six exact RRC/controller-route cases.
- The workflow was copied to the authorized private audit repository `99percentgrip/agent-vesper-rrc-acceptance`; its default branch exact commit is `e149b2e0f57ec9962ad56e217b1a5571b9fb1c23`.
- The repository remains private and preserved. It has zero tag refs and zero releases.

## Exact evidence

### Local RRC and repository verification

Focused controller core:

```text
running 25 tests
...
test result: ok. 25 passed; 0 failed; 0 ignored; 0 measured; 131 filtered out
```

Production executor:

```text
running 4 tests
...
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 152 filtered out
```

Fresh-SHA repair dispatch:

```text
running 1 test
test advancing_verified_repair_pushes_new_sha_without_actions_rerun ... ok
test result: ok. 1 passed; 0 failed

running 1 test
test verified_repair_requires_a_fresh_candidate_push_before_remote_retry ... ok
test result: ok. 1 passed; 0 failed
```

Final acceptance enrollment:

```text
acceptance verified: release_recovery::tests::partial_matrix_blocks_retry
acceptance verified: release_recovery::tests::verified_repair_is_the_only_path_to_one_full_gate_retry
acceptance verified: release_recovery::tests::paused_epoch_reopens_with_exact_identity_and_resumes_through_remote_refresh
acceptance verified: release_recovery::tests::green_release_then_red_closeout_keeps_publication_immutable
acceptance verified: release_executor::tests::production_orchestrator_reaches_publication_only_through_settled_gates
acceptance verified: commands::tests::release_routes_to_the_controller_instead_of_a_model_workflow
Acceptance regression gate: 29 exact cases passed in 7797 ms. Offline fixture model cost: zero; live-model effectiveness is not measured.
```

Final full gate:

```text
running: cargo fmt --all --check
running: cargo clippy --workspace --all-targets --all-features -- -D warnings
running: cargo test --workspace --all-features
```

The final `cargo xtask verify` exited `0`. `git diff --check` also exited `0`.

### Controlled real GitHub receipts

All final runs used exact fixture commit `e149b2e0f57ec9962ad56e217b1a5571b9fb1c23` and event `workflow_dispatch`.

| Scenario | Run | Settled result | Job evidence |
|---|---:|---|---|
| complete matrix red | [36586801959](https://github.com/99percentgrip/agent-vesper-rrc-acceptance/actions/runs/36586801959) | completed / failure | four jobs success; `fixture-macos-intel` job `109469201386` failure |
| focused repair green | [36586812935](https://github.com/99percentgrip/agent-vesper-rrc-acceptance/actions/runs/36586812935) | completed / success | all five jobs success; macOS-Intel job `109469239475` success |
| same fingerprint red | [36586826432](https://github.com/99percentgrip/agent-vesper-rrc-acceptance/actions/runs/36586826432) | completed / failure | four jobs success; `fixture-macos-intel` job `109469341229` failure |
| post-release main red | [36586840308](https://github.com/99percentgrip/agent-vesper-rrc-acceptance/actions/runs/36586840308) | completed / failure | four jobs success; `fixture-macos-intel` job `109469327571` failure |

Bounded failed-log inspection found the stable causal signature, the panic source `tests/rrc_fixture.rs:42`, and the synthetic canary in each red run. Raw canary text is intentionally omitted here. Log artifact receipts:

```text
run=36586801959 bytes=2456 sha256=9d47822fe3a697e2b46555aaa3ffe5790c7488a94964b550eecc8214ab47545a causal=True panic=True canary=True
run=36586826432 bytes=2457 sha256=28133aa4ceec674772bff8af673b6745e529a0ce724d2468730b1497450aefc7 causal=True panic=True canary=True
run=36586840308 bytes=2458 sha256=e322275ab99ee48f461901def6793dd36b399e48e48467817f8f80e5a6d55a37 causal=True panic=True canary=True
tags=0
releases=0
```

### Workspace artifact identities

```text
ed17f353f2e4f53c31e86c42684210b0d8ad8dee4ad4cd00779b0b0063296fc6  .github/workflows/release-recovery-acceptance.yml
6b9b6e6ab93b38df8f2c809a1882806df2226c18a1380a18afea063ddaf91524  crates/vesper-harness/src/release_executor.rs
ffa493e704fbb1cbef447243cbf969e25f4815db20df738f408b68d54d83359b  crates/vesper-harness/src/release_recovery.rs
```

These hashes identify the dirty-worktree artifacts tested in this unit; they are not Agent Vesper commit identities.

## Final requirement audit

| Requirement | Verdict | Current evidence |
|---|---|---|
| AC-01 partial matrix cannot trigger repair/rerun | PASS | Exact acceptance `partial_matrix_blocks_retry`. |
| AC-02 first causal failure, not wrapper exit | PASS | Parser tests plus real red-run panic/source receipts. |
| AC-03 same fingerprint without change blocks retry | PASS | State-machine tests and controlled repeated-signature red run. |
| AC-04/05 repair records hypothesis and focused proof | PASS | Bounded worker observes tool receipts; promotion and retry regressions. |
| AC-06 retry budget bounded | PASS | Typed budget/state-machine tests. |
| AC-07 changed fingerprint opens diagnosis | PASS | State-machine regression. |
| AC-08 last-green comparison | PASS | Immutable exact comparison regression. |
| AC-09/11 community data cannot declare outage | PASS | Production never queries it; deterministic regressions. |
| AC-10/12 bounded official-health hierarchy | PASS | Present/all-green local evidence and source/last-green exclusions enforced. |
| AC-13 paused release resumes same checkpoint | PASS | Exact acceptance pause/resume case. |
| AC-14/15 published versus later red main | PASS | Exact acceptance plus controlled post-main-red run; publication stays immutable in controller test. |
| AC-16 provider neutrality | PASS | Production orchestrator runs multiple provider fixtures; core has no provider branch. |
| AC-17 TUI/ACP same active state | PASS | Both delegate to shared controller/projection; workspace suites pass. |
| AC-18 stale SHA cannot advance | PASS | Exact-SHA state-machine regression. |
| AC-19 secret canary excluded from persisted/UI evidence | PASS | Redaction tests; real logs contain the synthetic canary for ingestion testing, while this report and projection omit its value. |
| AC-20 restart exact run/job identity | PASS, deterministic | Ledger round-trip and pause/reopen regressions. No live host-process restart matrix was run. |
| AC-21 local cancellation truthful | PASS, deterministic | Atomic worker cancellation, child kill/wait and persisted-state reload; remote workflows are explicitly unaffected. |
| AC-22 stagnation watchdog | PASS | Six-action watchdog regression. |
| AC-23 green publication then red main avoids loops | PASS | Exact immutable-publication regression and controlled post-main-red scenario. |
| Controlled real GitHub workflow | PASS | Four named preserved private runs with exact SHA, run/job IDs and log digests. |
| Current Agent Vesper release workflow end to end | NOT RUN | Prohibited by the authorized non-publishing scope; no tag/release was created. |
| Live five-target host cancellation/restart | NOT RUN | Deterministic and workspace suites pass; no five-OS/architecture host-process run was performed. |

## Final audit

The closeout re-derived the authority chain by hand: only RRC state emits typed
mutation admissions; a complete exact-SHA gate matrix is required; publication is
an immutable boundary; deterministic repairs remain permissioned and confined;
external health cannot override repository evidence; and neither host owns a
private lifecycle. Static inspection found no concrete-provider token in either
RRC source file. The controlled workflow has `contents: read`, no write/publish
token and scenario-specific run names.

The repair regressions in `release_repair_retry.rs` and
`release_repair_dispatch.rs` encode the pre-repair defects: an unverified repair
cannot consume the full-gate retry, and a verified changed SHA must be pushed for
fresh workflows rather than rerunning old Actions IDs. The partial-matrix,
pause/reopen, immutable-publication and host-route cases likewise prevent the
pre-continuation behavior from satisfying acceptance. Current receipts are green;
no historical red test receipt was invented where one was not retained.

Narrative claims were checked against the source, final local commands and GitHub
run/job API responses. The audit corrected the previous report's stale open-gap
claims in the PRD, evidence index and DOX ownership records, while retaining the
two genuinely unexecuted evidence items.

## Deviations and failures

1. The first final `cargo xtask verify` attempt failed strict Clippy because `advance_release` had nine arguments (`clippy::too_many_arguments`). It was refactored to a typed `ReleaseAdvanceContext`; focused Clippy and the complete gate then passed. The failed attempt is preserved rather than erased.
2. The first controlled run set lacked scenario names in GitHub's run title. The fixture gained `run-name: rrc-${{ inputs.scenario }}`, was committed to the private repository as `e149b2e0f57ec9962ad56e217b1a5571b9fb1c23`, and all four scenarios were rerun. The final named runs above supersede the earlier ambiguous runs; the earlier runs remain preserved in the private repository.
3. The workflow's five lanes are deterministic logical fixture lanes on `ubuntu-24.04`; they do not claim five-target native execution.
4. No live official-status outage was manufactured. External-health behavior was verified with deterministic ports to avoid fabricating or depending on a current vendor incident.
5. The working tree already contained extensive unrelated tracked and untracked artifacts. They were not deleted, swept, committed or attributed to RRC.

## Readiness effect

PR-4 through PR-7 now have production code, local acceptance, shared host wiring, permission-aware bounded repair/worktree promotion, bounded official-health behavior and preserved controlled real-GitHub evidence. This closes the previously recorded repair-handoff, worktree-promotion and undispatched-controlled-workflow gaps.

The result is **not evidence that an Agent Vesper production release was published successfully**. A future release still must satisfy the repository's exact-commit canonical, MSRV, five-target foundation and web-driver gates before tagging, and live five-target host cancellation/restart remains unexecuted evidence. No production asset or user installation changed during this work unit.
