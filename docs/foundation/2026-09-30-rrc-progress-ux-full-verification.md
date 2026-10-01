# RRC progress UX full-verification repair — execution report

**Date:** 2026-09-30  
**Status:** Full repository verification passed locally on Linux; no candidate or release action was performed.  
**Scope boundary:** This work validates the current uncommitted RRC resource-governor, terminal-ownership, and persisted-progress source in `.worktrees/rrc-resource-governor`. It does not claim Alex-operated live terminal acceptance, macOS/Windows resource-backend or UX evidence, a release-profile candidate build, a release, publication, tag, push, installation, live-provider call, or VRO-19 work.

## Objective

Complete deterministic repository verification for the RRC progress-UX repair before any candidate action. Verify that the real-PTY terminal-ownership regression is stable under the all-features suite while retaining the required proof: release-child output is piped, terminal controls are inert, carriage-return updates are captured, output stays inside RUN, and settled work no longer presents as active.

## Findings and repair

The first complete `cargo xtask verify` attempt failed in the Unix real-PTY test:

```text
error: 1 target failed:
    `-p agent-vesper-tui --test rrc_terminal_pty`

carriage-return progress was not captured as bounded telemetry
```

This was a genuine test-evidence defect, not a passing verification result. The producer ended each pipe with separate long ANSI and CR marker labels. At the narrow real RUN rail, the latest bounded record could be truncated after the ANSI label before its separate CR label. The existing test could therefore fail based on the last-reader/tail projection even though the production capture path was sound.

`apps/agent-vesper-tui/tests/rrc_terminal_pty.rs` now terminates both stdout and stderr with compact `ANSI_CAPTURE CR_CAPTURE` labels on both sides of a real carriage-return boundary. Both labels fit in the narrow RUN row, while the fixture retains 48 noisy stdout records, 48 noisy stderr records, CSI/OSC controls, carriage-return progress, C0 controls, a real 120×36 PTY, alternate-screen checks, RUN-rectangle confinement, no premature `Ready`, and stale-telemetry rejection. This strengthens determinism without removing a production safety assertion.

## Files

- `apps/agent-vesper-tui/tests/rrc_terminal_pty.rs` — deterministic compact terminal/CR terminus labels for the all-features PTY regression.
- `docs/foundation/2026-09-30-rrc-progress-ux-full-verification.md` — this execution report.
- `docs/foundation/evidence-index.md` — durable evidence-index entry.
- `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md` — current RRC verification-status trace.
- `docs/AGENTS.md` and `docs/foundation/AGENTS.md` — documentation ownership/index updates.

## Methods and exact evidence

All commands ran from `.worktrees/rrc-resource-governor` with `CARGO_BUILD_JOBS=1` and `RUST_TEST_THREADS=1`. The verification commands use offline fixtures and synthetic/loopback adapters; no release command, installer, provider credential operation, or production network call was invoked.

### Targeted red-to-green PTY evidence

```text
cargo test -p agent-vesper-tui --all-features --test rrc_terminal_pty -- --nocapture

Before repair:
test rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty ... FAILED
carriage-return progress was not captured as bounded telemetry
```

After the compact paired-marker repair:

```text
cargo fmt --all -- --check
for attempt in $(seq 1 20); do
  cargo test -p agent-vesper-tui --all-features --test rrc_terminal_pty -- --nocapture
done

PTY_ALL_FEATURES_20X_OK
```

Every one of the 20 attempts reported:

```text
test rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### Scoped RRC receipts

```text
cargo clippy -p agent-vesper-tui --all-targets -- -D warnings
cargo test -p vesper-harness --lib
cargo test -p agent-vesper-tui --lib
cargo test -p agent-vesper-tui --bin agent-vesper-tui
cargo test -p agent-vesper-acp --lib
cargo xtask acceptance
cargo fmt --all -- --check
git diff --check
```

The relevant receipts were:

```text
vesper-harness: 183 passed; 0 failed; 2 ignored
agent-vesper-tui library: 268 passed; 0 failed
agent-vesper-tui binary: 166 passed; 0 failed
agent-vesper-acp library: 61 passed; 0 failed
Acceptance regression gate: 43 exact cases passed in 21129 ms.
Offline fixture model cost: zero; live-model effectiveness is not measured.
```

### Complete repository verification

```text
cargo xtask verify
```

The successful rerun completed the repository pipeline: formatting, strict workspace all-target/all-feature Clippy, workspace all-feature tests, workspace doc tests, fixture validation/index/coverage, contracts, architecture, naming guard, GLM/runtime/ACP/session verification, and the enforced acceptance gate. Its final acceptance receipt was:

```text
acceptance verified: rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty
acceptance progress: 43/43 — rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty
Acceptance regression gate: 43 exact cases passed in 21841 ms.
Offline fixture model cost: zero; live-model effectiveness is not measured.

FULL_XTASK_VERIFY_OK
```

## Deviations and unresolved items

1. The first full verification was red and is retained above. Only the post-repair full `cargo xtask verify` success is completion evidence.
2. Explicit environment-gated integration tests remained ignored by their existing contracts (for example, real container/runtime, public-web, and device-dependent checks). Their ignored status is not treated as executed evidence.
3. Linux-local deterministic verification does not supply Alex-operated live terminal acceptance, cross-platform UX evidence, macOS/Windows Host Resource Governor backends, or release exact-SHA CI evidence.
4. The worktree remains intentionally uncommitted and contains the broader prior RRC/governor/progress implementation. This report creates no clean candidate identity and does not authorize one.

## Readiness effect

The current RRC progress-UX source has a reproducible all-features real-PTY safety regression and a passing complete repository verification receipt. Within the documented Linux-local, non-publishing scope, deterministic verification is complete before any candidate action: terminal output ownership, truthful RUN state, typed progress, resource-governor paths, TUI/ACP composition, and the enforced 43-case acceptance gate all passed.

This does **not** convert local verification into release readiness. Alex-operated acceptance, cross-platform evidence, exact-commit release workflows, and all publication prerequisites remain separate requirements.
