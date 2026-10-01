# OpenAI Responses decoder reinspection — 2026-09-30

## Status

**BOUNDED STRUCTURED-DIAGNOSTIC REPAIR IMPLEMENTED AND FOCUSED OFFLINE TESTS PASSED. NO LIVE PROVIDER CALL OR RELEASE ACTION.**

> **Audit correction:** the later [Responses streaming boundary audit](2026-09-30-openai-responses-streaming-boundary-audit.md) supersedes this report's original claim that decoder branch detail belonged in the user-visible safe message. The safe message remains generic; parser stage, sanitized event type, field, expected shape and byte measurements belong only in structured diagnostics.

## Objective

Reinspect the historical native OpenAI `MalformedProtocol` failure against the current repository and first-party source, verify whether the accepted subscription Responses events still decode correctly, trace a real rejection to the TUI-visible error, and change only a demonstrated remaining defect.

## Methods and commands

- Re-read the root, production-crate, OpenAI-adapter, application/TUI, documentation and foundation DOX contracts.
- Inspected the historical repair report and repair commit `560b32cdf9c209283c620aaf259790c8f61e8177`, including its pre-repair `wire::Decoder::event` catch-all.
- Inspected the retained first-party snapshot `.repair-evidence/openai-upstream/codex-rs_codex-api_src_sse_responses.rs`, pinned by `.repair-evidence/openai-upstream/codex-tree.json` to OpenAI Codex commit `44fe510ce3ee61c8ef623adcbf89b901c73ddd61`.
- Performed one read-only current-source fetch from the public `openai/codex` repository. The default-branch commit was `7219fd735bef2f9cfd0363fecdbbb212e3df5255`; `codex-rs/codex-api/src/sse/responses.rs` had SHA-256 `327127fb7e6886b0ffe73c74c753cc44b2c09e72c090600a861756334397cd2b` and still contained `response.metadata`, `codex.response.metadata`, and `response.reasoning_text.delta` handling.
- Traced `wire::invalid_at` through `transport::drive`, `AgentLoopError::ProviderTurn`, and TUI `apply_agent_event`. The adapter safe message remained generic while structured diagnostics carried the stage, event and field.
- Added a red regression requiring structured diagnostics to carry the decoder stage, sanitized event type, rejected field, expected shape and applicable limit while retaining `ErrorCategory::MalformedProtocol`, a generic safe message and canary exclusion.
- Extended only the structured protocol-rejection record. Accepted event branches, limits, tool identity validation, interruption behavior and retry policy were not changed.
- Ran only focused verification:

```text
CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 cargo test -p vesper-provider-openai tests::malformed_event_diagnostics_are_bounded_and_secret_safe -- --exact
CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 cargo test -p vesper-provider-openai --features integration-test-harness tests::http::subscription_metadata_and_reasoning_content_events_are_not_malformed -- --exact
cargo fmt -p vesper-provider-openai -- --check
git diff --check -- crates/vesper-provider-openai/src/wire.rs crates/vesper-provider-openai/src/tests.rs
```

No broad workspace suite, live OpenAI request, authentication, credential read/write, installer, candidate build, release, publication, tag, push, or VRO-19 work was performed.

## Findings and repair

### Historical root cause and current accepted shapes

Before repair commit `560b32c`, `wire::Decoder::event` recognized `response.reasoning_summary_text.delta` but not `response.reasoning_text.delta`, and its informational-event arm did not include `response.metadata` or `codex.response.metadata`. Those events therefore reached the catch-all `invalid()` branch and became the generic `MalformedProtocol` error.

The retained first-party snapshot handles `response.reasoning_text.delta` as reasoning content and treats both metadata event types as non-executable stream metadata. The current first-party source at `7219fd7` retains the same discriminants. Current Vesper source already matches those shapes:

- `response.metadata` and `codex.response.metadata` are accepted as informational events;
- `response.reasoning_text.delta` becomes `ReasoningKind::ProviderVisible`;
- ordinary output and terminal events continue normally;
- unknown events still fail closed.

The original provider event was not retained, so this establishes a demonstrated historical decoder defect consistent with the incident, not byte-for-byte proof of the exact live event.

### Remaining defect

`wire::invalid_at` already stored secret-safe structured diagnostics, but the record did not state the adapter-owned expected shape for the rejected field. The intentionally generic safe message remained:

```text
OpenAI returned malformed or oversized Responses data
```

That stable safe message is user-facing. Parser branch labels and measurements are internal diagnostics and must not be interpolated into ordinary chat or ACP response prose.

### Bounded correction

`wire::invalid_at` now adds an adapter-owned `expected` label to the structured diagnostic. Only allowlisted event discriminants are retained; all others become `<unrecognized>`. Non-event branches use `not-applicable`. Rejected values, event payloads, prompts, tool arguments, credentials, headers and raw provider responses remain excluded. `ErrorCategory::MalformedProtocol`, `Retryability::Never`, the generic safe message, the 1 MiB event bound, accepted-event arms and fail-closed unknown-event behavior are unchanged.

## Files

- `crates/vesper-provider-openai/src/wire.rs`
- `crates/vesper-provider-openai/src/tests.rs`
- `crates/vesper-provider-openai/AGENTS.md`
- `docs/openai-provider-prd.md`
- `docs/AGENTS.md`
- `docs/foundation/AGENTS.md`
- `docs/foundation/evidence-index.md`
- this report

## Exact evidence

### Red regression

Before the bounded correction:

```text
running 1 test
test tests::malformed_event_diagnostics_are_bounded_and_secret_safe ... FAILED

assertion `left == right` failed
  left: Null
 right: "known Responses event type"

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 31 filtered out
```

This proves the previous implementation lacked the required expected-shape diagnostic and user-visible branch detail.

### Green malformed-event diagnostic

```text
running 1 test
test tests::malformed_event_diagnostics_are_bounded_and_secret_safe ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 31 filtered out
```

The test also asserts the stable `MalformedProtocol` category and generic safe message; structured stage `responses-event`; unknown event `<unrecognized>`; field `type`; expected shape `known Responses event type`; limit `1048576`; absence of `DO_NOT_ECHO_PROTOCOL_SECRET` from the complete debug representation; and an allowlisted malformed `response.output_text.delta` event reporting field `delta` with expected shape `bounded text delta`.

### Green accepted subscription events

```text
running 1 test
test tests::http::subscription_metadata_and_reasoning_content_events_are_not_malformed ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 47 filtered out
```

The loopback fixture sends, in order, `response.metadata`, `codex.response.metadata`, `response.reasoning_text.delta`, `response.output_text.delta`, and `response.completed`; it asserts visible reasoning, ordinary answer text and `FinishOutcome::Stop`.

### Formatting and whitespace

```text
cargo fmt -p vesper-provider-openai -- --check
# exit 0

git diff --check -- crates/vesper-provider-openai/src/wire.rs crates/vesper-provider-openai/src/tests.rs
# exit 0
```

## Deviations

- The first attempted exact test filter omitted the Rust module path and ran zero tests (`32 filtered out`). It was not counted as evidence. The corrected exact filter then produced the retained red failure and final green pass.
- The accepted-event exact filter was first run without its `integration-test-harness` feature and therefore ran zero tests. It was not counted as evidence. The corrected feature-enabled command ran the intended test and passed.
- The first format check found only rustfmt's single-line preference for one match arm. That formatting was applied, and the final format check passed.
- The original live event body remains unavailable. No fixture or narrative claims byte-identical reproduction.
- Pre-existing unrelated RRC worktree modifications were preserved. This repair touched only the OpenAI adapter/tests and its owning documentation/evidence records.

## Unresolved items

- No live OpenAI request was made, so current service entitlement, account-specific event ordering and live transport behavior remain unmeasured.
- No broad workspace, cross-platform, host process or release gate was run. Existing earlier both-host and release evidence remains historical evidence rather than a fresh rerun for this bounded message-only repair.
- If a malformed event arrives after already-visible assistant output or a started tool fragment, existing interruption safety may surface the interruption terminal rather than replaying or exposing the rejected payload. This repair intentionally does not weaken partial-output preservation or ambiguous-tool no-replay behavior.

## Readiness effect

The accepted subscription metadata and visible reasoning event shapes were already correctly implemented and remain focused-test green. A concrete structured-diagnostic defect is repaired: genuine OpenAI protocol rejections retain a bounded, secret-safe decoder stage, sanitized event type, field, expected shape and applicable limit separately from the generic user-facing safe message while preserving `MalformedProtocol` and fail-closed unknown-event handling. The later streaming boundary audit adds host separation and framing coverage. This is focused offline Linux evidence only and does not authorize a candidate, release, publication, installation or live-provider readiness claim.
