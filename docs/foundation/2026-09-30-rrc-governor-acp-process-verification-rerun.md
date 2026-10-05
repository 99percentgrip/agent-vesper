# RRC Host Resource Governor ACP Process Acceptance — verification rerun

**Date:** 2026-09-30  
**Status:** Local Linux verification rerun passed; no candidate or release action.  
**Scope boundary:** This report re-verifies the ACP process regression recorded in [`2026-09-30-rrc-governor-acp-process-acceptance.md`](2026-09-30-rrc-governor-acp-process-acceptance.md). It does not add macOS/Windows resource backends, Alex-operated live acceptance, a candidate build, CI matrices, publication, tagging, pushing, installation, provider dispatch, or VRO-19 work.

## Objective

Independently re-run the implemented ACP Resource Governor process proof and the repository verification pipeline before treating the current worktree's release-status/Cargo-policy wiring as locally verified. Confirm that the exact ACP regression remains enrolled in the enforced acceptance gate.

## Methods and commands

1. Re-read the root, crate/harness, application/ACP, documentation/foundation, and `xtask` DOX contracts.
2. Audited the ACP test driver, process harness, controlled-Cargo test, `validate_version_mutation` policy inheritance, acceptance registration, PRD link, and evidence-index link.
3. Ran:

   ```text
   cargo test --locked --offline --all-features -p agent-vesper-acp --test release_resource_governor acp_process_release_status_uses_governor_for_every_cargo_path -- --exact --test-threads=1
   cargo fmt --all -- --check
   cargo clippy --locked --offline -p vesper-harness -p agent-vesper-acp --all-targets --all-features -- -D warnings
   git diff --check
   cargo xtask acceptance
   cargo xtask verify
   ```

## Files assessed

- `crates/vesper-harness/src/release_executor.rs`
- `apps/agent-vesper-acp/src/bin/agent-vesper-acp-test-driver.rs`
- `apps/agent-vesper-acp/tests/support/mod.rs`
- `apps/agent-vesper-acp/tests/release_resource_governor.rs`
- `xtask/src/main.rs`
- `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md`
- `docs/foundation/evidence-index.md`
- `docs/foundation/2026-09-30-rrc-governor-acp-process-acceptance.md`

The functional change remains narrowly scoped: version-preparation `cargo metadata` receives the admitted `CargoResourcePolicy`; the existing all-target check and later local gate already receive that policy. The Unix ACP process test controls Cargo and asserts the registered RUN/resource projection plus inherited Cargo job/test/target settings for `check`, `metadata`, and the held local gate, without provider dispatch.

## Exact evidence

The exact integration test passed:

```text
running 1 test
test acp_process_release_status_uses_governor_for_every_cargo_path ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.48s
```

Focused formatting/lint/whitespace verification returned successfully:

```text
cargo fmt --all -- --check
cargo clippy --locked --offline -p vesper-harness -p agent-vesper-acp --all-targets --all-features -- -D warnings
git diff --check
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.16s
```

The enforced acceptance gate completed all registered exact cases, including the ACP process case:

```text
acceptance verified: acp_process_release_status_uses_governor_for_every_cargo_path
acceptance progress: 43/44 — acp_process_release_status_uses_governor_for_every_cargo_path
acceptance verified: rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty
acceptance progress: 44/44 — rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty
Acceptance regression gate: 44 exact cases passed in 22283 ms. Offline fixture model cost: zero; live-model effectiveness is not measured.
```

The complete repository pipeline `cargo xtask verify` also returned success. Its all-features workspace test phase included the new process test:

```text
running 1 test
test acp_process_release_status_uses_governor_for_every_cargo_path ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.49s
```

No test or verification command contacted a live provider. The ACP regression's nonblocking loopback listener remained a negative dispatch canary; the test succeeds only when release/status commands do not connect to it.

## Deviations and unresolved items

- The worktree contains broader inherited RRC resource/progress changes. This rerun
  audits and verifies only the ACP process proof and Cargo-policy inheritance described
  here; it does not attribute every pending worktree change to this narrow repair.
- The process acceptance is intentionally Unix/Linux-local because it exercises the real Linux observation backend and a POSIX controlled-Cargo wrapper. macOS and Windows continue to report unavailable discovery and fail closed for expensive local RRC work.
- The integration-only ACP driver enables a permissive **admission policy** so a constrained CI machine can reach the controlled gate. It does not replace Linux telemetry, cgroup, process-tree, or filesystem observations.
- The proof covers Cargo-policy propagation and ACP status projection, not destructive memory/disk pressure or owned-process termination; those behaviors retain their separate governor regressions.
- Alex-operated live acceptance, candidate/release work, publication and all remote CI evidence remain unexecuted.
- A broad link traversal of the pre-existing `evidence-index.md` found the historical
  missing target `2026-09-29-active-agent-supervision-0845z.md`. The new report's
  registrations and directly referenced evidence validate; this narrow process-proof
  rerun does not rewrite unrelated historical evidence-index entries.

## Readiness effect

The current worktree retains direct Linux-local evidence that the controller owns the ACP RUN/resource status and that every presently reachable version-preparation/local-gate Cargo invocation inherits its bounded Cargo policy. The exact case is now enforced by the 44-case acceptance gate and passed again inside full `cargo xtask verify`. This closes only the stated ACP process proof gap; it does not justify a cross-platform or release-readiness claim.
