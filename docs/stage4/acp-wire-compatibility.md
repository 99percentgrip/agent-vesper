# ACP wire compatibility

Status: COMPLETE

## Versions

- Official crate: `agent-client-protocol` 2.0.0.
- Resolved schema crate: `agent-client-protocol-schema` 1.5.0.
- ACP wire protocol: v1.

The official SDK owns JSON-RPC parsing, framing, request enums, notifications,
and stdio. `vesper-acp::compat` owns the frozen protocol-v1 response difference:
the SDK `PromptResponse` has no `userMessageId`, so the official SDK responder
is parameterized with a bounded `serde_json::Value` containing top-level
`stopReason` and `userMessageId`.

## Dispatch and framing

SDK callbacks use bounded `try_send` queues and return without awaiting provider
completion. A tracked dispatcher completes correlated responses; a separate
tracked event pump emits notifications. Terminal barriers prevent the prompt
response from overtaking earlier updates. Stdout is owned only by SDK stdio;
tracing is configured on stderr.

Malformed request handling remains SDK-owned. EOF cancels and joins the runtime.
No SDK type crosses into domain or runtime contracts.

## Evidence

Unit tests prove the exact `userMessageId` placement and truthful capability
shape. Real binary transcript tests parse every stdout line as JSON-RPC and
exercise initialization, auth, lifecycle, prompts, replay, cancellation, and
EOF shutdown.

