# Native ConPTY cleanup race repair

## Objective and status

Continue the approved Windows/macOS voice repair and single v0.24.12 release. Correct terminal-fixture cleanup after the complete capacity-repair matrix. Local deterministic cleanup checks passed; fresh native CI and publication remain pending.

## Methods and commands

- Read the complete exact-SHA matrix for `9c5378131f308ea1396ccf93be0860bdffc711b7`: all eight local gates passed; eleven of twelve hosted jobs passed. Windows foundation failed in `Verify clean native first launch and provider Settings`.
- Captured `gh run view 38123265388 --job 114422218694 --log`. Landing for all four providers, authentication navigation and voice consent/decline passed before cleanup failed with `subprocess.CalledProcessError` from `taskkill /PID 7136 /T /F`, exit 255.
- Inspected the shared ConPTY driver: checking liveness and terminating are separate operations. A child exiting between them previously failed the fixture even if it was already dead. The log does not prove the particular child exited; the repair therefore verifies exit and continues to fail when death cannot be observed.
- `python3 apps/agent-vesper-tui/tests/test_windows_child_cleanup.py`: before repair, six tests executed, four passed and two errored; after repair, six passed. Mocked boundary checks are offline algorithm proof, not native Windows proof.

- Linux native TUI/ACP release-control fixture (`release_hosts_pty.py`): all four registered providers passed partial-matrix, immutable publication/main-health, blocked retry, cancellation identity and no-provider-call checks using private temporary state.

## Files and exact evidence

- `apps/agent-vesper-tui/tests/settings_pty.py`: nonzero taskkill is accepted only after bounded owned-child exit verification. Live-child failure becomes an explicit assertion; taskkill timeout and unsettled reader remain failures. POSIX cleanup is unchanged.
- `apps/agent-vesper-tui/tests/test_windows_child_cleanup.py`: already exited, successful kill, exit race, failed kill with live child, taskkill timeout, and unsettled reader.
- `.github/workflows/platform-foundation.yml`: runs the six guards on every native lane before actual terminal fixtures.
- Owning DOX, both PRDs, evidence index and objective provenance travel with the corrective source commit.
- [Complete failed matrix and retained RRC state](2026-10-11-native-conpty-cleanup-race-repair-failed-matrix.json).
- [Red/green receipts and complete Windows log](2026-10-11-native-conpty-cleanup-race-repair-receipts.tar.gz).

## Deviations and unresolved items

RRC stopped in `NeedMoreEvidence` with zero source repairs and zero full retries. Generic taskkill exit 255 was unknown; it did not invent an infrastructure cause or loop. This correction was performed by the root operator, so it is not evidence of autonomous RRC diagnosis of every failure. Existing budgets and archived epochs are preserved through normal canonical-source reconciliation. No ledger mutation, tag, release, installed application update or new version was performed for this repair.

Native Windows execution of the repaired fixture remains required. The Windows job stopped before real speech setup/recognition/synthesis; other platforms' green results cannot replace that missing lane. Microphone/speaker hardware, OS permission prompts, the original laptop and unlimited dictation remain separate acceptance boundaries.

## DOX and readiness effect

Updated the tests contract and CI ownership for bounded verified ConPTY cleanup, plus documentation ownership and links. Root/apps/TUI parent contracts remain unchanged because no production UI/provider behavior or domain boundary changed. RRC unknown-cause refusal remains intact. A clean canonical descendant may enter fresh exact-SHA release gates for the unchanged v0.24.12 target; this report does not certify full PRD parity or publication.
