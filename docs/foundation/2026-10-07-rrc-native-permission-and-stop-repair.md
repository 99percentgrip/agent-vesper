# RRC native permission and stop repair

## Objective and status

After Alex cancelled the stalled v0.24.7 ACP client, verify native approval/stop behavior, fix confirmed production defects, and combine these repairs with the unreleased immutable-proof/Git-inventory audit. Worktree `/tmp/vesper-rrc-closeout-fix`, branch `repair/rrc-automatic-closeout`, base `0857f02b8dbdc2a867cdb93bf86a03559fb7555b`. ACP/shared source passed repository-wide verification. A later TUI stale-approval repair passed focused checks; final combined-source release gates remain pending. Uncommitted/unreleased, no installation change.

## Methods and findings

Read installed process state and client source without changing its authority. The cancelled ACP PIDs were absent. The temporary client's source had independently gained permission handling since diagnosis, but its post-prompt ledger-only polling remained an unreliable lifecycle owner; it was not used to verify these repairs.

- The shared controller's generic permission refusal discarded its real reason and fabricated local verification failure. Typed `AuthorizationBlocked` now preserves the bounded redacted reason and records stopped liveness without changing gate evidence, source repair admissions or retry budgets. Cancellation remains separately typed.
- The ACP bridge accepted `allow-always` although it advertised only `allow-once`/`reject-once`. Accept only the offered allow option. Real protocol tests reject `allow-always` and an invented option without a mutation or provider request.
- A replaced ACP approval future unconditionally removed the session's cancellation slot. Its cleanup could erase a newer request, and dropping a timeout future could leak its old slot. Generation-bound RAII leases remove only their own slot, including on drop; replacement retains the current request's cancellation hook.
- The TUI kept dead visible/queued approvals after their waiting operation ended, blocking input and retaining a mobile capability. Shared request liveness now lets each frame retire closed requests, revoke the mobile token, and skip expired queued entries with a bounded drain. A real broker regression reproduced the bug before repair and now proves fresh approval still works.
- The earlier [promotion/inventory audit](2026-10-07-rrc-promotion-and-inventory-gap-audit.md) supplies five additional repairs in this candidate. Its archived gates are historical; the full verification here must bind the combined source.

## Verification receipts

Focused checks:

- Shared authorization classification/projection: 2 passed; existing permission/no-side-effect regression: 1 passed.
- `cargo test --locked -p agent-vesper-acp --test release_resource_governor --features integration-test-harness`: 8 passed, 0 failed, 26.12 seconds. Real isolated ACP processes cover native/natural release, explicit Ask approvals across version preparation and local gates, concurrent status, cancellation, completed final receipt, conflicting target, rejected/invalid approval, and ignored late approval after cancellation. No live provider call or real release operation.
- `cargo test --locked -p vesper-acp --all-features`: 25 passed, 0 failed, including both request-lease regressions.
- ACP/shared source: `cargo xtask verify`, separate `cargo xtask acceptance` (127/127), architecture (31 packages), Rust 1.88 locked all-target/all-feature check and default-feature workspace check all passed. The receipt JSON preserves the four-file source snapshot from that run.
- Later TUI/core source: request-liveness regression passed; TUI binary suite 172 passed/1 existing skip; strict all-target/all-feature workspace Clippy passed. The final strengthened stale-modal assertion also passed. Final combined release gates must verify these later bytes and the installer integration.

Raw logs: `/tmp/vesper-rrc-acp-permission-process-eight.log`, `/tmp/vesper-rrc-permission-adapter.log`, `/tmp/vesper-rrc-authorization-final.log`, `/tmp/vesper-rrc-authorization-existing.log`. Combined checks write `/tmp/vesper-rrc-permission-full-{verify,acceptance,architecture,msrv,default}.log`.

## Deviations and retained failures

The first process fixture answered only version-preparation approval, then waited for a local gate requiring a separate approval: 6 passed/1 failed, retained in `/tmp/vesper-rrc-acp-permission-process.log`. Correct the protocol client fixture to answer both one-time requests; no production permissions, deadlines or resource thresholds were weakened. The corrected seven-test run passed; the final eight-test run also covers pending cancellation and late approval.

The first broad run retained 384 passed, one failed and six existing skips in the harness: the existing firewall-denial assertion still expected `MutationBlocked`. Update it to require typed `AuthorizationBlocked` with the exact firewall cause; the focused rerun passed. Preserve `/tmp/vesper-rrc-permission-full-verify-stale-assertion.log` as failed evidence; the full gate restarts on the corrected source.

The initial TUI regression fixture omitted its `Duration` import and failed compilation; retain `/tmp/vesper-rrc-tui-expired-approval-red.log`. After correcting the fixture, `/tmp/vesper-rrc-tui-expired-approval-reproduced.log` records the actual failed stale-request assertion; green and final logs record the production correction.

## Files and DOX

Shared executor/recovery error, ACP protocol bridge, existing release process fixture, shared permission request and TUI modal drain. Owning harness, agent, ACP crate, ACP app and TUI app contracts updated. Root instructions record native invocation and truthful installed-versus-local identity; foundation ownership, evidence index and PRD link this unit. Parent crates/apps/docs structure and indexes are unchanged because no new ownership boundary or dependency was added. Earlier source-bound reports retain their original hashes and have forward links.

## Unresolved items and readiness effect

Final integrated-source full local gates, exact-commit hosted matrices and publication/closeout remain pending. Synthetic providers and controlled Cargo commands prove orchestration/permissions, not live model repair effectiveness. No separate delegated review is claimed. No change to Alex's installation or the cancelled external client's script; a future release must use a current native host with a protocol-complete client. Passing focused tests alone cannot establish all PRD gaps are gone.

## Source and receipt binding

[Evidence JSON](2026-10-07-rrc-native-permission-and-stop-repair-evidence.json) binds source snapshots, requirement observations and raw log digests. [Receipt archive](2026-10-07-rrc-native-permission-and-stop-repair-receipts.tar.gz) preserves successes, fixture failures and the pre-repair red regression. Earlier full-source results do not certify later TUI or installer integration.

## Windows source integration

Preserve Windows installer branch commit `57d5456e53391e6012fdab6f7c86bc70507e592f` as a merge parent, including PowerShell 5.1/TLS/checksum/host-version checks, its permanent native Windows CI fixture, and unique owned voice-policy temporary roots. Both evidence indexes retain their reports. Integrated provenance explicitly supersedes the cancelled unmutated objective; native admission must reconcile it without manual ledger or retry-budget changes. Full integrated local gates and exact-main hosted receipts are still pending.
