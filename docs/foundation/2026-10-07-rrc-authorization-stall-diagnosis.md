# RRC authorization stall diagnosis and scoped repair

## Objective and status

Explain why the live agent remains running after previous RRC repairs and correct the confirmed shared-controller reporting defect. Local source repair only; the installed session and its external Python client were not modified, resumed or stopped. Full PRD parity and live release recovery are not established by this work.

## Methods and incident evidence

Read-only `/proc` process/child inspection, selected ledger fields, installed ACP `--version`, ACP driver source and log timestamps. At 09:37 UTC on 2026-10-07, installed TUI PID 1507718 and ACP PID 1872484 were alive, but no build/test workers or active release operation existed. ACP ran in `vesper-ps51-release-anchor`; its parent was `/tmp/vesper-ps51-rrc-acp.py`. Both ledger and live log last progressed at approximately 09:12 UTC, with `host permission denied release side effect`. The ledger was `diagnosing_local_failure`, liveness idle, no owner or in-flight mutation, no version prepared, candidate push or tag. Installed ACP reports v0.24.7. The previous promotion/inventory audit is local and unreleased.

The ACP client omits permission-response handling: incoming requests are collected as notifications, not answered. It also polls for up to an hour, treating neither `diagnosing_local_failure` nor blocked liveness as an intervention boundary. The source has a five-minute approval timeout. An unanswered approval is consistent with the observed denial, but the installed controller collapses rejection/timeout/cancellation reasons into the same generic message, so the historical log cannot distinguish them conclusively. Permission refusal must remain enforced; user intent does not justify silently elevating an ACP client's permission mode.

## Confirmed defect and repair

`authorize_controller_step` returned generic `MutationBlocked`, discarding the permission-port reason. `persist_worker_failure` then treated that controller authorization stop during LocalVerification as source/test failure and entered failure diagnosis without causal test evidence.

Introduce typed `AuthorizationBlocked` for missing host permission ports, firewall denial, execution-policy denial and permission-port denial. Preserve bounded redacted causes, including timeout and ACP rejection. Explicit active release cancellation retains its cancellation classification. Authorization blocks use existing failed-liveness projection, retain the preauthorization checkpoint and do not fabricate gate failure, consume retry budgets or trigger source repair. Both hosts use this shared provider-neutral path; no concrete-provider or host shortcut was added.

## Verification and artifacts

- `cargo fmt --all`: passed.
- `cargo test --locked -p vesper-harness --lib --all-features authorization`: 2 passed, 0 failed. The direct authorization test invokes the real shared gate with a denied port for two synthetic provider identities; the persistence test covers timeout, ACP rejection, firewall denial and missing permission port, with unchanged mutation/gate/budget and no repair or causal failures.
- `cargo test --locked -p vesper-harness --lib --all-features denied_host_permission_prevents_every_release_side_effect`: 1 passed, 0 failed.
- `cargo clippy --locked -p vesper-harness --all-targets --all-features -- -D warnings`: passed.
- `git diff --check` and report relative-link/JSON validation: passed.

[Source/log digest receipt](2026-10-07-rrc-authorization-stall-evidence.json) binds both Rust files and focused logs. Local command logs: `/tmp/vesper-rrc-authorization-final.log`, `/tmp/vesper-rrc-authorization-existing.log`, `/tmp/vesper-rrc-authorization-clippy.log`.

## Files and DOX pass

Changed shared executor and recovery error type; nearest harness contract records typed authorization stops. This report is linked from the evidence index and owning RRC PRD, and the foundation ownership contract is updated. Earlier promotion/inventory evidence explicitly retains its frozen source scope. Root/crates/docs parent instructions and child indexes remain unchanged because ownership, dependency direction and hierarchy are unchanged.

## Deviations, unresolved items and readiness effect

No live provider calls, credentials, user-state writes, installed-app replacement, release operations or ledger resets. The external driver's permission handler and terminal detection remain defective and unchanged; preserving another running agent's client avoids altering its release authority. This work improves shared failure classification and diagnostic truthfulness, but does not fix that live client's workflow. Full current-source workspace/acceptance/MSRV and hosted release gates were not rerun in this scoped incident repair; earlier gates cannot certify these new bytes. No claim that all RRC gaps are fixed or that this running release completed is made.

## Later native-path repair

The [native permission and stop unit](2026-10-07-rrc-native-permission-and-stop-repair.md) adds real protocol coverage and fixes approval-option/cleanup defects. The scoped source hashes above remain historical; no running-client recovery was performed by this diagnosis.
