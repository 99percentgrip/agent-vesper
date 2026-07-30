# Stage 3 Readiness

Status: COMPLETE

## Verdict

The local shared-contract input to a GLM adapter is complete. Stage 3 may create
the concrete GLM adapter crate, but it must remain bounded to provider behavior
and the existing 21 GLM fixtures.

## Required Stage 3 inputs

1. `ProviderRequest` plus pre-dispatch capability validation.
2. Typed `ProviderCapabilities` and observable fallback decisions.
3. Small factory/catalog/session/auxiliary ports with owned cancellation.
4. `ProviderStreamEvent`, `ProviderStreamContract`, and `ProviderError`.
5. Typed continuation context with provider/harness maxima.
6. Ordered content/tool/usage/finish/error DTOs.
7. 21 source-derived GLM scenarios and the applicable synthetic vectors.
8. The local SSE spike evidence in
   `docs/foundation/rust-sse-transport-spike.md`.

## Stage 3 test matrix

- exact GLM request serialization;
- reasoning/content separation;
- fragmented and interleaved tool assembly;
- usage-only chunks and provenance;
- blank/comment/malformed lines and `[DONE]`;
- known and unknown terminal reasons;
- EOF/failure before versus after visible output;
- retryability and numeric/date `Retry-After`;
- cancellation before connect/headers and during stream;
- no post-cancel output;
- output-length continuation wording and cap;
- no plain replay after visible output;
- safe/redacted provider errors;
- capability failure before dispatch.

## Gates

Stage 3 must not:

- change shared contracts merely to mirror GLM wire JSON;
- introduce GLM conditions into `vesper-domain` or the future core;
- implement ACP, the agent loop, tools, persistence, or frontend behavior;
- use live provider credentials in default tests;
- claim non-Linux platforms validated before CI executes.

## Remaining risks

- Exact retry/cancellation timing is transport-owned and still unimplemented.
- Provider schema dialect translation can expose latent bounds or extension
  needs; any shared-contract change requires a new neutral vector and review.
- Opaque reasoning must remain excluded from generic logging/telemetry.
- The remote five-target matrix has not executed.

## Readiness classification

Local contract gates are satisfied. Remote platform validation remains pending,
so the final Stage 2 status is the CI-pending readiness variant.

