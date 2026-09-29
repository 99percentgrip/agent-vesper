# User-initiated cancellation presentation repair

**Date:** 2026-09-29
**Status:** IMPLEMENTED AND LOCALLY VERIFIED; RELEASE PAUSED
**Source HEAD:** `1554ec8f1ff97298b3c64228eaa6aa09afc96bb5` (`vro18.1/settings-auth`)
**Live provider requests:** NOT RUN

## Objective

Repair the live-confirmed presentation defect where a user-cancelled OpenAI turn carried the structured `Cancellation` category but the TUI rendered the full `ProviderError` through the generic agent-failure path. Preserve the accepted skill-routing and Z.ai repairs, provider cancellation behavior, partial output, completed tool actions, no-replay safety, and protocol-appropriate ACP cancellation.

## Implementation

- Added a first-class TUI `AgentEvent::Cancelled` presentation state that retains the original structured terminal event internally.
- Promotion requires both positive signals:
  1. the active host-owned `RuntimeCancellation` token is cancelled; and
  2. the runtime terminal is cancellation-classified (`Interrupted(Cancelled)`, `ProviderTurn` with `ErrorCategory::Cancellation`, `Incomplete(Cancelled)`, or an acceptance report carrying its explicit cancellation gap).
- Normal TUI conversation now emits `Turn cancelled by user.`. If the interrupted outcome contains completed tool results it emits `Turn cancelled by user. Completed actions were not rolled back.`
- LAST RUN now begins with `Cancelled`, includes the completed-action count, and states that rollback was not performed.
- Partial assistant content, citations, plan state, completed tool telemetry, and returned provider-working history remain intact. The bounded structured cancellation diagnostic is retained in detailed activity/telemetry and debug logging, not printed as a normal `agent error:` chat line.
- Timeouts, authentication errors, provider-side cancellation without the host token, malformed responses, and other failures retain the failure path.
- ACP now also requires token-plus-terminal corroboration before returning its native `Cancelled` stop reason. A coincident timeout/failure remains an error. Cancelled interrupted history is retained internally for a later turn; already-streamed partial content/tool updates are not duplicated.
- Provider adapters and their cancellation behavior were not changed. No replay, rollback, credential, persistence-format, version, release, or VRO-19 behavior was added.

## Regression-first evidence

The first TUI regression was run before the implementation and failed on the old presentation:

```text
cargo test -p agent-vesper-tui --bin agent-vesper-tui user_cancellation_before_visible_output_is_benign_and_distinct -- --nocapture
running 1 test
test tests::user_cancellation_before_visible_output_is_benign_and_distinct ... FAILED
left: None
right: Some("Turn cancelled by user.")
test result: FAILED. 0 passed; 1 failed
```

After the repair, focused coverage passed for cancellation before output, structured provider cancellation, partial output, completed actions/no-rollback wording, genuine failures, timeout distinction, next-turn recovery, and ACP classification/history:

```text
cargo test -p agent-vesper-tui --bin agent-vesper-tui user_cancellation -- --nocapture
running 5 tests
...
test result: ok. 5 passed; 0 failed

cargo test -p agent-vesper-tui --bin agent-vesper-tui provider_failure_and_timeout_never_become_user_cancellation -- --nocapture
running 1 test
test result: ok. 1 passed; 0 failed

cargo test -p agent-vesper-acp --lib acp_cancellation -- --nocapture
running 1 test
test tests::acp_cancellation_requires_both_user_token_and_cancelled_terminal ... ok

test result: ok. 1 passed; 0 failed

cargo test -p agent-vesper-acp --lib acp_cancelled_interruption_retains_partial_history_for_the_next_turn -- --nocapture
running 1 test
test tests::acp_cancelled_interruption_retains_partial_history_for_the_next_turn ... ok

test result: ok. 1 passed; 0 failed
```

## Broader verification

```text
cargo test -p agent-vesper-tui --bin agent-vesper-tui
running 166 tests
test result: ok. 166 passed; 0 failed

cargo test -p agent-vesper-acp --lib --all-features
running 62 tests
test result: ok. 62 passed; 0 failed

cargo test -p agent-vesper-acp --test process_blockers -- --test-threads=1
running 12 tests
...
test cancellation_before_dispatch_observes_zero_http_requests ... ok
test cancellation_remains_responsive_while_stdout_is_backpressured ... ok
test post_output_interruption_continues_from_partial_state_and_session_recovers ... ok
test result: ok. 12 passed; 0 failed

cargo clippy -p agent-vesper-tui -p agent-vesper-acp --all-targets --all-features -- -D warnings
Finished `dev` profile ...

cargo xtask architecture
architecture boundaries validated for 31 packages

cargo fmt --all -- --check
git diff --check
PASS
```

## Files changed

- `apps/agent-vesper-tui/src/main.rs`
- `apps/agent-vesper-acp/src/lib.rs`
- `AGENTS.md`
- `apps/agent-vesper-tui/AGENTS.md`
- `apps/agent-vesper-acp/AGENTS.md`
- `docs/output-visual-upgrade-prd.md`
- `docs/foundation/AGENTS.md`
- `docs/foundation/evidence-index.md`
- this report

All pre-existing tracked and intended uncommitted repairs remain present. No unrelated work was reset or cleaned.

## Fresh local TUI candidate

Package-specific release artifacts were removed before compilation, then the TUI package was rebuilt and relinked from the repaired dirty working tree:

```text
Removed 15 files, 49.9MiB total
BUILD_STARTED_UTC=2026-09-29T01:09:08Z
Compiling agent-vesper-tui v0.24.3 (/home/Alex/Projects/agent-vesper/apps/agent-vesper-tui)
Finished `release` profile [optimized] target(s) in 1m 05s
BUILD_FINISHED_UTC=2026-09-29T01:10:14Z
```

```text
path: /home/Alex/Projects/agent-vesper-candidates/user-cancellation-presentation-2026-09-29/agent-vesper-tui
size: 23432624 bytes
reported version: agent-vesper-tui 0.24.3
SHA-256: e3728d3b343e1cd170c3e19a3570c2ff8ee7fd7ed68c5dd93b2e6363e57b89ce
source commit: 1554ec8f1ff97298b3c64228eaa6aa09afc96bb5
final tracked diff SHA-256: 1eedb5e6cfdb27afefd9b1d2c7c0b80d06814bbd70c6260945dec99fb7512b28
final status inventory SHA-256: cc7572d609d7e75fe1517f3f9a83de20c2d314933c531dfc8651369cac910fa9
```

The candidate is outside `target/` and was not installed.

## Deviations and unresolved items

- No live OpenAI or Z.ai request was run. The defect was reproduced from Alex's live report and verified offline through host-level structured outcomes.
- No manual terminal interaction replay was performed; the pure TUI event/presentation path and complete TUI binary tests are the local acceptance evidence.
- No cross-platform CI or installed-candidate acceptance was run.
- No version bump, commit, push, tag, publication, release, installation, credential mutation, or VRO-19 work occurred.

## Readiness effect

The release-blocking cancellation presentation defect is repaired in the current working tree and a fresh local Linux TUI candidate is available for Alex's test. Local focused, host-wide, ACP process, strict Clippy, formatting, and architecture checks pass. Release remains paused pending user acceptance and the project's normal release gates.
