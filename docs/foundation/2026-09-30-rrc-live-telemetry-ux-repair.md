# RRC live telemetry UX repair — execution report

> **Superseded after failed live acceptance.** The candidate recorded here allowed raw
> release output to corrupt the alternate screen and could show `RUN / Ready` during
> active compilation. It must not be reused. The corrective evidence and replacement
> artifact are in [`2026-09-30-rrc-terminal-ownership-repair.md`](2026-09-30-rrc-terminal-ownership-repair.md).

**Date:** 2026-09-30  
**Status:** Historical implementation evidence; terminal-ownership acceptance failed and the candidate is rejected.  
**Release boundary:** No release, publication, tag, push, installation, live Alex acceptance, or VRO-19 work was performed.

## Objective

Repair the visually static Release Recovery Controller experience after live acceptance proved that the controller task and `cargo xtask verify` subprocess tree were genuinely active. Project real, bounded controller/process telemetry into the TUI without inferring a percentage or treating process existence alone as progress.

## Implementation

### Controller-owned telemetry

`crates/vesper-harness/src/release_executor.rs` now retains a bounded `ReleaseWorkerActivity` alongside the existing registered worker handle and cancellation token. The snapshot includes:

- release stage/detail;
- current local gate and exact configured command;
- current child/test executable parsed from real Cargo lifecycle output;
- current-gate start time and last controller/output activity time;
- completed and total local gates;
- candidate version transition and candidate SHA after commit;
- persisted retry-budget and failure-fingerprint state;
- process-alive state;
- the newest eight secret-redacted output lines, each capped at 240 characters.

Native gate stdout and stderr still spool to temporary files for bounded settlement evidence. Separate file descriptions now permit the controller to tail new bytes while the child runs. Every complete output line updates the live activity clock and bounded output deque. `Running ... (target/.../<test>-<hash>)` lifecycle lines update the current executable. Spawn and exit are explicit process transitions. No filesystem polling or percentage inference was added.

### TUI projection

`apps/agent-vesper-tui/src/main.rs` reads the registered worker snapshot on every existing 250 ms event/render cycle. `apps/agent-vesper-tui/src/ui.rs` renders the real values in the right-side RUN area:

- Stage, Gate, Command, Current child;
- ticking Elapsed and Activity age;
- explicit `No output for … — process still alive` after sixty seconds of genuine silence;
- Gates completed/total;
- Candidate version transition and SHA when available;
- Retry budget and failure fingerprint when present;
- bounded Recent output.

The renderer continues to suppress `Ready` and `No active tasks` while registered release work exists.

## Regression evidence

### Production admission/runtime path

`release_recovery::tests::tui_natural_release_retains_registered_controller_through_local_gate_progress` uses the real natural-language TUI admission seam with a dirty primary checkout, obsolete failed epoch, newer completed objective, isolated release workspace, registered worker, and real Cargo child processes. The fixture now:

1. runs `workspace-verify` for multiple seconds;
2. emits real child lifecycle output for `test_a`, then `test_b`;
3. proves gate elapsed time increases without restarting the host;
4. proves last-activity age increases during silence and resets on the next output;
5. proves the child name changes;
6. settles workspace verification and advances to a live `acceptance` gate;
7. observes completed gates move from `0/2` to `1/2`;
8. retains cancellation and dirty-primary invariance.

### Renderer/runtime projection

`ui::tests::registered_release_task_replaces_ready_and_no_active_tasks` redraws the same terminal/model with `routing_quality_eval`, then `test_b`, increased elapsed time, changed activity, and gate advancement. It verifies candidate version rendering, prolonged-silence wording, and continued exclusion of both idle labels.

### Retention bound

`release_executor::tests::release_telemetry_retains_only_the_newest_bounded_output_lines` feeds 32 lines and verifies that only lines 24–31 remain, then verifies the 240-character per-line cap.

## Commands and exact receipts

### Focused regressions

```text
cargo test -p vesper-harness tui_natural_release_retains_registered_controller_through_local_gate_progress --lib -- --nocapture
1 passed; 0 failed

cargo test -p agent-vesper-tui registered_release_task_replaces_ready_and_no_active_tasks --lib
1 passed; 0 failed

cargo test -p vesper-harness release_telemetry_retains_only_the_newest_bounded_output_lines --lib
1 passed; 0 failed
```

### Relevant suites

```text
cargo test -p vesper-harness --lib
168 passed; 0 failed; 2 ignored

cargo test -p agent-vesper-tui --lib
268 passed; 0 failed

cargo check -p agent-vesper-acp
Finished `dev` profile

cargo check -p agent-vesper-tui --bin agent-vesper-tui
Finished `dev` profile

cargo xtask acceptance
Acceptance regression gate: 36 exact cases passed in 18128 ms.

cargo clippy -p vesper-harness -p agent-vesper-tui --all-targets -- -D warnings
Finished `dev` profile; zero warnings

cargo fmt --all -- --check
passed

git diff --check
passed
```

The two ignored harness tests are the pre-existing explicit workspace-layout and contained-runtime acceptance probes; neither is an RRC telemetry test.

## Fresh prerelease candidate

Build command:

```text
CARGO_TARGET_DIR=/home/Alex/Projects/agent-vesper-prerelease-builds/rrc-live-telemetry-20260930T051114Z \
  cargo build --locked --release -p agent-vesper-tui --bin agent-vesper-tui
Finished `release` profile [optimized] target(s) in 1m 45s
```

Preserved candidate:

```text
Path: /home/Alex/Projects/agent-vesper-prerelease-candidates/rrc-live-telemetry-20260930T051114Z/agent-vesper-tui-rrc-live-telemetry
SHA-256: a5d00e64914032887b4dc2c8146640dfa2bac6ae4e80d3e1ebbc2199c9b0ab86
Version: agent-vesper-tui 0.24.4
```

Build-input identity captured immediately before compilation:

```text
SOURCE_HEAD=01d2df045a7c30253bea8e613e9840b9f62d053b
TRACKED_DIFF_SHA256=37034a96b2170412cd00e82e38eb6ec64fa44401383d24e85734614496b0e748
STATUS_INVENTORY_SHA256=65654aad2298751971cdfd657ed421849ff119834116597453fc2903441d46fb
```

The preserved file is byte-identical to the fresh external-target build output. Its SHA differs from both the rejected static candidate `99de2f87e50abd696bac8b204539ec752f14ce9a41370ef8d44af7516c545f01` and the prior task-lifecycle candidate `8f15fa72e4288668043c188007188f94b1da7f29ed6d62886ecaf4409ff7fefd`.

Candidate-local receipts: `source-identity.txt`, `SHA256SUMS`, and `version.txt`.

## Files changed by this work unit

- `AGENTS.md`
- `crates/vesper-harness/src/release_executor.rs`
- `crates/vesper-harness/src/release_recovery.rs`
- `crates/vesper-harness/AGENTS.md`
- `apps/agent-vesper-tui/src/main.rs`
- `apps/agent-vesper-tui/src/ui.rs`
- `apps/agent-vesper-tui/AGENTS.md`
- `xtask/AGENTS.md`
- `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md`
- `docs/AGENTS.md`
- `docs/foundation/AGENTS.md`
- `docs/foundation/evidence-index.md`
- this report

The worktree already contained the earlier RRC autonomy/task-lifecycle changes and their reports; this unit built on them rather than resetting user work.

## Deviations and unresolved items

- Alex's live acceptance command was deliberately not executed.
- No fake progress percentage was added; total local gates are exact, while intra-gate work is represented by child/output/activity transitions.
- Child executable naming depends on lifecycle lines emitted by the owned command. When no such line is available, the real spawned command remains visible and the UI does not invent a child name.
- This Linux candidate is not cross-platform execution evidence. The existing five-target lifecycle evidence remains separate; the new telemetry code has local compile/test evidence only.
- Documentation/index updates made after the binary build are not part of the recorded build-input diff hash and do not affect executable code.

## Readiness effect

**Superseded verdict:** this report did not establish terminal ownership, and its candidate failed Alex-operated live acceptance. It provides historical telemetry evidence only and confers no current acceptance or readiness. The corrected verdict is owned by `2026-09-30-rrc-terminal-ownership-repair.md`.
