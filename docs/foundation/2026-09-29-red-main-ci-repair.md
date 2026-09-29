# Red main CI repair

**Date:** 2026-09-29

**Status:** ROOT-CAUSE CORRECTION IMPLEMENTED AND LOCALLY VERIFIED; EXACT-COMMIT GITHUB GATES PENDING

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
- Checked Apple's [XNU `killpg1` implementation](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/kern_sig.c#L1642-L1721)
  at pinned commit `f6217f891ac0bb64f3d375211650a4c1ff8ca1ea`, including the zombie exclusion
  at `bsd/kern/kern_sig.c:1647-1665,1702-1718`.
- Added regression tests before the final correction and recorded their expected
  compile failure against the pre-correction state.
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
re-signals the Unix process group while settlement remains incomplete. Verified
cleanup requires an accepted signal, leader reaping, and both inherited pipes
reaching EOF; this avoids mistaking Darwin's unreaped process-group zombies for
live executable descendants. Unknown signal or wait errors remain fail-closed as
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

### Two pushed repairs: wrong Darwin error-state model

The first repair commit `af662eac572ba16763520c524dd131b1f9ddbe88`
re-signalled the group but required `killpg` to return `ESRCH` before accepting
settlement. Run `36543566821` failed normal successful command settlement on both
macOS architectures.

The second repair commit `d097bdfa531e442a14adc215616baa804a0d4047`
correctly stopped requiring eventual `ESRCH` after one accepted signal, but still
treated Darwin `EPERM` as an unknown cleanup failure. Run `36549489767` repeated
the same ACP host failure on both macOS architectures.

Pinned XNU source identifies the exact cause. `killpg1` finds the process group,
but its group iterator excludes `SZOMB` members (`kern_sig.c:1709-1715`). With
no signalable member counted, POSIX mode returns `EPERM`, not `ESRCH`
(`kern_sig.c:1718`). Therefore an ordinary command whose group is retained only
by a Darwin zombie was incorrectly classified as uncertain even after exit 0 and
both inherited pipes reached EOF.

The final correction classifies Darwin raw error 1 only as the platform's
no-signalable-member terminal result. Verified settlement still requires the
leader to be reaped and both inherited pipes to reach EOF. The loop checks that
complete proof before issuing another signal, so a completed command is not
invalidated by an unnecessary final `killpg`. Linux keeps the `ESRCH` rule;
Windows keeps its Job Object path; every other signal/wait failure remains
fail-closed. No sleep, timeout extension, retry, ignore, or weaker assertion was
added.

## Files

- `crates/vesper-agent/src/tools.rs` — classifies pinned Darwin `EPERM`
  semantics, stops signalling once the complete proof exists, and adds pure
  regression coverage while retaining the existing settlement deadline.
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

run 36543566821 on first repair commit af662ea
macos-apple-silicon job 109324478181: failure
acp_host_registry_settles_large_command_output_and_recovers:
cleanup=uncertain after leader exit 0 and stdout/stderr EOF
macos-intel job 109324478201: failure
tui_host_registry_settles_large_command_output_and_recovers:
cleanup=uncertain after leader exit 0 and stdout/stderr EOF
linux-x86_64, linux-arm64, windows-x86_64: success

run 36549489767 on second repair commit d097bdf
macos-apple-silicon job 109343880064: failure
macos-intel job 109343880176: failure
acp_host_registry_settles_large_command_output_and_recovers:
exit_status=exit status: 0; cleanup=uncertain; stdout=7; stderr=0
linux-x86_64, linux-arm64, windows-x86_64: success
```

### Root-cause source receipt

```text
Apple XNU f6217f891ac0bb64f3d375211650a4c1ff8ca1ea
bsd/kern/kern_sig.c:1709-1715 excludes SZOMB from pgrp iteration
bsd/kern/kern_sig.c:1718 returns EPERM when nfound == 0 in POSIX mode
```

### Regression-first receipt

Before the correction, the two new unit tests failed to compile because the
Darwin classification and settlement-order predicates did not exist:

```text
error[E0425]: cannot find function `process_group_cleanup_complete_for`
error[E0425]: cannot find function `command_settlement_complete`
```

After the correction:

```text
darwin_zombie_only_group_error_is_a_completed_cleanup: 1 passed
settled_command_does_not_require_another_group_signal: 1 passed
```

### Targeted local receipts

```text
cargo test -p vesper-agent --test command_settlement
9 passed; 0 failed

cancellation_preserves_partial_output_and_next_command_runs
30 consecutive final-correction iterations: pass

acp_host_registry_settles_large_command_output_and_recovers
20 consecutive final-correction iterations: pass

tui_host_registry_settles_large_command_output_and_recovers
20 consecutive final-correction iterations: pass

ACP selected_model_reaches_the_real_http_body_instead_of_launch_model
100 consecutive final-correction iterations: pass

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

cargo xtask acceptance
Acceptance regression gate: 23 exact cases passed in 6712 ms
Offline fixture model cost: zero
```

GitHub's required push workflows run against the report-bearing repair commit.
Their exact run and job receipts are intentionally reported in the delivery
summary rather than creating a second evidence-only commit that would trigger
another recursive full workflow cycle.

## Deviations

- The macOS-specific failure cannot be reproduced authoritatively on the Linux
  development host. The repair is grounded in pinned XNU implementation evidence
  and the exact CI failure; the five-target GitHub matrix is the authoritative
  macOS execution gate.
- An attempted independent delegated review could not start because the configured
  OpenAI worker model was unavailable in the current account model list. No
  delegated assurance is claimed.
- The TUI fixture already restored blocking mode and required no source change.
- Two environment-dependent all-feature tests remained intentionally ignored by
  their existing contracts: they require a real container runtime and bundled
  immutable image. No ignore or assertion was added by this repair.
- `cargo xtask verify` produced an untracked test lock at
  `apps/agent-vesper-acp/.config/agent-vesper/xai-credentials.lock`; it was
  removed before commit and did not touch user state.
- No retry, added sleep, timeout increase, assertion removal, or test exclusion
  was used to obtain a pass.

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
