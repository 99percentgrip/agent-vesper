# RRC PTY stream-neutral test-oracle repair — execution report

**Date:** 2026-10-03
**Status:** Test-only correction committed; exact real-PTY regression passed 10/10; focused formatting, Clippy, and whitespace checks passed.
**Owning PRD:** [`../Agent_Vesper_Release_Recovery_Controller_PRD.md`](../Agent_Vesper_Release_Recovery_Controller_PRD.md)
**Authoritative base:** `43c937848745ec504b1f82c8e87fd7a664e2ccc0`
**Test correction commit:** `9a438ffc731e2bf1e70f53a06469bef0d790ad49`

## Objective

Repair only the scheduler-sensitive oracle in
`apps/agent-vesper-tui/tests/rrc_terminal_pty.rs`. Preserve the production
stdout/stderr capture architecture, eight-line telemetry capacity, sanitization,
watchdog behavior, terminal ownership, RUN confinement, settlement ordering, and
stale-output rejection. Do not resume or publish `v0.24.5`, build a candidate,
install anything, or touch VRO-19.

## Root cause and correction

The noisy PTY child emits 48 stdout `Compiling crate-*` records and 48 stderr
`Running test-*` records. Production drains those pipes concurrently into one
bounded eight-line tail. The old generic capture assertion required an
stdout-only `Compiling crate-*` write. A valid stderr-last reader schedule could
replace every stdout-only record in the tail while still retaining sanitized
child telemetry. The same source commit consequently passed the acceptance run
and later failed complete verification at the misleading assertion
`captured child output never reached telemetry`.

The fixture now appends the same compact `TELEMETRY_CAPTURE` record to both
stream suffixes immediately before each stream's existing compact ANSI/CR
terminus. Generic capture, RUN confinement, and stale-post-settlement checks use
that paired marker. The heavy per-stream noise and all existing CSI, OSC,
carriage-return, C0-control, alternate-screen, active-worker, premature-Ready,
and settled-idle checks remain. No production file changed.

Failure diagnostics now report a bounded tail of parsed terminal writes: at
most 32 entries and at most 160 characters per entry. The diagnostic is derived
from the terminal parser's observed writes rather than raw unsanitized child
bytes.

## Files

### Test correction

- `apps/agent-vesper-tui/tests/rrc_terminal_pty.rs` — paired stream-neutral
  telemetry marker, marker-based confinement/settlement assertions, and bounded
  parsed-write diagnostics.

### Evidence closeout

- `docs/foundation/2026-10-03-rrc-pty-stream-neutral-test-oracle-repair.md`
- `docs/foundation/evidence-index.md`
- `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md`
- `docs/foundation/AGENTS.md`
- `docs/AGENTS.md`

`apps/agent-vesper-tui/tests/AGENTS.md` is intentionally unchanged: the test's
owned production-path contract did not change; only the nondeterministic oracle
and failure diagnostics changed. Root and application DOX indexes are also
unchanged because no ownership boundary or child index changed.

## Methods and exact evidence

All commands ran in
`.worktrees/rrc-liveness-watchdog-20261002-2` with
`CARGO_BUILD_JOBS=2` and `RUST_TEST_THREADS=1`. The PTY command selected the
single named integration test with all TUI features, locked/offline resolution,
`--exact`, and one test thread:

```text
cargo test --locked --offline --all-features \
  -p agent-vesper-tui --test rrc_terminal_pty \
  rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty \
  -- --exact --test-threads=1
```

Ten independent Cargo invocations reported:

```text
PTY_RUN_01=PASS elapsed_ms=2222 exit=0
PTY_RUN_02=PASS elapsed_ms=1882 exit=0
PTY_RUN_03=PASS elapsed_ms=1852 exit=0
PTY_RUN_04=PASS elapsed_ms=1882 exit=0
PTY_RUN_05=PASS elapsed_ms=1839 exit=0
PTY_RUN_06=PASS elapsed_ms=1837 exit=0
PTY_RUN_07=PASS elapsed_ms=1850 exit=0
PTY_RUN_08=PASS elapsed_ms=1854 exit=0
PTY_RUN_09=PASS elapsed_ms=1841 exit=0
PTY_RUN_10=PASS elapsed_ms=1890 exit=0
```

Every invocation contained:

```text
test rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Focused checks after the ten-run regression:

```text
cargo fmt --all -- --check
FMT_FINAL_EXIT=0 elapsed_ms=1571

cargo clippy -p agent-vesper-tui --tests -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 27.04s
CLIPPY_EXIT=0 elapsed_ms=27140

git diff --check
DIFF_CHECK_EXIT=0
```

Documentation closeout then checked only links added by this work unit and the
new report, followed by a final whitespace check:

```text
ADDED_LOCAL_LINKS_CHECKED=4
MISSING_ADDED_LOCAL_LINKS=0
FINAL_DIFF_CHECK_EXIT=0
```

## Deviations and unresolved items

1. The first `cargo fmt --all -- --check` invocation returned 1 and showed only
   rustfmt wrapping for the new `unwrap_or_else` diagnostic. The test file was
   manually aligned to that output; the required check then passed. No semantic
   change followed the ten PTY executions.
2. Documentation commit preflight initially found four Markdown hard-break
   spaces in this report's header because the earlier unstaged `git diff --check`
   did not include the new file. The spaces were removed before commit, and the
   staged whitespace check passed.
3. An initial whole-file link sweep of the long-lived evidence index reported
   53 existing targets as missing; that over-broad result included URL-encoded
   path handling and historical entries outside this repair. The task-scoped
   check then resolved paths relative to each changed file and passed all four
   newly added local links. No historical evidence-index target was changed.
4. `cargo xtask verify`, broad acceptance, MSRV, candidate build, release,
   publication, push, tag, and installation were explicitly not run.
5. The ten-run result proves the requested Linux-local real-PTY race regression.
   It is not new evidence for production capture behavior on other operating
   systems and does not replace later broad verification if separately authorized.
6. No raw failed PTY trace existed from the original broad-verification failure;
   the repair follows the source-proven two-reader/eight-line ordering and the
   same-SHA pass/fail receipts.

## Readiness effect

The generic real-PTY telemetry oracle no longer depends on stdout winning the
shared bounded tail. Ten independent all-feature executions passed while
preserving the original terminal ownership and settlement safety checks. This
removes the diagnosed local test flake only. Production telemetry, watchdog,
release progression, release readiness, and `v0.24.5` status are unchanged, and
complete repository verification remains unexecuted for this commit.
