# RRC terminal-ownership repair — execution report

**Date:** 2026-09-30  
**Status:** Implemented, regression-tested, and fresh non-publishing prerelease TUI built; Alex-operated live acceptance remains unexecuted.  
**Release boundary:** No release, publication, tag, push, installation, or VRO-19 work was performed.

## Objective

Correct the failed live acceptance in which release/Cargo output bypassed the Ratatui renderer, wrote directly into the alternate screen, damaged the layout, and coexisted with an idle `RUN / Ready` presentation.

The enforced display path is now:

```text
release child stdout/stderr
→ owned OS pipes
→ concurrent bounded readers
→ terminal-control sanitization and secret redaction
→ registered ReleaseWorkerActivity snapshot
→ ViewModel.background_task
→ Ratatui frame
```

No release child, background tracing subscriber, or cognition diagnostic may write directly to the interactive terminal while Ratatui owns it.

## Root cause and leaking path

The failed acceptance exposed a terminal-ownership defect rather than a telemetry-content defect. The interactive process did not enforce one exclusive terminal writer as a tested invariant: background diagnostics were configured to write to process stderr while the alternate screen was active, and release subprocess confinement had no full-PTY regression capable of detecting an inherited/direct stdout or stderr route. Consequently, a direct writer could bypass `ReleaseWorkerActivity` and Ratatui. The renderer then had no synchronized worker snapshot for that output and could display the idle `Ready` state at the same time.

The leaking class was:

```text
background/release writer
→ TUI process stdout or stderr / inherited terminal descriptor
→ alternate-screen terminal
```

That path bypassed capture, control-sequence neutralization, redaction, bounded retention, registered worker state, `ViewModel`, and Ratatui layout.

## Repair

### Release subprocess ownership

`NativeReleaseExecutor::run_command` now explicitly configures every local release gate with:

- stdin: `Stdio::null()`;
- stdout: `Stdio::piped()`;
- stderr: `Stdio::piped()`.

The parent removes both pipes from the child handle immediately and drains them concurrently on owned reader threads, preventing pipe-capacity deadlock. The readers retain at most 4 MiB per stream for settled command evidence. Complete LF and CR-delimited records update the synchronized worker activity snapshot while the child is alive.

Before entering telemetry, records pass through terminal sanitization that strips CSI, OSC, DCS/SOS/PM/APC and other control bytes. Existing secret redaction and the eight-line/240-character telemetry bounds are then applied. The settled `CommandOutput` is assembled only after the child exits and both readers join. Cancellation kills and waits for the owned process group/Job Object before returning.

### Exclusive interactive terminal writer

The interactive tracing subscriber now uses `io::sink` rather than stderr. Cognition diagnostics formerly emitted with `eprintln!` now use tracing, so they follow that non-terminal interactive sink. Ratatui/crossterm remains the only interactive stdout/stderr presentation owner.

### One worker state for RUN and telemetry

The event/render loop calls one `release_background_task(workspace)` projection per frame. That projection reads `active_release_worker_for_workspace` and constructs the complete `ViewModel.background_task`. Top-level RUNNING/READY state, status line, screen-reader state, TODO rail, RUN panel, elapsed/activity values and recent output all consume that same snapshot. A registered release worker therefore cannot coexist with `Ready` or `No active tasks`; after registry settlement, the next frames return to idle normally.

## PTY regression and causal mutation proof

`apps/agent-vesper-tui/tests/rrc_terminal_pty.rs` launches the real debug TUI in a 120×36 pseudo-terminal and runs a noisy registered release child that emits, on both stdout and stderr:

- 48 compilation/test lines and overlong paths;
- CSI clear-screen bytes;
- OSC title bytes;
- carriage-return progress redraws;
- C0 control bytes;
- delayed settlement.

The test parses the actual terminal byte stream and verifies alternate-screen entry, absence of verbatim child control sequences, captured inert telemetry inside the RUN coordinates, active-before-idle ordering, no `Ready` before worker settlement, and no stale telemetry redraw after settlement.

A mutation check temporarily changed the production release child from piped stdout/stderr to inherited stdout/stderr. The PTY regression failed with exit 101. The source was restored byte-for-byte (`release_executor.rs` SHA-256 `699920a277ee7ab7d0895140dd7dd79a34e9c2e0baab2f30b2adf2a522bb00c7`), and the same regression passed:

```text
MUTATION_TEST_EXPECTED_FAILURE exit=101
rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty ... FAILED
restored production source:
rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty ... ok
```

This establishes that the test detects the rejected direct-terminal-output class rather than merely exercising a green renderer.

## Verification receipts

```text
cargo test -p vesper-harness --lib
168 passed; 0 failed; 2 ignored

cargo test -p agent-vesper-tui --lib
268 passed; 0 failed

cargo test -p agent-vesper-tui --test rrc_terminal_pty
1 passed; 0 failed

cargo check -p agent-vesper-acp
Finished `dev` profile

cargo check -p agent-vesper-tui --bin agent-vesper-tui
Finished `dev` profile

cargo xtask acceptance
Acceptance regression gate: 37 exact cases passed in 62112 ms.
```

The two ignored harness tests are the pre-existing explicit workspace-layout and contained-runtime probes. Final formatting, clippy, diff-whitespace and documentation-link checks are recorded in Closeout below.

## Fresh prerelease TUI

Build command:

```text
CARGO_TARGET_DIR=/home/Alex/Projects/agent-vesper-prerelease-builds/rrc-terminal-ownership-20260930T062127Z \
  cargo build --locked --release -p agent-vesper-tui --bin agent-vesper-tui
Finished `release` profile [optimized] target(s) in 1m 50s
```

Preserved candidate:

```text
Path: /home/Alex/Projects/agent-vesper-prerelease-candidates/rrc-terminal-ownership-20260930T062127Z/agent-vesper-tui-rrc-terminal-ownership
SHA-256: 7e5cd4f3bc5dd0d3e15473b5f9cae0a202ad8f9fc5ad413b2053652c9351e0c1
Version: agent-vesper-tui 0.24.4
```

The checksum differs from the rejected live-acceptance artifact. `cmp -s` proved the preserved candidate is byte-identical to the fresh external-target output, and candidate-local `sha256sum -c SHA256SUMS` returned:

```text
agent-vesper-tui-rrc-terminal-ownership: OK
```

Source identity was identical before and after compilation:

```text
SOURCE_HEAD=01d2df045a7c30253bea8e613e9840b9f62d053b
TRACKED_DIFF_SHA256=97252b8ce9415e9dde5109ebb8df5a58fbd9b3cfb70f1894eaa27c3fe40a0a37
STATUS_INVENTORY_SHA256=fbc5fa50ed4cf11c915a091fad0210cc318a72ea8d02bdd427c8498ec6b19d96
```

Candidate-local receipts are `source-identity.txt`, `post-build-source-identity.txt`, `version.txt`, and `SHA256SUMS`.

## Files changed by this corrective work

- `crates/vesper-harness/src/release_executor.rs`
- `apps/agent-vesper-tui/src/main.rs`
- `apps/agent-vesper-tui/tests/rrc_terminal_pty.rs`
- `xtask/src/main.rs`
- applicable `AGENTS.md` contracts
- the owning RRC PRD and foundation evidence index
- this report

The worktree contained earlier RRC source and evidence changes before this correction. They were preserved rather than reset or overwritten.

## Deviations and unresolved items

- The exact user-observed terminal bytes from the rejected process were not retained as a machine-readable capture. The live observation is therefore the incident evidence; the new PTY test deterministically reproduces and kills the inherited-stream failure class.
- Alex-operated live acceptance of the new binary remains unexecuted and must not be inferred from local PTY automation.
- This candidate is Linux x86-64 local evidence, not fresh five-target execution evidence.
- No release workflow, provider call, GitHub mutation, installer, or local installation was run.

## Closeout

```text
cargo fmt --all -- --check
passed

git diff --check
passed

cargo clippy -p vesper-harness -p agent-vesper-tui --all-targets -- -D warnings
Finished `dev` profile; zero warnings

cargo test -p agent-vesper-tui --test rrc_terminal_pty
1 passed; 0 failed

sha256sum -c SHA256SUMS
agent-vesper-tui-rrc-terminal-ownership: OK

DOX_REPORT_LINKS=pass
```

The final DOX pass rechecked the root, harness, TUI, xtask, docs and foundation contracts. They now record exclusive terminal ownership, piped/sanitized child streams, the mandatory PTY regression, the rejected historical candidate, this corrective report and the replacement artifact. No applicable owning document was intentionally left stale.

## Readiness effect

The source now enforces a single interactive terminal presentation path and has a full-PTY regression that fails when release stdout/stderr inherit the terminal. The new candidate is suitable for Alex-operated live RRC acceptance. It is not a release or installation candidate until that separate acceptance and the repository's release gates are explicitly authorized and completed.
