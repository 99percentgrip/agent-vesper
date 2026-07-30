# Stage 4.1 process-level blocker closure

Status: COMPLETE — CI VALIDATION PENDING

## Outcome

All seven missing vectors execute through the real ACP SDK, runtime, GLM
adapter, loopback HTTP/SSE transport, and stdio process boundary. No persistence,
agent loop, tools, or other Stage 5 work was added.

| Vector | Process test | Result | Production defect found | Fix | Gate |
| --- | --- | --- | --- | --- | --- |
| Retry before output | `retry_before_visible_output_uses_two_requests_and_one_terminal` | PASS; 2 requests | No | none | CLOSED |
| Continuation | `output_limit_continuation_is_one_acp_turn_with_cumulative_usage` | PASS; 2 requests; cumulative usage 10 | No | none | CLOSED |
| Post-output interruption | `post_output_interruption_does_not_replay_and_session_recovers` | PASS; 1 request; partial preserved | No | none | CLOSED |
| Cancel before dispatch | `cancellation_before_dispatch_observes_zero_http_requests` | PASS; 0 cancelled-turn requests | Test synchronization lacked cancellation observation | non-default generic dispatch gate waits for cancellation observation before release | CLOSED |
| Cross-session concurrency | `separate_sessions_execute_provider_requests_concurrently` | PASS; maximum concurrency 2 | Test collector discarded out-of-order responses | shared collector retains unmatched responses | CLOSED |
| Same-session serialization | `prompts_in_one_session_are_serialized_and_history_reaches_second` | PASS; maximum concurrency 1 | No | none | CLOSED |
| Slow-consumer backpressure | `slow_stdout_reader_backpressures_without_dropping_visible_events` | PASS; 6,000 deltas; bounded RSS | Official SDK writer queue did not itself provide application backpressure | one-at-a-time physical-writer acceptance gate in `vesper-acp` | CLOSED |

The focused `cancellation_remains_responsive_while_stdout_is_backpressured`
test additionally proves that cancellation closes the provider connection while
ACP stdout is held and produces one cancelled terminal after draining resumes.

## Stability

- cancel before dispatch: 25/25 after the deterministic gate correction;
- cross-session concurrency: 25/25 after correcting the out-of-order collector;
- same-session serialization: 25/25;
- slow-reader ordered-delivery/RSS: 10/10;
- cancellation under pressure: 10/10.

No test-runner retries hide failures. The initial collector race is retained in
the evidence ledger as a harness defect discovered by repetition.

## Resource and safety observations

- Cancel-before-dispatch server request count: 0.
- Cross-session maximum active provider requests: 2.
- Same-session maximum active provider requests: 1.
- Slow reader: all 6,000 visible deltas retained in order.
- Linux child RSS: each repetition remained below its pre-prompt baseline plus
  24 MiB; macOS and Windows RSS proof remains CI/host pending.
- The common RAII harness kills and reaps on failure, joins stdout/stderr
  readers, joins deterministic servers, uses isolated environment roots, and
  scans both streams for the synthetic credential canary.
- Fixture corpus: unchanged at 76 scenarios and 154 indexed payloads,
  `d09edfe2169df49e0cfef9a66083a7df046651f441deb0e78bc0c855dec6db7a`.

## Production changes

- `vesper-acp` bounds outstanding `session/update` delivery at the official SDK
  stdio writer boundary.
- The composition logic is reusable with an injected provider factory.
- `agent-vesper-acp-test-driver` is guarded by the non-default
  `integration-test-harness` feature and supplies synchronization only; the
  normal release binary has no test barrier.
- `xtask acp verify`, Stage 4 coverage, architecture checks, and the five-target
  workflow include the blocker suites.

## Validation and readiness

- Workspace tests: 105/105 on Rust 1.95 and 105/105 on Rust 1.88.
- Process transcripts: 12/12; no ignored tests.
- Formatting, strict Clippy, Cargo check, doc tests, `xtask verify`, and all
  focused fixture/provider/runtime/ACP/architecture gates pass.
- Cargo Audit: no vulnerabilities across 316 locked dependencies.
- Cargo Deny: advisories, bans, licenses, and sources pass; reviewed transitive
  duplicate warnings remain for `getrandom`, `r-efi`, and `syn`.
- Four workflow YAML files parse successfully.
- Source: exact frozen HEAD, zero tracked changes, only the pre-existing
  roadmap file untracked.
- Target: `main`, no initial commit, no remote, 21 top-level untracked paths.

Linux x86-64 is locally validated. Linux ARM64, macOS Intel, macOS Apple
Silicon, and Windows x86-64 remain remote-CI pending.

READY FOR STAGE 5 — SESSION PERSISTENCE READ PATH AND REPLAY WITH CI VALIDATION PENDING
