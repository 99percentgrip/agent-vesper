# Native RRC advisory-fetch read recovery

## Objective and measured scope extension

Complete the authorized queue/paste release through native RRC. The original
mission permits a shared blocking repair only when measured and unavoidable.
The attached v0.24.13 epoch stopped after six passed gates because `cargo audit`
could not fetch the RustSec database. Its transport diagnostic was classified
unknown and the controller exited to `NeedMoreEvidence`; no push/tag occurred.
A subsequent Git fetch observation and exact `cargo audit` both succeeded.
This is evidence of a recovered read, not an official service outage.

Source before correction: `8c0f7bd65e53f786e9525c6dec5c272f26bb952e`.
Failed epoch: `20261012T051709.716449587Z-8c0f7bd65e53`. Owner exited safely;
the native TUI was closed with `/quit` after settlement. No journal, retry budget,
user installation or dirty primary checkout was changed.

## Root cause and implemented boundary

`release_executor::advance_release` had no read-only diagnostic recovery for
this exact fetch failure. Generic unknown classification correctly prevents
source repair; a local transport read needs a separately bounded observation.
The fixed shared path accepts only `advisories` with the exact `cargo audit`
command and an explicit advisory Git transport diagnostic. One new audit read
is reserved by `mutation.local_advisory_read_retries` and the existing targeted
diagnostic budget, persisted before dispatch. Both native hosts use this shared
path, existing permission/firewall/governor/cancellation handling and executor.
Original failure evidence remains; successful audit output supplies a hashed
proof and a dependency/toolchain state-change receipt. No source/CI retry
admission is reset, no remote write is retried, and no vulnerability is ignored.
A repeated failure pauses through the existing diagnosis path. Certificates,
authentication, security findings, other commands/gates and controller stops
remain ineligible. Legacy records default the new counter to zero.

## Methods and exact evidence

- Native failed gate: `cargo audit`, advisory fetch I/O error; six of eight
  local gates passed, push/tag/publication flags false.
- `git ls-remote https://github.com/RustSec/advisory-db.git HEAD`: exit 0,
  `7eebec69c352c7191b1f13eb95dd510eeca5d1de`.
- Exact versioned-workspace `cargo audit`: exit 0, loaded 1,296 advisories and
  scanned 554 lockfile dependencies. Log:
  `/tmp/vesper-queue-native-advisory-fetch-diagnostic.log`.
- Red-before-green production-flow fixture: exit 101, zero passed/one failed.
  `/tmp/vesper-queue-advisory-recovery-red.log`.
- Focused all-feature harness tests: four passed/zero failed.
  `/tmp/vesper-queue-advisory-recovery-focused.log`.
- Full sequential final-source verification: all seven mandatory gates passed.
  The current workspace run includes 39 scoped regressions, all passing, and
  the real terminal test also passed under Rust 1.88. Older receipts remain
  historical. Exact source and log digests are in the
  [verification receipt](2026-10-12-rrc-advisory-fetch-read-recovery-receipts.json);
  the [failed native epoch](2026-10-12-rrc-advisory-fetch-read-recovery-native-failed.json)
  preserves the actual blocker before any new admission.

| Final-source gate | Exit | Seconds |
| --- | --- | --- |
| `cargo xtask acceptance` | 0 | 243.06 |
| `cargo xtask verify` | 0 | 1034.15 |
| `cargo xtask msrv` | 0 | 893.7 |
| `cargo deny --all-features check` | 0 | 1.78 |
| `cargo audit` | 0 | 3.32 |
| `cargo fmt --all -- --check` | 0 | 1.78 |
| `git diff --check` | 0 | 0.03 |

The ladder used two Cargo jobs and two Rust test threads sequentially.
Existing ignored cases are recorded and are not completion evidence.

## Files and DOX

Production: `crates/vesper-harness/src/release_executor.rs` and
`release_recovery.rs`; tests are colocated in the executor module. Nearest
harness DOX, RRC PRD, queue/paste PRD/report and foundation evidence index are
updated with this implementation before release CI. Parent ownership/indexes
remain unchanged. No provider, host-local release shortcut, VRO-19, installation
or release-policy weakening is introduced.

## Unresolved and readiness effect

Final-source verification is complete; a fresh native exact-SHA release remains
pending. Queue live
Windows/macOS and live-provider/delegated proofs remain distinct and unexecuted.
This bounded repair does not claim all RRC failures are automatically repairable.
**READY FOR LIVE ACCEPTANCE: YES.** Publication still requires native gates,
complete exact-SHA hosted matrices and registry/report closeout.
