# End-to-end transcripts

Status: COMPLETE

## Method

Twelve process-level suites launch the real `agent-vesper-acp` binary (or the
non-default composition-only dispatch-gate driver) with
isolated environment, synthetic credentials, loopback ephemeral HTTP, bounded
timeouts, and a raw deterministic GLM-compatible SSE server.

## Covered transcript cases

The suites cover initialization, authentication, new/list/load/resume/fork/close,
text prompt, reasoning before content, usage, history replay, exact
`userMessageId`, empty prompt, unsupported slash command, cancellation after
visible reasoning, provider tool-call surfacing without execution, missing
ephemeral load, invalid auth method, malformed input, clean EOF, stdout JSON
purity, and secret-canary absence.

The transport traverses the official ACP SDK, `vesper-acp`, `vesper-runtime`,
and production `vesper-provider-glm`; production code contains no scenario-ID
branches or fixture lookup.

## Stage 4.1 blocker vectors

| Vector | Test | Server requests | ACP order/result | Leak/resource result | Class |
| --- | --- | ---: | --- | --- | --- |
| Retry before visible output | `retry_before_visible_output_uses_two_requests_and_one_terminal` | 2 | one content, one usage state, one terminal, exact `userMessageId` | process/server joined | semantic |
| Output-limit continuation | `output_limit_continuation_is_one_acp_turn_with_cumulative_usage` | 2 | `part`, newline, `rest`; cumulative usage 10; one terminal | process/server joined | semantic plus exact continuation wording |
| Post-output interruption | `post_output_interruption_does_not_replay_and_session_recovers` | 1 for interrupted turn | partial content, refusal terminal, later prompt succeeds | no replay; process/server joined | semantic |
| Cancel before dispatch | `cancellation_before_dispatch_observes_zero_http_requests` | 0 cancelled; 1 later prompt | cancelled terminal, then successful later prompt | gate, process, and server joined | exact safety invariant |
| Cross-session concurrency | `separate_sessions_execute_provider_requests_concurrently` | 2 | B completes while A is held; identities stay isolated | maximum server concurrency 2 | semantic concurrency |
| Same-session serialization | `prompts_in_one_session_are_serialized_and_history_reaches_second` | 2 sequential | A terminal precedes B; B request contains A answer | maximum per-session concurrency 1 | semantic concurrency |
| Slow-consumer backpressure | `slow_stdout_reader_backpressures_without_dropping_visible_events` | 1 | 6,000 ordered deltas and one terminal | Linux RSS stayed within baseline + 24 MiB | safety/resource |

`cancellation_remains_responsive_while_stdout_is_backpressured` is the focused
pressure variant: TCP backpressure is observed before cancellation, the
provider connection closes while stdout remains paused, draining resumes, and
one cancelled terminal response is produced.

The cancellation, cross-session, and same-session tests passed 25/25 clean
stability repetitions. Both slow-reader variants passed 10/10 repetitions.
The shared process collector preserves out-of-order JSON-RPC responses rather
than relying on response arrival order.
