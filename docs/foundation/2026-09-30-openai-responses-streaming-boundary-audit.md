# OpenAI Responses streaming boundary audit — 2026-09-30

## Status

**ROOT-CAUSE CLASS IDENTIFIED AS A SEMANTIC UNKNOWN-EVENT REJECTION; EXACT LIVE EVENT TYPE REMAINS UNRECOVERABLE. REQUESTED OFFLINE FRAMING, BOUNDARY, HOST-SEPARATION AND NO-REPLAY REGRESSIONS PASSED. NO LIVE PROVIDER, CANDIDATE OR RELEASE ACTION.**

## Objective

Audit the native OpenAI Responses failure recorded as:

```text
category: MalformedProtocol
safe_message: OpenAI returned malformed or oversized Responses data
event_type: <unrecognized>
field: type
observed_bytes: 1048576
bound: 1048576
```

Distinguish among a valid event larger than 1 MiB, accidental SSE accumulation across events or chunks, an unsupported Responses event type, and another bounded-buffer defect. Preserve `wire::MAX_EVENT == 1_048_576`, fail-closed unknown semantic events, tool-call validation, opaque reasoning, partial-output retention and no automatic replay. Separate user-facing failure text from structured diagnostics in both hosts and add direct transport regressions for fragmentation, coalescing, CRLF, multiline data, control fields, EOF, aggregate traffic over 1 MiB, exact/over-bound events and exact-once interrupted settlement.

## Scope and methods

- Re-read the root, production-crate, `vesper-agent`, native OpenAI adapter, TUI, ACP, documentation and foundation DOX chains.
- Inspected `crates/vesper-provider-openai/src/transport.rs::drive`, including its per-line `Vec<u8>`, per-event `String`, newline parsing, CR stripping, blank-line dispatch, line/data limits, accumulator clearing and EOF behavior.
- Inspected `crates/vesper-provider-openai/src/wire.rs::Decoder::event` and every protocol-rejection diagnostic field.
- Compared the same transport and decoder code at repair commit `560b32cdf9c209283c620aaf259790c8f61e8177`; the relevant framing and `observed_bytes = value.to_string().len()` logic is unchanged.
- Traced `ProviderError` through `AgentLoopError::ProviderTurn`, TUI terminal handling/Last Run/activity, and ACP `safe_agent_loop_error`.
- Added native loopback transport fixtures with caller-selected HTTP chunk sizes and direct event collection.
- Added a diagnostic measurement-basis field so `observed_bytes` can no longer be mistaken for raw SSE size when it was calculated from canonical JSON serialization.
- Ran focused and complete affected-crate tests, both-host checks, strict Clippy and the architecture boundary gate. No provider network request, credential operation, installer, candidate build, release, publication, tag, push, installation, RRC implementation change or VRO-19 work was performed.

## Root-cause determination

### 1. One valid event larger than 1 MiB — ruled out for the recorded diagnostic branch

The line guard reports:

```text
stage=responses-sse-line field=line observed_bytes_basis=sse-line
```

The multiline event-data guard reports:

```text
stage=responses-sse-data field=data observed_bytes_basis=sse-data-accumulator
```

Both reject before JSON decoding and before `Decoder::event`. A valid oversized single-line event and a valid oversized multiline event now have direct regressions proving those distinct branches. The recorded failure instead said `stage=responses-event` (from the retained incident context), `field=type`, and `event_type=<unrecognized>`. It therefore passed the framing limits, parsed as JSON, and reached semantic decoding. The recorded tuple is not the oversized-line or oversized-data branch.

### 2. SSE framing accumulation across events/chunks — ruled out for the requested cases

`drive` keeps HTTP chunk boundaries out of protocol semantics: bytes remain in `buffer` until LF, then each completed line is interpreted. `buffer.clear()` runs after every line. `data.clear()` runs immediately after a blank-line-delimited event is parsed, before semantic decoding. The new regressions prove:

- one-byte HTTP chunks preserve UTF-8 and event boundaries;
- several complete events in one HTTP chunk remain separate;
- CRLF blank lines dispatch and reset correctly;
- multiline `data:` reconstructs only the current JSON event;
- comments plus `id:`, `event:` and `retry:` do not enter JSON data;
- an aggregate stream larger than 1 MiB succeeds when each event is bounded;
- an exact-bound multiline event succeeds and is cleared before the next event;
- an over-bound multiline event fails at `responses-sse-data`, not as an unknown semantic type.

No requested framing/accumulator defect was reproduced. These finite regressions do not prove the absence of every imaginable transport defect, but they directly cover the reported hypotheses and boundary cases.

### 3. Unsupported current Responses event type — proven as the recorded failure class; exact discriminant remains unknown

In `Decoder::event`, only an absent `type` or the catch-all unknown-type arm can produce `field=type`. At the incident revision, `<unrecognized>` specifically represents a present string discriminant not in the adapter allowlist. A recognized event with a malformed payload would retain its allowlisted event type and identify another field such as `delta` or `output_index`.

Therefore the recorded diagnostic classifies the live failure as a syntactically valid, framing-bounded JSON event whose `type` was not recognized by that decoder. The exact provider discriminant was deliberately sanitized to `<unrecognized>` and the payload was not retained, so it remains impossible to prove which event it was or whether it was one of the subsequently added metadata/reasoning event shapes. The approximately 3,157-character prompt is not evidence for this failure and is not attributed as a cause.

### 4. Another bounded-buffer defect — not demonstrated

No requested buffer-lifecycle case failed. `wire::MAX_EVENT` remains exactly `1_048_576`; no limit was raised and no fail-closed branch was removed.

The apparent equality `observed_bytes == bound` was misleading because `responses-event` calculated bytes with `serde_json::Value::to_string()`, not from the raw SSE line or event accumulator. The new exact-bound regression constructs a raw bounded event whose six `1e100` numbers canonicalize to six `1e+100` numbers. Its raw JSON is `MAX_EVENT - 6`, its complete `data: ` SSE line is exactly `MAX_EVENT`, and its canonical JSON serialization is exactly `MAX_EVENT`. It reaches the semantic unknown-type branch and reproduces:

```text
stage=responses-event
event_type=<unrecognized>
field=type
observed_bytes=1048576
observed_bytes_basis=canonical-json
bound=1048576
```

This proves that the equality itself is not evidence that a wire buffer filled or overflowed. Structured diagnostics now state the byte basis explicitly.

## Repairs

### User-facing versus structured failure separation

- `AgentLoopError::ProviderTurn` now formats `ProviderError` with safe `Display`, not full debug formatting.
- TUI chat/status and Last Run use only the provider's bounded safe message.
- The TUI retains redacted typed provider diagnostics separately in activity, bounded to 4,096 serialized bytes.
- ACP provider-turn text uses the same safe message rather than category-only text or a debug representation.
- The OpenAI safe message remains exactly:

```text
OpenAI returned malformed or oversized Responses data
```

- Parser stage, sanitized type, field, expected shape, byte count, byte basis and limit remain in `openai:protocol-rejection` structured diagnostics only.

No rejected payload, prompt, credential, tool argument, raw response or encrypted reasoning is included.

### Stream and replay safety

The transport behavior was not weakened. The exact-once regression sends visible text, starts a tool call, then supplies malformed JSON. It observes exactly one text delta, one tool start, zero tool completions/executions and one interrupted terminal with `tool_call_started=true`. There is no automatic replay and no duplicate visible output.

An EOF regression proves that a non-blank-line-delimited `data:` event is not decoded or replayed at EOF; the stream settles once as `RemoteEof`.

## Requested regression matrix

| Requested case | Regression | Result |
|---|---|---|
| SSE split across arbitrary boundaries | `sse_one_byte_http_chunks_preserve_event_boundaries_and_utf8` | Pass |
| Multiple events in one chunk | `sse_multiple_events_in_one_http_chunk_remain_separate` | Pass |
| CRLF blank-line reset | `sse_crlf_blank_lines_reset_the_data_accumulator` | Pass |
| Multiline `data:` | `sse_multiline_data_reassembles_only_the_current_json_event` | Pass |
| Comments/control fields | `sse_comments_and_control_fields_never_enter_json_data` | Pass |
| EOF with unterminated event | `sse_eof_discards_an_undelimited_event_without_decoding_or_replay` | Pass |
| Aggregate stream over 1 MiB | `sse_aggregate_over_one_mib_across_events_does_not_accumulate` | Pass |
| Exact 1 MiB event | `sse_exact_one_mib_multiline_event_is_accepted_then_cleared` | Pass |
| Real oversized event | `sse_valid_single_line_event_over_one_mib_reports_line_bound`; `sse_multiline_event_over_one_mib_reports_data_bound_not_unknown_type` | Pass |
| Exact no replay/no duplication | `malformed_after_visible_text_and_tool_start_emits_one_terminal_without_replay` | Pass |
| Recorded exact-bound semantic tuple | `sse_semantic_rejection_can_report_exact_bound_after_bounded_wire_input` | Pass |
| User/diagnostic separation | adapter malformed diagnostic test; TUI provider-failure separation test; ACP safe mapping test | Pass |

## Files

### Production behavior

- `crates/vesper-provider-openai/src/wire.rs`
- `crates/vesper-agent/src/agent_loop.rs`
- `apps/agent-vesper-tui/src/main.rs`
- `apps/agent-vesper-acp/src/lib.rs`

### Regressions

- `crates/vesper-provider-openai/src/tests.rs`

### Durable contracts and evidence

- `crates/vesper-provider-openai/AGENTS.md`
- `crates/vesper-agent/AGENTS.md`
- `apps/agent-vesper-tui/AGENTS.md`
- `apps/agent-vesper-acp/AGENTS.md`
- `docs/AGENTS.md`
- `docs/foundation/AGENTS.md`
- `docs/foundation/evidence-index.md`
- `docs/openai-provider-prd.md`
- `docs/foundation/2026-09-30-openai-responses-reinspection.md` (audit correction)
- this report

## Verification receipts

Source baseline during verification:

```text
HEAD 01d2df045a7c30253bea8e613e9840b9f62d053b
version 0.24.4
```

### Native OpenAI adapter

```text
cargo test -p vesper-provider-openai --all-features

running 59 tests
...
test result: ok. 59 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

After adding the exact-bound semantic measurement regression, its focused rerun passed:

```text
cargo test -p vesper-provider-openai --all-features \
  tests::http::sse_semantic_rejection_can_report_exact_bound_after_bounded_wire_input -- --exact

running 1 test
test tests::http::sse_semantic_rejection_can_report_exact_bound_after_bounded_wire_input ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 59 filtered out
```

The final full adapter rerun is recorded in the closeout section below.

### Shared agent loop

```text
cargo test -p vesper-agent --lib

running 428 tests
...
test result: ok. 428 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### TUI host

```text
cargo test -p agent-vesper-tui --bin agent-vesper-tui

running 167 tests
...
test tests::provider_failure_keeps_safe_text_separate_from_structured_diagnostics ... ok
...
test result: ok. 167 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### ACP host and native OpenAI process path

```text
cargo test -p agent-vesper-acp --lib

running 61 tests
...
test tests::agent_loop_failures_keep_safe_actionable_classification ... ok
...
test result: ok. 61 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

```text
cargo test -p agent-vesper-acp --features integration-test-harness --test openai_native

running 5 tests
...
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### Architecture and strict lint

```text
cargo xtask architecture
architecture boundaries validated for 31 packages
```

```text
cargo clippy -p vesper-provider-openai -p vesper-agent \
  -p agent-vesper-tui -p agent-vesper-acp \
  --all-targets --all-features -- -D warnings

Finished `dev` profile [unoptimized + debuginfo] target(s) in 16.80s
```

### Final closeout rerun

After the byte-basis diagnostic and documentation updates:

```text
cargo fmt --all -- --check
# exit 0

cargo test -p vesper-provider-openai --all-features
running 60 tests
...
test result: ok. 60 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

cargo clippy -p vesper-provider-openai --all-targets --all-features -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.06s

cargo xtask architecture
architecture boundaries validated for 31 packages

git diff --check
# exit 0

# Scoped relative-link validation for this report, its corrected predecessor,
# the OpenAI PRD evidence block and the new evidence-index entry:
changed/scoped markdown relative links validated
```

## Deviations

- The first draft of the exact-bound semantic regression made the raw JSON `MAX_EVENT - 1`; adding the six-byte `data: ` prefix correctly triggered the earlier `responses-sse-line` guard. That failed fixture was not product evidence. The corrected fixture makes the complete SSE line exactly `MAX_EVENT` and uses six exponent canonicalizations to make the later canonical JSON size exactly `MAX_EVENT`; the corrected test passes.
- The first closeout format check found rustfmt's single-line preference for one assertion. Formatting was applied and the final check passed.
- An initial repository-wide ad hoc Markdown-link scan surfaced numerous pre-existing missing historical references in the inherited evidence index. It was not treated as a failure of this work. A scoped scan of every new or corrected link passed.
- The incident payload and original unsanitized event type are unavailable. This audit proves the failure class from the recorded branch fields and current/historical code path, not the exact provider discriminant.
- Existing unrelated RRC worktree modifications were preserved. This work did not modify RRC implementation, RRC tests, release state or VRO-19.
- No live provider replay was attempted because the request explicitly prohibited release/candidate work before root cause and no retained payload exists to replay faithfully.

## Unresolved items

- The exact unknown OpenAI event discriminant remains unknown because diagnostics intentionally sanitized it and no incident payload was retained.
- A public-provider replay cannot be exact without that payload/event identity. Current service event ordering and account-specific behavior therefore remain unmeasured.
- These are Linux-local offline and loopback receipts. No cross-platform workflow, canonical full-workspace verification, MSRV matrix or release gate was run for this audit.
- The TUI activity diagnostic is redacted and bounded but is not a durable incident payload capture. Future occurrences will identify the measurement basis and expected shape, not retain raw provider data.

## Readiness effect

The reported failure is no longer plausibly attributed to prompt length, HTTP chunking, cross-event accumulation or a demonstrated 1 MiB buffer overflow. Its recorded branch proves a bounded, syntactically valid JSON event reached semantic decoding with an unsupported string `type`; only the exact type remains unknown. The requested framing and exact-once safety cases are now regression-protected, structured byte measurements state their basis, and both hosts keep safe user prose separate from typed diagnostics.

This closes the offline audit and repair scope. It does **not** authorize a candidate build, release, publication, installation, live-account claim or a claim that the exact historical provider event has been identified.
