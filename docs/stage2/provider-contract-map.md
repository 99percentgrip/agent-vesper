# Provider Contract Map

Status: COMPLETE

## Objective

Map every observed GLM transport requirement to provider-neutral contracts
without implementing GLM HTTP, SSE, authentication, retries, or continuation.

## Evidence

The frozen client separates reasoning/content, assembles tools, and continues
length-limited output in `glm_acp/glm_client.py:180-230` and
`glm_acp/glm_client.py:688-800`. It classifies `Retry-After` and retry delays in
`glm_acp/glm_client.py:624-656`; the 21 deterministic GLM fixtures under
`fixtures/provider/glm/` are the authoritative wire-behavior inputs.

## Mapping

| Observed behavior | Stage 2 contract | Classification | Stage 3 obligation |
| --- | --- | --- | --- |
| model/endpoint selection | provider-qualified model + endpoint ID | shared request intent | map GLM registry and endpoint keys |
| system/messages/media/tools | ordered `ProviderRequest` | shared request intent | exact GLM JSON serialization |
| reasoning controls | `ReasoningIntent` + capability requirement | shared request/capability | map GLM thought/effort fields |
| reasoning deltas | `ProviderStreamEvent::ReasoningDelta` | shared stream event | parse `reasoning_content` |
| content deltas | `ContentDelta` | shared stream event | parse content chunks |
| fragmented/interleaved tools | index, optional ID/name fragments, completed call | shared stream event | exact incremental assembly |
| usage/cache data | `NormalizedUsage` with provenance and delta/cumulative mode | shared usage | map GLM usage chunks |
| finish reasons | `FinishOutcome`, including raw unknown | shared terminal | exact reason translation |
| pre-output retry | `ProviderError` retryability | shared error | implement bounded retry loop |
| post-output interruption | visible-output flag + no-plain-replay | shared invariant | never replay emitted output |
| cancellation | owned cancellation signal + cancellation error/outcome | shared port/error | cancel HTTP read and emit nothing later |
| output continuation | `ContinuationContext`/strategy/bounds/reason | shared continuation | exact legacy continuation message/cap |
| GLM endpoint/model/profile fields | compatibility envelope | GLM-specific configuration | adapter validation |
| SSE framing, `[DONE]`, malformed chunks | none in shared domain | GLM-specific wire behavior | Stage 3 parser/transport |

`ProviderRequest` and pre-dispatch validation are implemented at
`crates/vesper-provider/src/request.rs:156-320`. Support algebra is implemented
at `crates/vesper-provider/src/capability.rs:5-78` and the typed capability
snapshot at `crates/vesper-provider/src/capability.rs:215-308`. Stream events and
terminal/visibility validation are at
`crates/vesper-provider/src/stream.rs:38-197`. Small factory, catalog, session,
auxiliary, cancellation, and stream ports are at
`crates/vesper-provider/src/ports.rs:14-163`.

## Contract decisions

- `Native`, `Emulated`, `Unsupported`, and `Unknown` remain distinct.
- `Require`, `Prefer`, and `AllowFallback` resolve deterministically.
- Fallback/omission decisions are observable.
- Explicit controls lacking a capability intent fail before dispatch.
- A provider response has exactly one terminal outcome or terminal error.
- Cancellation is not transport failure.
- Plain replay after visible output is forbidden.
- Opaque continuation material is bounded, debug-redacted, versioned, and
  exposed only through an explicit accessor.
- Backpressure is poll-driven; adapters must use bounded transport buffers.

## Conformance

The 21 source GLM fixtures plus 11 synthetic contract vectors are loaded by
`vesper-testkit`. Synthetic vectors fill only requirements the frozen
single-provider source cannot express: fallback observability, unknown finish,
usage modes/provenance, opaque continuation, error redaction, terminal
uniqueness, correlation, unknown extensions, invalid bounds, ACP message IDs,
and fragmented parallel identity.

## Deferred behavior

All HTTP headers/bodies, SSE bytes, retry timers, live cancellation, wire error
mapping, exact continuation wording, authentication, and model discovery remain
Stage 3. No concrete provider condition exists in the provider-neutral request
or stream code.

