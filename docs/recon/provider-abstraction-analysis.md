# Provider Abstraction Analysis

Status: COMPLETE

## Scope

This proposal follows the observed GLM behavior; it is not a finalized Rust trait signature. The source provider boundary is currently `GlmClient` plus GLM registries (`glm_client.py:111-804`, `config.py:415-633`) and is called by the shared loop at `agent.py:2942-2952`.

## What is genuinely provider-independent

- Conversation roles/content/tool definitions and call/result pairing.
- Turn cancellation, normalized streamed events, retry classification hooks, usage totals, context/output budgets, and finish outcomes.
- The harness tool loop, permissions, compaction policy, verification, sessions, workers, MCP, and frontends.
- A request’s intent: model, messages, system instructions, multimodal parts, tools, tool choice, sampling intent, output bound, structured-output intent, and provider extensions.

## What is GLM-specific

- Z.ai credential names/setup method and three plan endpoints (`config.py:433-453`, `:669-775`).
- Static GLM model/plan/context registry (`config.py:495-633`).
- `thinking.type`, `clear_thinking`, `reasoning_effort=high|max`, and preserved `reasoning_content` rules (`glm_client.py:128-131`, `:540-548`).
- `tool_stream=true`, OpenAI-like GLM delta details, usage/cache field names, error codes, and Coding Plan limitations (`glm_client.py:519-801`).
- Official plan-quota monitor host/response normalization (`glm_client.py:373-453`).
- Automatic continuation wording/cap and which output-bound calls disable it (`glm_client.py:169-230`).
- Coding-plan text-model screenshot fallback and model-specific vision availability (`agent.py:6628-6757`).

These belong only in `vesper-provider-glm` and a GLM auth/config contribution. No `if provider == glm` belongs in the core loop.

## Provider-neutral domain

### Identity

- `ProviderId`: stable namespaced ID (`zai`, `openai`, `anthropic`, `google`, `openai-compatible:<profile>`).
- `ModelId`: provider-qualified opaque ID plus display metadata; never a global bare string.
- `EndpointId`: selected configured endpoint/plan, not embedded in ModelId.
- Persist both stable IDs and a provider-specific snapshot so old sessions remain explainable when discovery changes.

### Request content

- `Message`: stable role and ordered content parts.
- Content parts: text, image bytes/reference with media type, audio, provider opaque block, tool call, tool result.
- System instructions are ordered separately from conversation messages because providers differ on placement and caching.
- Tool definitions carry normalized JSON Schema plus stable harness name; provider adapters own schema dialect conversion.
- Tool choice is an enum: auto, none, required, named, provider extension.
- Sampling/output controls are optional intents; unsupported explicit controls return an error unless a configured fallback permits omission.

### Stream events

Normalized ordered events should be an enum similar to:

- `ResponseStarted { provider_request_id, metadata }`
- `ReasoningDelta { stream_id, text, preservation }`
- `ContentDelta { stream_id, part }`
- `ToolCallStarted { index, call_id?, name? }`
- `ToolCallDelta { index, id?, name?, arguments_fragment }`
- `ToolCallCompleted { normalized_call }`
- `UsageUpdate { cumulative_or_delta, normalized, provider }`
- `RateLimitUpdate`
- `ResponseCompleted { finish_reason, provider_metadata }`

The adapter must guarantee one terminal event or an error carrying `visible_output_emitted`. Core—not the provider—maps these events to ACP/TUI. GLM ordering evidence is `glm_client.py:665-678`, `:744-801`.

### Usage and cache

Normalized usage has optional input, output, total, cached-input, cache-write, reasoning, tool, image/audio units, cost, and provider raw metadata. Each field records whether it is exact, estimated, or unavailable. A provider declares whether events are deltas or cumulative; the adapter normalizes before core aggregation.

### Finish and continuation

Normalized finish reasons: stop, tool-calls, output-limit, context-limit, safety, cancelled, network-interrupted-with-partial, provider-error, unknown(raw). Continuation is a provider capability/strategy object, not unconditional core logic:

- unsupported;
- replay with continuation message;
- provider cursor/token;
- native continuation.

Core asks the adapter for a continuation request only when policy and cap permit. The GLM adapter reproduces the current exact message/cap.

## Capability model

Use typed capability descriptors, not dozens of booleans or a lowest-common-denominator trait:

| Capability | Descriptor examples | Core behavior |
|---|---|---|
| Context/output | limits by model; hard/soft/unknown | compaction/budget validation |
| Reasoning | none, streamed, preserved, effort levels, opaque/encrypted | expose supported settings; store only per privacy policy |
| Vision/audio | accepted media/reference forms, limits | validate or invoke fallback |
| Tools | schema dialect, choice modes, parallelism, streamed args | adapt definitions and scheduling |
| Prompt caching | explicit/automatic; read/write metrics; stable-prefix rules | place cacheable system/context segments |
| Structured output | JSON mode/schema/grammar | expose only valid options |
| Discovery | static/dynamic/list endpoint | cache catalog with provenance/expiry |
| Authentication | API key/OAuth/CLI runtime/local none | frontend contribution and credential scope |
| Rate/quota | headers/events/monitor endpoint | normalized provider-status event |
| Continuation | strategy and cap hints | provider-specific continuation |
| CLI/runtime bridge | subprocess protocol and availability | adapter may be process-backed, still emits normalized events |

Capabilities have support level `Native`, `Emulated { caveats }`, `Unsupported { reason }`, or `Unknown`. A feature request may specify `Require`, `Prefer`, or `AllowFallback`. Required unsupported behavior errors before sending a request. Fallback use is observable in metadata/UI.

## Ports, not finalized signatures

Recommended conceptual ports:

- `ProviderFactory`: validates config/auth and creates scoped sessions.
- `ProviderSession`: owns HTTP/process pools and cancellation; starts a response stream.
- `ModelCatalog`: discovers/returns models and capability descriptors.
- `ProviderStream`: ordered normalized events with explicit terminal/error semantics.
- `AuxiliaryService`: uses the same provider port with bounded, thinking-disabled intent; not a separate client type in core.
- `ProviderConfigContribution`: typed provider-specific fields rendered by ACP/CLI/TUI.

Provider-specific configuration is an opaque versioned JSON object validated by the adapter. Core persists it but never interprets it. Provider metadata is namespaced and redacted before telemetry/export.

## Error model

`ProviderError` requires:

- stable class: auth, permission, quota/rate, invalid request, unsupported, context/output limit, safety, transient HTTP, transport, protocol/malformed stream, timeout, cancelled;
- retryability and optional retry-after;
- whether visible output was emitted;
- safe user message;
- redacted diagnostic metadata;
- provider code/status.

Provider adapters classify; core owns retry policy limits and user-visible recovery. No adapter may replay a stream after visible deltas unless the protocol supplies a deduplicating cursor.

## Authentication and process-backed providers

- API providers use scoped secret handles; raw secrets cannot implement `Debug`, serialize, or enter events.
- OpenAI-compatible local endpoints may allow no auth and custom TLS/network policy, but quota probes never inherit cloud credentials.
- Codex-compatible or Gemini CLI/runtime integration is a process-backed provider adapter with the same normalized events/capabilities. It must expose whether tools/permissions are controlled by the external runtime to prevent double-authority ambiguity.
- Anthropic preserved reasoning/signature blocks and OpenAI reasoning/response metadata remain opaque provider blocks when round-trip preservation is required; core may display normalized reasoning only when permitted.

## Initial provider mapping

| Adapter | Native path | Important non-common behavior |
|---|---|---|
| GLM | Direct Z.ai HTTP | current thinking preservation, plans/quota, GLM SSE, continuation |
| OpenAI | Direct official API and optional Codex-compatible runtime as separate transport profiles | Responses/Chat differences, reasoning/encrypted items, hosted tools |
| Anthropic | Messages API | content-block stream lifecycle, thinking/signatures, cache controls |
| Gemini | Direct API and/or supported CLI runtime adapters | parts/candidates, safety, function calls, thought support |
| OpenAI-compatible | Configured `/chat/completions`-style endpoint | capability probing/config overrides; no assumption that every extension exists |
| LM Studio | OpenAI-compatible profile | local discovery/endpoint/auth defaults; preserve generic adapter |

Do not claim these provider behaviors until each adapter phase performs official-doc reconnaissance and fixture capture.

## Fallback policy

- Missing optional sampling parameter: omit only under `AllowFallback`, record fallback.
- Unsupported reasoning display: no synthetic chain-of-thought; display capability status.
- Unsupported direct vision: offer an explicit auxiliary vision/tool fallback, never silently upload elsewhere.
- Unsupported parallel tools: core may serialize only if semantic order is safe and disclosed.
- Unsupported structured output: fail a required request; do not prompt-emulate for security-critical schemas.
- Unknown model limits: conservative configured limit and visible “estimated” status.

## Compatibility implications

Legacy sessions contain bare GLM model IDs and `api_endpoint`/`thought_level`. The compatibility reader must map them to `provider=zai`, qualified models, endpoint config, and reasoning intent without rewriting the source files. ACP config IDs may initially remain legacy-facing under the GLM adapter; a later version can expose provider-neutral dynamic options.

## Required conformance suite

Every adapter must pass:

1. deterministic request serialization golden tests;
2. arbitrary chunk-boundary SSE/protocol tests;
3. reasoning/content/tool order tests;
4. fragmented/interleaved parallel tool assembly;
5. exact usage normalization and estimation provenance;
6. cancellation before headers, mid-body, and during continuation;
7. partial-output no-replay;
8. retry/Retry-After/timeouts;
9. unsupported/fallback matrix;
10. secret/redaction and endpoint-host tests;
11. capability/model discovery cache invalidation;
12. provider metadata round trip without core interpretation.

## Completion status

The shared normalized domain, capability model, provider-specific escape hatches, fallback rules, error/cancellation semantics, process-backed integration, and GLM migration boundary are defined. Trait signatures intentionally remain open until the foundation phase pins the official SDK versions.
