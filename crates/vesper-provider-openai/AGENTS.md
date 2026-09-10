# Native OpenAI adapter

## Purpose

Own native OpenAI authentication and Responses transport implementation.
Both hosts register one provider with API-key and subscription authentication.

## Ownership

- `src/auth.rs` owns bounded device authorization and token refresh.
- `src/auth_tests.rs` owns offline loopback authentication protocol evidence.
- `src/credentials.rs` owns Vesper-only credential records, selected billing
  mode, local logout, bounded refresh, and cross-process RAII file locking.
- `src/catalog.rs` owns verified model/effort/capability metadata and the
  conservative shared 272K context budget.
- `src/policy.rs` owns per-model/auth-mode reasoning choices and compatible
  model-switch cascades. API-only `none` is not offered in subscription mode.
  `ultra` is a Codex host delegation feature, not a literal Responses effort.
- `src/usage.rs` owns native subscription quota normalization. The session
  queries the fixed passive `wham/usage` endpoint with bounded cancellation,
  size and time limits, preserving primary/secondary/additional windows.
  It never consumes reset credits or queries subscription limits with API keys.
- `src/wire.rs` and `src/transport.rs` own Responses serialization, ordered
  bounded SSE, opaque reasoning, call/result identity, and interruption safety.
- `src/http_error.rs` owns bounded HTTP rejection diagnostics shared by both
  authentication modes and hosts: at most 16 KiB and two seconds of body reading,
  cancellable, with exact code/parameter allowlists. Preserve HTTP status and
  authentication/rate classifications; never echo provider prose or arbitrary
  identifiers, or grant retries from error-body claims. Unknown, malformed,
  oversized and stalled bodies retain the safe status-based fallback.
  `src/http_error_tests.rs` exercises native loopback transport in both modes;
  `src/http_error_bounds_tests.rs` verifies stalled-body and cancellation bounds.
  `src/http_error_parameter_tests.rs` proves safe parameter diagnostics survive
  null/unknown error codes in both modes without exposing provider prose or IDs.
- `src/lens_wire_tests.rs` verifies native interview function-call decoding and
  next-request serialization in both authentication modes, preserving call identity,
  schema, action, Unicode notes and all selected answers. It is adapter-boundary
  evidence, not browser submission, host registration or end-to-end Lens acceptance.
- `src/factory.rs` owns the neutral factory, credential/control ports, and
  bounded structured memory extraction used by both host cognition adapters.

## Local Contracts

- Never install, bundle, launch, or read credentials from Codex.
- API-key and subscription modes replace the selected credential record; never fall
  back to API billing when subscription authentication fails.
- Provider preference changes never mutate this credential record. Returning
  to OpenAI reuses the valid selected mode until explicit logout or replacement.
- Depend only on auth/domain/provider/config/security foundations.
- Authentication uses fixed TLS origins in production. Loopback endpoints
  require an explicit test-only constructor. Redirects are disabled.
- Secrets never enter Debug, errors, events, or normal serialization. Token
  exposure is explicit at authenticated transport or secure-storage boundaries.
- Cancellation and bounded response sizes apply to login and refresh as well
  as generation. No live accounts or user storage in verification.
- Subscription protocol evidence is pinned upstream source, not a claim that
  OpenAI publishes a stable third-party subscription API or grants entitlement.
- Production endpoints are fixed. The non-default integration-test feature
  permits only loopback Responses endpoints with synthetic credentials.
- Subscription inference uses the pinned upstream protocol. Its visible-byte
  output guard is not a guarantee about hidden reasoning or billed tokens;
  account entitlement and device-login policy remain service-controlled.

## Work Guidance

- Keep host logic out of the adapter and wire both hosts in the same change.
- Maintain evidence and deployment limitations in `docs/openai-provider-prd.md`.

## Verification

- Run `cargo test -p vesper-provider-openai --all-features`.
- Run ACP `openai_native` process tests and TUI native OpenAI wiring tests.
- Run `cargo xtask architecture`.

## Child DOX Index

No children.
