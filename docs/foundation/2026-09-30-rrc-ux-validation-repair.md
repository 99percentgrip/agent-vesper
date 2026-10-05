# RRC UX validation and deterministic-repair — execution report

**Date:** 2026-09-30  
**Status:** Locally verified on Linux; no release action was performed.  
**Scope boundary:** This follow-up validates the current RRC terminal-ownership and persisted-progress UX work in the isolated `rrc-resource-governor` worktree. It does not claim Alex-operated live acceptance, a candidate build, cross-platform UX execution, `cargo xtask verify`, a tag, push, publication, installation, provider call, or mutation of Alex's primary checkout.

## Objective

Validate the latest RRC UX path against the owning PRD's explicit boundaries: one interactive Ratatui writer; piped, sanitized child output; truthful RUN state; persisted typed progress; TUI/ACP shared status; and acceptance evidence that does not fabricate release readiness.

## Findings and repair

The audit found two real defects that required repair before an evidence-backed verdict was possible.

1. `cargo xtask acceptance` first stopped at TUI binary-test compilation. The new `TuiSession::release_milestone_cursor` field was absent from four explicit test-session initializers in `apps/agent-vesper-tui/src/main.rs` (four `E0063` errors). Every initializer now explicitly starts with `None`, matching the normal session construction and preserving the once-per-epoch/sequence milestone projection.
2. On the next full acceptance run, the real PTY test reached 42/43 then intermittently failed to find its ANSI marker in the bounded visible tail. This was a regression-test race, not a pass: independently drained stdout and stderr pipes may acquire the shared eight-line tail in either scheduler order. `rrc_terminal_pty.rs` still emits 48 noisy records per stream and the original CSI/OSC/carriage-return/control payloads. It now terminates **both** streams with the same sanitized terminal-control and carriage-return capture markers, then keeps the child alive for 400 ms. The final bounded tail therefore deterministically proves pipe capture and inert rendering regardless of reader order, while the test still rejects raw controls, writes outside RUN, premature `Ready`, and stale post-settlement telemetry.

The strict TUI all-target Clippy gate then reported 13 warnings in current RRC governor/progress source. The source was corrected rather than suppressed broadly:

- resource-refusal telemetry in `ResourceGovernorError` is boxed, preserving inspection via `telemetry()` while keeping the `Result` error bounded;
- concurrency and swap arithmetic use `clamp` and checked division;
- persisted progress aggregation uses explicit empty/non-empty branches and `contains` where appropriate; and
- the one retained `too_many_arguments` allow is documented on the release-worker composition boundary, whose explicit state, ports, cancellation and governor dependencies are intentionally passed together.

No release transition, admission, remote workflow, provider interaction, credential operation, installer, or user release-state operation was executed.

## Files changed by this validation repair

- `apps/agent-vesper-tui/src/main.rs` — complete test-only `TuiSession` initialization for the persisted milestone cursor.
- `apps/agent-vesper-tui/tests/rrc_terminal_pty.rs` — scheduler-order-independent paired terminal/CR capture terminus; existing full-PTY safety assertions retained.
- `crates/vesper-harness/src/host_resources.rs` — bounded refusal error representation and equivalent Clippy-safe policy arithmetic.
- `crates/vesper-harness/src/release_executor.rs` — documented composition-boundary lint exception.
- `crates/vesper-harness/src/release_recovery.rs` — equivalent explicit progress-unit aggregation and state membership checks.
- `docs/foundation/2026-09-30-rrc-ux-validation-repair.md`, `docs/foundation/evidence-index.md`, `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md`, `docs/AGENTS.md`, and `docs/foundation/AGENTS.md` — this evidence, linkage, ownership and current scope.

## Methods and exact evidence

All commands ran from `.worktrees/rrc-resource-governor` with `CARGO_BUILD_JOBS=1` and `RUST_TEST_THREADS=1` where Cargo tests/builds ran.

### First red receipts

```text
cargo xtask acceptance
... missing field `release_milestone_cursor` in initializer of `TuiSession`
... four E0063 errors in apps/agent-vesper-tui/src/main.rs
```

After the initializer repair:

```text
cargo xtask acceptance
acceptance progress: 42/43 — ui::tests::registered_release_task_replaces_ready_and_no_active_tasks
rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty ... FAILED
sanitized ANSI payload was not rendered as inert telemetry text
```

Those failures are retained as audit evidence and are not represented as passing acceptance.

### Stability and final verification receipts

```text
cargo fmt --all -- --check
git diff --check
passed

for attempt in 1 2 3 4 5; do
  cargo test -p agent-vesper-tui --test rrc_terminal_pty -- --nocapture
done
5 × rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty ... ok
```

```text
cargo clippy -p agent-vesper-tui --all-targets -- -D warnings
Finished `dev` profile [unoptimized + debuginfo]
```

```text
cargo test -p vesper-harness --lib
183 passed; 0 failed; 2 ignored

cargo test -p agent-vesper-tui --lib
268 passed; 0 failed

cargo test -p agent-vesper-tui --bin agent-vesper-tui
166 passed; 0 failed

cargo test -p agent-vesper-acp --lib
61 passed; 0 failed
```

```text
cargo xtask acceptance
acceptance verified: rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty
acceptance progress: 43/43 — rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty
Acceptance regression gate: 43 exact cases passed in 21129 ms.
Offline fixture model cost: zero; live-model effectiveness is not measured.
```

The 43-case acceptance gate includes the shared controller state/matrix/retry lifecycle cases, Host Resource Governor constrained-host cases, persisted-progress hierarchy, TUI controller routing/projection, and the Unix real-PTY terminal-ownership case. It is Linux-local deterministic evidence, not live-release evidence.

## Deviations and unresolved items

- `cargo xtask verify`, the full workspace suite, ACP process-suite coverage, platform CI, and macOS/Windows live terminal UX were not rerun. This work did not alter the PRD's existing Linux-only governor limitation or the explicitly unrun non-Linux resource backend work.
- Alex-operated live acceptance of the corrective non-publishing TUI remains unrun. The earlier live-telemetry candidate remains rejected; this report does not replace the terminal-ownership corrective report's artifact identity.
- No production release workflow or publication was exercised. The PRD's exact-commit and complete cross-platform release requirements remain separate and unfulfilled here.
- The worktree retains earlier uncommitted RRC/governor/progress work; this validation neither reset nor committed it, and no clean-candidate identity is claimed.
- A whole-file relative-link scan of the pre-existing `evidence-index.md` reports many missing older VRO-19 and `Vesper bridge/recon/` targets. The new report/index/PRD RRC links resolve; the unrelated historical-link inventory was not rewritten in this RRC UX work unit.
- The two ignored harness tests are their pre-existing explicit workspace-layout and contained-runtime probes; neither is an RRC UX pass receipt.

## Readiness effect

The current RRC UX source is now locally build-clean, deterministic under five repeated real-PTY runs, and passes the enforced 43-case acceptance gate plus scoped TUI, harness and ACP library suites. This validates the corrective terminal presentation and typed-progress paths **within the documented Linux-local, non-publishing scope**. Further code work is not indicated by the executed deterministic gates; remaining work is evidence/acceptance work only: Alex-operated live terminal acceptance, cross-platform UX/resource-backend evidence, and the separately authorized exact-commit release gate sequence.
