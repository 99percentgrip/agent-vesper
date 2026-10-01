# RRC Host Resource Governor ACP Process Acceptance — execution report

**Date:** 2026-09-30  
**Status:** Linux-local ACP process proof passed; no candidate or release action.  
**Scope boundary:** This follow-up proves the real ACP release-status projection and every Cargo invocation made during RRC version preparation/local-gate startup. It does **not** establish macOS or Windows resource-backend support, Alex-operated live acceptance, a candidate build, CI matrix, publication, tag, push, installation, provider dispatch, or VRO-19 work.

## Objective

Close the remaining ACP process-level proof gap for the controller-owned Host Resource Governor before any candidate build. The proof must show that an ACP `/release patch` starts the background RRC, `/release status` renders the registered worker's observed RUN/resource snapshot, no provider request is dispatched, and all Cargo invocations in version preparation and the admitted gate inherit the governor's bounded concurrency and managed target cache.

## Audit note and correction

The earlier [`2026-09-30-rrc-host-resource-governor.md`](2026-09-30-rrc-host-resource-governor.md) correctly described the intended shared ACP projection, but its recorded verification did not include an ACP **process** proof that the version-preparation `cargo metadata` command inherited the governor policy. Treating the earlier source/constrained-host evidence as complete ACP process evidence would have overstated the proof.

This work adds that process regression. It also corrects the regression's initial broad “no percent sign” assertion: `Swap gate below 100% used` is a truthful resource-policy threshold, not release progress. The final assertion rejects percentages only on release-progress rows, where the contract permits only typed/count-derived progress.

## Implementation

- `crates/vesper-harness/src/release_executor.rs`
  - `validate_version_mutation` now receives the admitted `CargoResourcePolicy` and passes its inherited environment to `cargo metadata --locked --no-deps --format-version 1`.
  - Therefore the version-preparation all-target `cargo check`, locked `cargo metadata`, and every later local gate have the same `CARGO_BUILD_JOBS`, `RUST_TEST_THREADS`, and `CARGO_TARGET_DIR` policy.
- `apps/agent-vesper-acp/src/bin/agent-vesper-acp-test-driver.rs`
  - The integration-only driver enables only the harness's process-test permissive **policy** seam. It still uses the real Linux `/proc`, cgroup, owned-process-tree, and filesystem observation paths.
- `apps/agent-vesper-acp/tests/support/mod.rs`
  - Adds a full-harness process helper and a session initializer that uses the test fixture workspace as the ACP workspace root.
- `apps/agent-vesper-acp/tests/release_resource_governor.rs`
  - Creates a clean temporary Git release fixture and a controlled `cargo` wrapper.
  - Drives actual ACP JSON-RPC `initialize` → `session/new` → `/release patch` → `/release status`.
  - Holds the first local gate open, verifies RUN/resource rows and typed count progress, then checks the wrapper receipts for `check`, `metadata`, and `gate`.
  - Requires one Cargo job, one test thread, and a managed target path under the temporary RRC release root for all three receipts; it also rejects a source-worktree `target/` directory and any provider connection.
  - Releases the controlled gate only after inspection; the wrapper exits nonzero, so the RRC settles locally without push, tag, or publication.
- `xtask/src/main.rs`
  - Enrolls the exact Unix ACP process case in `cargo xtask acceptance`; deletion, rename, or a zero-match selection fails the gate.

## Files

- `crates/vesper-harness/src/release_executor.rs`
- `apps/agent-vesper-acp/src/bin/agent-vesper-acp-test-driver.rs`
- `apps/agent-vesper-acp/tests/support/mod.rs`
- `apps/agent-vesper-acp/tests/release_resource_governor.rs`
- `xtask/src/main.rs`
- `apps/agent-vesper-acp/AGENTS.md`
- `xtask/AGENTS.md`
- `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md`
- `docs/foundation/evidence-index.md`
- `docs/foundation/AGENTS.md`
- `docs/AGENTS.md`

## Exact verification evidence

The targeted ACP process test ran with all features, locked/offline dependency resolution, and one exact case:

```text
cargo test --locked --offline --all-features -p agent-vesper-acp --test release_resource_governor acp_process_release_status_uses_governor_for_every_cargo_path -- --exact --test-threads=1
running 1 test
test acp_process_release_status_uses_governor_for_every_cargo_path ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.48s
```

Formatting, targeted strict linting, and patch-whitespace validation also passed:

```text
cargo fmt --all -- --check
cargo clippy --locked --offline -p vesper-harness -p agent-vesper-acp --all-targets --all-features -- -D warnings
git diff --check
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.60s
```

The ACP status assertion observed the actual registered worker fields:

```text
RUN                  Local verification 0/7 · workspace-verify
Progress             Local verification · 0/7 local · 0/0 remote jobs
Current process      cargo
RESOURCES
RAM avail
Cgroup
RRC RSS
Swap
Disk free
Cargo jobs  1
Pressure    Normal
Action      verification admitted within the RRC resource budget
```

The controlled wrapper records all governed Cargo paths and the test requires these properties for `check`, `metadata`, and `gate`:

```text
<Cargo path>|1|1|<temporary release-root>/host-resources/targets/...
```

The enrolled acceptance gate passed after adding the ACP process case:

```text
cargo xtask acceptance
acceptance verified: acp_process_release_status_uses_governor_for_every_cargo_path
acceptance progress: 43/44 — acp_process_release_status_uses_governor_for_every_cargo_path
acceptance verified: rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty
acceptance progress: 44/44 — rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty
Acceptance regression gate: 44 exact cases passed in 22646 ms. Offline fixture model cost: zero; live-model effectiveness is not measured.
```

The test's nonblocking loopback listener receives no connection; `/release` and `/release status` remain host/controller actions rather than provider turns.

## Deviations and unresolved items

1. **Linux/Unix-only process proof.** The test depends on the real Linux governor backend and the Unix controlled-Cargo wrapper. macOS and Windows still intentionally report resource discovery unavailable and fail closed for expensive local RRC work; this is not five-target acceptance.
2. **Permissive test policy only.** The integration-only driver avoids a constrained CI worker refusing the fixture before the controlled gate can be inspected. It does not mock telemetry: Linux host/cgroup/process/disk discovery stays real. Production binaries neither enable nor expose this seam.
3. **No destructive host-pressure experiment.** The controlled wrapper proves environment propagation and ACP projection; constrained-memory/disk and process-stop behavior retain their separate harness evidence.
4. **No candidate or release action.** The fixture uses an isolated temporary Git repository and temporary release-state root. The gate intentionally fails before any remote mutation path. The test proves no provider request, but it is not a live human acceptance of a production release.
5. **Historical evidence-index links remain stale outside this work unit.** A repository-wide scan found pre-existing missing historical foundation/VRO-19/bridge targets in `evidence-index.md`. The new report, index, and PRD links resolve; this narrow proof does not rewrite unrelated evidence history.

## Readiness effect

The ACP process path now has direct evidence that status is sourced from the registered RRC worker and that all currently reachable Cargo paths—including version-validation metadata—inherit the controller-owned resource policy. This removes a narrow Linux-local proof gap and strengthens the no-fabricated-progress contract. It does not elevate the historical PRD status claim into full progress-UX, cross-platform, or release readiness.
