# VRO-18 PR-6 native compaction and WebSocket execution

Status: **PASS at offline transport and shared-policy scope — host controls remain PR-7**  
Date: 2026-09-25

## Objective

Add xAI Responses native compaction and WebSocket transport without replacing
Vesper's provider-neutral context governance, weakening cancellation, or
replaying an ambiguous turn.

Requirements: [`../vro18-native-xai-provider-prd.md`](../vro18-native-xai-provider-prd.md).  
Predecessor: [`xai-provider-pr5-execution.md`](xai-provider-pr5-execution.md).

## Implementation

The provider boundary now has an optional `NativeCompactionPort`. The shared
AgentLoop owns an explicit `NativeCompactionPolicy`, defaulting to `Disabled`.
When a host chooses `PreferProvider`, unfocused compaction sends only the
replaceable prefix to the active provider and atomically commits the returned
opaque item with the untouched recent suffix. Provider mismatch or invalid
state fails closed. Cancellation is surfaced; other pre-commit native failures
fall back to Vesper's existing auxiliary/main/deterministic semantic path.
Focused manual compaction remains inspectable. Opaque summaries record
`quality_measured=false` instead of fabricating semantic coverage.

The xAI implementation calls the fixed Global API-key endpoint
`POST /v1/responses/compact`, bounds the response to 2 MiB and 60 seconds,
normalizes usage, validates the single `compaction` item, and preserves its
encrypted JSON unchanged. Grok-session and US-regional configurations fail
closed.

WebSocket mode is an explicit adapter transport setting for Global API-key
mode. It uses `wss://api.x.ai/v1/responses`, the same request and event decoder
as HTTP/SSE, one serialized turn per owned connection, bounded inactivity and
turn deadlines, and cancellation that closes the connection. A handshake
failure may fall back to HTTP before dispatch. Send failure, midstream failure,
visible-output interruption and cancellation never replay the request.

## Red-to-green evidence

The shared policy test first failed because the AgentLoop had no native
compaction selection or atomic opaque-prefix commit. It now proves one native
call, a four-message replaceable prefix, no auxiliary summarization turn, and
opaque history replacement. The compaction unit test proves the recent suffix
is byte-for-byte retained and semantic quality is explicitly unmeasured.

The xAI loopback suite covers opaque compaction round-trip, zero-retention
WebSocket continuation, common event decoding, cancellation cleanup,
visible-output disconnect without replay, and pre-dispatch handshake fallback.

## Verification

```text
CARGO_TARGET_DIR=/tmp/agent-vesper-vro18-target \
  cargo test -p vesper-agent --all-features
CARGO_TARGET_DIR=/tmp/agent-vesper-vro18-target \
  cargo test -p vesper-provider-xai --all-features
```

Results: `vesper-agent` 426 unit + 8 policy + 24 AgentLoop + 9 command
settlement + 14 executor + 5 firewall tests passed (live LM Studio and soak
tests remain explicitly ignored); xAI 39/39 passed. The worktree-local
duplicate target accidentally consumed the remaining `/tmp` quota during one
run; only `/tmp/agent-vesper-vro18/target` was removed with `cargo clean`
(3.3 GiB), and verification resumed against the established isolated target.
No source, installed binary, model or user state was removed.

## Deviations and unresolved items

Native compaction remains default-off until PR-7 supplies host persistence and
controls. WebSocket is not used for Grok-session or US-regional mode because
current evidence does not establish those paths. No live xAI request occurred.
TUI/ACP composition, cross-system inheritance, live acceptance, five-target
CI and release remain PR-7/PR-8 work.

## Readiness effect

PR-6 is closed at offline transport and shared-policy scope. PR-7 and PR-8
remain open; xAI is still unregistered and unadvertised in production hosts.
