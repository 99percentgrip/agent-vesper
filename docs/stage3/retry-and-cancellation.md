# GLM Retry and Cancellation

Status: COMPLETE

`RetryPolicy` retries 429, 500, 502, 503, and 504 up to three times. It parses
numeric and HTTP-date `Retry-After`, caps waits at 60 seconds, otherwise uses
75–100% jitter over capped exponential delay. Tests inject deterministic
jitter; cancellation is selectable during dispatch, headers, body reads, and
backoff.

Pre-output transport/incomplete failures may replay. Once reasoning or content
is visible, plain replay is forbidden: the adapter may issue only a bounded
continuation carrying accumulated assistant state, and only before any tool
fragment appears. Clean EOF after one fully assembled tool call emits that call
once with its stable ID; partial/ambiguous calls terminate without replay.
Unrecovered failures emit typed `FinishOutcome::StreamInterrupted` causes
(`GenerationDeadline`, `ReadInactivity`, `RemoteEof`, or `Transport`) and the
tool-call ambiguity bit. Cancellation remains idempotent and distinct, emits
one cancelled terminal result, drops the body, and permits no later delta.

Evidence: `glm_acp/config.py` lines 415–419;
`glm_acp/glm_client.py::_retry_delay` lines 622–639,
`_do_stream_request` lines 557–601, and `cancel` lines 154–163.
Rust ownership is in `retry.rs`, `transport.rs`, and `adapter.rs`; loopback
coverage includes pre-connect, pre-header, mid-stream, retry, incomplete EOF,
partial-output continuation, complete and partial tool EOF, independent
deadline/inactivity classification, bounded repeated interruption, and
continuation cancellation paths.
