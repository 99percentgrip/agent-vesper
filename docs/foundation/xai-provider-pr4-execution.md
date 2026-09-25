# VRO-18 PR-4 tools, continuation, usage and citations execution

Status: **PASS at offline fixture scope — not registered or advertised**  
Date: 2026-09-25

## Objective

Close the adapter-owned PR-4 protocol work: preserve shared Vesper function
identity, tool-result continuation, encrypted reasoning, explicit Responses
continuation, stable prompt-cache routing, normalized usage and structured
citations without adding an xAI executor or host branch.

Requirements: [`../vro18-native-xai-provider-prd.md`](../vro18-native-xai-provider-prd.md).  
Predecessor: [`xai-provider-pr3-execution.md`](xai-provider-pr3-execution.md).

## Implementation

- All nine core Vesper tools retain shared registry order and identities. xAI
  wire names and call IDs round-trip back to the existing `ToolId`/`ToolCallId`;
  tool results serialize as `function_call_output` for the shared AgentLoop.
- Native Responses continuation accepts only a version-1 `provider.xai`
  envelope carrying one bounded `xai:previous-response-id`. The caller must
  also explicitly select `xai:store=true`; continuation never silently changes
  remote retention.
- `prompt_cache_key` accepts a stable, bounded, non-secret routing identifier
  through the same adapter-owned envelope. Unknown keys, namespaces, versions,
  whitespace and control-shaped values fail before dispatch.
- Encrypted reasoning remains byte-for-byte provider-owned opaque content and
  is never rendered as chain-of-thought.
- Cached-input and reasoning-token usage remain normalized through the shared
  usage event. The regression requires exactly one usage event followed by one
  terminal event.
- Streaming URL citation annotations are validated, bounded and retained as
  provider-owned `citation` content. They survive AgentLoop/session persistence,
  are not converted into fake Vesper web-tool events, and are not replayed to
  xAI as input. Generic host rendering remains PR-7.

Current official xAI documentation was re-read for Responses chaining,
`prompt_cache_key`, cached-token usage and citation annotations on 2026-09-25.

## Red-to-green evidence

Two regression anchors were added before the adapter change:

```text
native_continuation_and_prompt_cache_are_explicit_and_bounded
citations_are_preserved_as_bounded_provider_owned_content
```

The first failed because any continuation/provider extension returned
`UnsupportedCapability`; the second failed because the documented annotation
event produced zero events. After the repair, both pass. Existing regressions
also cover the full shared-tool surface, call-ID/argument assembly, tool-result
serialization, incomplete-call fail-closed behavior, opaque reasoning and
usage.

## Verification

```text
cargo fmt --all -- --check
cargo test -p vesper-provider-xai --all-features
cargo clippy -p vesper-provider-xai --all-targets --all-features -- -D warnings
git diff --check
```

Result: xAI **29 passed, 0 failed**; strict Clippy, formatting and whitespace
PASS. No network/provider call or credential write occurred.

## Deviations and unresolved items

`previous_response_id` is opt-in because it uses provider-side stored state;
client-managed encrypted-reasoning history remains the default with
`store=false`. Host wiring must supply a stable Vesper conversation routing key
in PR-7. Provider-hosted tools are PR-5; native compaction/WebSocket are PR-6;
host rendering/composition is PR-7. Live acceptance, five-target CI and release
remain PR-8.

## Readiness effect

PR-4 is closed at offline fixture scope. The xAI adapter remains unregistered
and unadvertised. PR-5 may add provider-hosted tools through a generic
provider-owned capability without changing shared local-tool execution.
