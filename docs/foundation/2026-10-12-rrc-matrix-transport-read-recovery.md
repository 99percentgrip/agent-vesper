# RRC exact-matrix transport read recovery

## Objective and measured scope extension

Complete the authorized queue/paste release through native RRC. The mission's
measured shared-blocking exception applies to a second observed read failure.
Source `c43967bd326c8ed8a54dd55a9e3f9d7118d14233` passed the seven mandatory
checks. Native RRC then passed all eight versioned local gates and pushed its
v0.24.13 candidate `5a5882da5a8341c7f47c00c078dc79163c0d6a13`. Ten of twelve
hosted jobs were green when a GitHub API connection error ended the watcher.
No tag or publication occurred; the full platform matrix was still incomplete.
It subsequently completed cancelled on both macOS lanes at the new queue PTY
test. The [queue execution report](2026-10-12-tui-message-queue-paste-execution.md)
owns that measured fixture backpressure correction and preserved partial ladder.
A subsequent exact-run API read succeeded. This does not establish an official
GitHub outage or a CI source failure.

Failed epoch: `20261012T071740.937081379Z-c43967bd326c`.
The failed owner was not polled indefinitely. Its ledger was copied read-only,
and the invoking TUI exited normally with `/quit` after settlement. The isolated
branch was fast-forwarded to its own controller-created version commit `5a5882da`;
no unrelated main change, rebase, primary-checkout edit or journal reset occurred.
The target remains the unused v0.24.13; no second release is requested.

## Root cause and architecture

`advance_release` returned a matrix adapter connection error directly, ending
the native worker although the operation was an exact-SHA read. The correction
uses the existing shared GitHub evidence port and worker's cancellable poll
backoff. Only the identified `GitHub API command failed` / `error connecting to
api.github.com` diagnostic qualifies in an untagged, pushed-candidate matrix
watch with no uncertain mutation. The last real matrix is retained, never
replaced by fabricated green or fabricated CI failure evidence.

`mutation.remote_matrix_read_retries` (legacy default zero) permits at most two
recoveries per epoch, additionally bounded by the existing shared targeted
diagnostic budget. Both counters and the redacted causal milestone are persisted
before a new read can occur. Initial remote-gate errors enter the normal matrix
wait so recovery observes the same cancellable backoff. Repeated errors stop
when either durable limit is exhausted. Source/CI retry budgets are untouched;
no CI rerun, candidate push or tag operation is replayed. Authentication,
certificate, permission, cancellation, malformed evidence, tagged/publication
and uncertain-write states remain ineligible. TUI and ACP inherit the same path.

## Exact evidence and methods

- Native stopped candidate: eight local gates passed; pushed true, tag and
  publication false; watcher failure was API connectivity, not a failed CI job.
- Red production-flow test on `5a5882da`: exit 101, zero passed/one failed:
  `/tmp/vesper-queue-matrix-read-red.log`.
- Four focused all-feature harness regressions: exit zero, four passed:
  `/tmp/vesper-queue-matrix-read-focused.log`. They verify fresh matrix recovery,
  durable pre-read limits/reload/exhaustion, unchanged CI/write authority,
  refusals, shared budget and legacy loading.
- Mandatory seven-gate final-source ladder passed sequentially with two Cargo
  jobs/two Rust test threads. All 44 scoped regressions passed, including real
  terminal interaction. The [receipt](2026-10-12-rrc-matrix-transport-read-recovery-receipts.json)
  records frozen source/log hashes and existing ignored cases. The
  [stopped native ledger](2026-10-12-rrc-matrix-transport-read-recovery-native-failed.json)
  preserves the observed failure before fresh admission. Earlier receipts remain
  historical and cannot certify this later correction.

| Final-source gate | Exit | Seconds |
| --- | --- | --- |
| `cargo xtask acceptance` | 0 | 207.97 |
| `cargo xtask verify` | 0 | 950.3 |
| `cargo xtask msrv` | 0 | 765.38 |
| `cargo deny --all-features check` | 0 | 11.78 |
| `cargo audit` | 0 | 2.98 |
| `cargo fmt --all -- --check` | 0 | 1.76 |
| `git diff --check` | 0 | 0.05 |

## Files and DOX

Production: `crates/vesper-harness/src/release_executor.rs` and
`release_recovery.rs`, with colocated tests. Owning harness/foundation DOX,
RRC PRD, queue PRD/report and evidence index accompany the candidate. Existing
parent boundaries/child indexes are unchanged. No provider adapter, host-local
release driver, VRO-19, installation or CI-policy shortcut is introduced.

## Unresolved and readiness

Full new-source verification passed. A fresh native exact-SHA release remains
pending; earlier hosted checks do not certify this corrected source.
Live Windows/macOS queue interaction and live-provider/delegated acceptance
remain separate and unexecuted. This correction covers the measured pre-tag
matrix connection error; it does not claim universal RRC repairability.
**READY FOR LIVE ACCEPTANCE: YES.** Native publication and live-only proofs
remain distinct. No tag, publication or installation has occurred.
