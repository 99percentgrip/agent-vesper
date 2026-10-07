# RRC native cancellation keybinding repair

## Objective and status

Repair the confirmed TUI cancellation gap discovered while the integrated v0.24.8 RRC candidate was undergoing native local verification. Work remains isolated on `repair/rrc-automatic-closeout`, parent `b2f88f4e23f5eabd5fa8c5284c4a5274b0ef8c7d`. Preserve the shared controller, permissions, retry history, one release target and Alex's installed application. Focused verification is recorded below; final versioned local and hosted release gates remain required.

## Methods and confirmed defects

Read the live controller ledger and owner tree, then followed the TUI keybinding and approval-modal paths. The native run had passed version preparation, workspace verification, acceptance and architecture and was running MSRV. It was progressing, not stuck. Ctrl+C only considered ordinary `agent_running`; native RRC is separately registered. The approval modal swallowed cancellation keys. Ordinary cancellation also overwrote the pending-cleanup status with an already-cancelled claim.

The configured `cancel_turn` binding now runs before modal interception. Existing foreground turn cancellation and explicit voice Stop retain priority; otherwise a registered native RRC worker invokes the same provider-neutral shared `release_command_for_workspace(..., "cancel")` used by the slash route. No model request or concrete-provider branch is introduced. The result or error is rendered truthfully. Ordinary cancellation retains its requested/waiting status until cleanup completes. Expired approvals continue to retire through the already-tested broker lifecycle; cancellation never approves an operation.

The preceding native attempt was intentionally cancelled through `/release cancel` and its temporary host exited through `/quit`, preserving the ledger and completed receipts. MSRV and subsequent gates were interrupted/unexecuted and are not counted as passed. v0.24.8 was not pushed, tagged or published; the managed build cache is preserved for the successor. Alex's installed v0.24.7 and user TUI were untouched.

## Verification receipts

- Red test against the extracted previous cancellation behavior: two failures retained in `vesper-rrc-cancel-keybinding-red.log`. The release-route assertion reproduced the defect; the other assertion initially assumed token cancellation immediately stopped the ordinary turn and was corrected to respect pending cleanup.
- Intermediate corrected test compile failed on an unqualified `Duration`; preserve `vesper-rrc-cancel-keybinding-green.log`. This fixture error was corrected without changing production semantics.
- Default focused cancellation checks: 17 passed, zero failed (`vesper-rrc-cancel-keybinding-green-v2.log`).
- Final all-feature TUI binary suite: 175 passed, zero failed, one existing ignored test (`vesper-rrc-cancel-keybinding-all-final.log`). Covers native release routing, foreground priority, truthful errors, configured F6 while a real broker approval waits, no implicit approval, expired mobile capability retirement, existing partial-output cancellation and voice behavior.
- Strict all-target/all-feature TUI Clippy and rebuilt native-host receipts are bound in the evidence companion. Final integrated local acceptance, exact-main canonical/MSRV/five-target/web-driver gates and publication/registry closeout remain separate required evidence.

## Files and DOX

`apps/agent-vesper-tui/src/main.rs`, nearest TUI contract, this report and companions, foundation ownership/evidence index, owning RRC PRD and integrated release-objective provenance. Parent root/apps/docs contracts and indexes retain their structure and ownership and were intentionally unchanged after review. ACP already routes `session/cancel` through the shared controller; its eight real process approval/cancellation cases are retained in the native permission report. No ACP protocol change is necessary for a terminal keybinding repair.

## Deviations, unresolved items and readiness effect

A broad historical-link scan stopped on the pre-existing missing `2026-10-02-agent-process-status-check.md` entry. Scoped links for this change and both JSON companions were checked successfully; unrelated historical content was left unchanged.

This is an additional confirmed gap, so the previous running candidate was stopped before publication and the same integrated objective/version is continued with the repaired native host. Interrupted local checks are retained rather than represented as final success. No independent delegated review or live-model repair effectiveness is claimed. The final publication, immutable tag, assets, registry update and completion receipt are still required; clean Windows device acceptance remains with Alex. Documentation is included in the code candidate before CI and post-publication-only receipts remain local unless separately authorized.

## Receipt binding

[Evidence JSON](2026-10-07-rrc-native-cancellation-keybinding-repair-evidence.json) and [raw receipts](2026-10-07-rrc-native-cancellation-keybinding-repair-receipts.tar.gz) bind the source, exact successes/failures and interrupted native state. Archive digest is recorded in the JSON.
