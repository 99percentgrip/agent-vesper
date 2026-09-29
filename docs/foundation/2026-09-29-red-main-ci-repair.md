# Red main CI repair

**Date:** 2026-09-29

**Status:** IMPLEMENTED AND LOCALLY VERIFIED; EXACT-COMMIT GITHUB RECEIPTS ARE REPORTED AT DELIVERY

**Affected workflow:** `five-target-foundation` run `36534993336` on documentation-closeout commit `0b5630d271965e7d9df3a0a09c116c7f8c44042e`

**Related release record:** [`2026-09-29-v0.24.4-combined-corrective-release.md`](2026-09-29-v0.24.4-combined-corrective-release.md)

## Objective

Repair every substantive macOS Intel failure exposed by the three attempts of
run `36534993336`, without weakening process-tree cleanup, bounded settlement,
or LM Studio request-body assertions. Restore `main` to green while leaving the
v0.24.4 tag, release assets, registry state, credentials, and Alex's local
installation unchanged.

## Methods and commands

- Queried all attempts and jobs with `gh run view 36534993336`.
- Downloaded failed-job logs with `gh run view ... --log-failed`.
- Read the owned process-group implementation in `command-group` 5.0.1 and
  traced `RunCommand::run_bounded` settlement behavior.
- Compared the ACP LM Studio fixture with the TUI fixture, which already cleared
  inherited nonblocking state on accepted sockets.
- Ran targeted stress loops, the full command-settlement matrix, architecture,
  clippy, the all-feature workspace suite, and canonical `cargo xtask verify`.

## Diagnosis

### Attempt 1: late fork after the first Unix group signal

The first macOS Intel attempt failed
`cancellation_preserves_partial_output_and_next_command_runs` because
`cancelled.marker` was written after `RunCommand` returned
`cleanup=verified`. The previous implementation treated one successful
`killpg(SIGKILL)` submission as proof of full group teardown. A shell can fork
between that syscall and signal delivery; the new child does not inherit the
pending signal and can retain the pipes until it writes the marker.

The repair keeps the existing 2.5-second settlement budget and strong marker
assertion. During the same loop that reaps the leader and drains both pipes, it
re-signals a still-present Unix process group until `ESRCH` proves that the
owned group is absent. Unknown signal or wait errors remain fail-closed as
`cleanup=uncertain`. Windows retains its Job Object path.

### Attempt 2: inherited nonblocking accepted socket on macOS

The second attempt failed
`selected_model_reaches_the_real_http_body_instead_of_launch_model` with:

```text
Os { code: 35, kind: WouldBlock, message: "Resource temporarily unavailable" }
```

The fixture deliberately made its listener nonblocking to bound `accept`.
macOS may propagate that state to the accepted socket; applying a read timeout
does not clear `O_NONBLOCK`. The ACP fixture now explicitly restores blocking
mode before its bounded request read, matching the already-correct TUI fixture.
The test still drives the real AgentLoop and adapter and still asserts that the
wire body contains `picked-model`, not the launch model.

### Attempt 3

Attempt 3 was cancelled at Alex's direction before completion. It supplied no
acceptance result and is preserved as cancelled, not passed.

## Files

- `crates/vesper-agent/src/tools.rs` — verifies Unix process-group absence while
  concurrently reaping and draining inside the existing settlement deadline.
- `crates/vesper-agent/AGENTS.md` — records the durable Unix settlement contract.
- `apps/agent-vesper-acp/src/lmstudio_provider.rs` — restores blocking mode on
  the accepted macOS fixture socket.
- `apps/agent-vesper-acp/AGENTS.md` — records the fixture portability contract.
- `docs/foundation/2026-09-29-red-main-ci-repair.md` — this execution report.
- `docs/foundation/evidence-index.md` — indexes this repair.
- `docs/foundation/AGENTS.md` — records report ownership.
- `docs/foundation/2026-09-29-post-release-quality-check-investigation.md` and
  `docs/foundation/2026-09-29-v0.24.4-combined-corrective-release.md` — link the
  follow-up without rewriting historical receipts.

## Exact evidence

### Historical red receipts

```text
run 36534993336 attempt 1
macos-intel job 109296887609: failure
cancellation_preserves_partial_output_and_next_command_runs:
owned descendant survived cleanup and wrote .../cancelled.marker

run 36534993336 attempt 2
macos-intel job 109307530447: failure
selected_model_reaches_the_real_http_body_instead_of_launch_model:
Os { code: 35, kind: WouldBlock, message: "Resource temporarily unavailable" }

run 36534993336 attempt 3
macos-intel job 109310766425: cancelled
```

### Targeted local receipts

```text
cargo test -p vesper-agent --test command_settlement
9 passed; 0 failed

cancellation_preserves_partial_output_and_next_command_runs
12 consecutive targeted iterations: pass

ACP selected_model_reaches_the_real_http_body_instead_of_launch_model
100 consecutive targeted iterations: pass

cargo xtask architecture
architecture boundaries validated for 31 packages

cargo clippy --workspace --all-targets --all-features -- -D warnings
exit 0

cargo test --workspace --all-features
exit 0

cargo xtask verify
fmt: pass
clippy: pass
workspace all-features tests: pass
architecture: pass
```

GitHub's required push workflows run against the report-bearing repair commit.
Their exact run and job receipts are intentionally reported in the delivery
summary rather than creating a second evidence-only commit that would trigger
another recursive full workflow cycle.

## Deviations

- The macOS-specific failure cannot be reproduced authoritatively on the Linux
  development host. The repair is grounded in the exact macOS errno and POSIX
  socket/process semantics; the five-target GitHub matrix is the authoritative
  macOS execution gate.
- The TUI fixture already restored blocking mode and required no source change.
- Two environment-dependent all-feature tests remained intentionally ignored by
  their existing contracts: they require a real container runtime and bundled
  immutable image. No ignore or assertion was added by this repair.
- No retry, sleep, timeout increase, assertion removal, or test exclusion was
  used to obtain a pass.

## Unresolved items

- No implementation item remains open locally.
- Cross-platform readiness depends on the required GitHub workflows for the
  pushed repair commit; delivery must preserve any failure rather than claim
  green prematurely.

## Readiness effect

The two concrete races exposed by run `36534993336` are repaired locally while
retaining the original cleanup and wire-body guarantees. The v0.24.4 release,
tag, assets, registry state, credentials, and Alex's local installation are
unchanged. Final `main` readiness is determined by the report-bearing commit's
required GitHub workflow results reported at delivery.
