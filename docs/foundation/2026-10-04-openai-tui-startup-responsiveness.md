# OpenAI TUI startup responsiveness

## Objective

Keep the OpenAI TUI interactive while account authentication state and model
discovery are still running. Keyboard and bracketed-paste input must not wait
for that network or credential-store work. Discovery must still apply the
account catalog, and failure must update status without freezing input.
Authentication and provider validation stay in force. Release recovery,
publication, Registry, and VRO-19 were out of scope.

## Root cause

`drive_loop` in `apps/agent-vesper-tui/src/main.rs` awaited
`ensure_provider_authenticated` before entering the interactive loop, then
awaited `OpenAiFactory::available_models` at the top of every refresh before
`event::poll`. The refresh path drew `Loading account models…` and did not
read the terminal until discovery returned or its 10-second timeout fired.

## Repair

OpenAI startup now polls credential presence and model discovery from
background tasks. The composer remains on screen with the status
`Loading OpenAI account models…`. The static catalog may render until the
account snapshot arrives. Completion installs the same account policy and
model surface as before. Failure records the adapter's safe message and
leaves the loop polling. Free-text provider prompts are withheld until both
the account check and discovery have settled. Non-OpenAI startup
authentication is unchanged. The test-only `AGENT_VESPER_OPENAI_TEST_URL`
route exists only behind `integration-test-harness` and still uses the
adapter's loopback-only factory.

## Methods

Worktree: `/home/Alex/Projects/agent-vesper/.worktrees/openai-tui-startup-input`

Base: `3e87bdcf3cd40f5e578948c7ee7f315310d2f433`

Red loop, before the event-loop change, with the stall fixture wired:

```text
CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=1 cargo test -p agent-vesper-tui \
  --features integration-test-harness --test openai_startup_responsiveness \
  stalled_openai_discovery_accepts_input_then_applies_catalog -- --test-threads=1
```

Focused proof after the repair:

```text
CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=1 cargo test -p agent-vesper-tui \
  --features integration-test-harness --test openai_startup_responsiveness \
  -- --test-threads=1
cargo fmt --all -- --check
CARGO_BUILD_JOBS=2 cargo clippy -p agent-vesper-tui --tests -- -D warnings
CARGO_BUILD_JOBS=2 cargo clippy -p agent-vesper-tui --tests \
  --features integration-test-harness -- -D warnings
git diff --check
```

## Evidence

The pre-fix PTY run drew the blocking menu and failed in 6.70s:

```text
test stalled_openai_discovery_accepts_input_then_applies_catalog ... FAILED
typed/pasted input was not visible within 1.2s while discovery was still pending
Loading account models…
```

After the repair:

```text
test stalled_openai_discovery_accepts_input_then_applies_catalog ... ok
test stalled_openai_discovery_failure_leaves_input_responsive ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.80s
```

```text
cargo fmt --all -- --check
fmt_exit=0
```

```text
cargo clippy -p agent-vesper-tui --tests -- -D warnings
clippy_exit=0
```

```text
cargo clippy -p agent-vesper-tui --tests --features integration-test-harness -- -D warnings
clippy_feature_exit=0
```

```text
git diff --check
diff_check_exit=0
```

## Deviations

The first green attempt matched rendered `Z` and `Q` cells split by cursor
controls, not the contiguous bytes `ZQ`. The regression now asserts those
rendered cells plus the contiguous paste token. The failure status wraps, so
the failure case asserts the contiguous `HTTP 500` marker.

## Unresolved

`cargo xtask verify`, MSRV, five-target CI, installer tests, and live OpenAI
calls were not run. Settings retry inside the nested Settings screen can still
await a model refresh after the user explicitly chooses Retry; startup and the
main event loop no longer do.

## Readiness effect

Local TUI input responsiveness for OpenAI startup is fixed and regression-tested.
This is not release, tag, publication, or hosted-CI evidence.
