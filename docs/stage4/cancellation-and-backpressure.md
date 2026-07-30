# Cancellation and backpressure

Status: COMPLETE

## Cancellation

`tokio_util::CancellationToken` represents runtime, session, turn, and provider
descendants. Cancellation is idempotent and select-biased at the active turn.
It stops provider polling, preserves already emitted output, emits one cancelled
terminal event, and does not close the session. Close and shutdown cancel and
join actors.

The process transcript cancellation test releases reasoning, sends ACP cancel,
then proves the delayed content is absent and the response is `cancelled`.
The provider adapter retains its Stage 3 no-post-cancel/no-retry guarantees.
Stage 4.1 additionally proves deterministic cancellation before dispatch with
zero HTTP requests and provider-connection closure while stdout is held under
backpressure.

## Backpressure

ACP request callbacks use a 32-item queue, session actors use 16-item queues,
and the runtime event path uses 64 items. Sends await capacity so reasoning and
content cannot be silently discarded. Cancellation remains available directly
to the actor while a prompt child is active rather than waiting behind prompt
completion.

The official SDK's internal outgoing queue is contained by a writer-acceptance
gate: after sending one `session/update`, the event pump waits until the
physical stdio writer accepts it before polling another runtime event. A real
slow-reader process test retains 6,000 ordered deltas and terminates cleanly.
Across 10 Linux repetitions child RSS stayed below its pre-prompt baseline plus
24 MiB. Portable behavioral tests are prepared for all target families; non-
Linux host memory evidence remains CI pending.
