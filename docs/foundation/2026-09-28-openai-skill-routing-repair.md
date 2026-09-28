# OpenAI Responses and shared skill-routing repair — 2026-09-28

## Status

**IMPLEMENTED AND OFFLINE-VERIFIED; LIVE INCIDENT PAYLOAD AND PUBLIC-PROVIDER REPLAY NOT AVAILABLE.**

Repair source commit: `560b32cdf9c209283c620aaf259790c8f61e8177`
(`v0.24.3-2-g560b32c`, package version `0.24.3`). VRO-19 remained on hold and was not researched, revised, implemented, authenticated or executed.

## Objective

1. Diagnose the native OpenAI `MalformedProtocol` interruption that stopped the in-progress repair, distinguish inference from auxiliary/discovery/usage paths, add secret-safe branch diagnostics, and repair only a demonstrated adapter defect.
2. Resume and finish the shared explicit-skill invocation parser repair without weakening legitimate automatic or explicit activation.
3. Prove the ordinary multiline prompt through production TUI and ACP submission paths, preserve later-turn recovery, and produce runnable Linux candidates without replacing the installed application.

## Methods and commands

- Re-read the applicable DOX chains for root, apps, tests, memory, OpenAI adapter, documentation and foundation evidence.
- Audited every production call to the former generic `wire::invalid()` rejection in `wire.rs`, `transport.rs`, `discovery.rs`, `usage.rs` and `factory.rs`.
- Captured first-party OpenAI/Codex Responses source at commit `44fe510ce3ee61c8ef623adcbf89b901c73ddd61` into the temporary `.repair-evidence/openai-upstream/` investigation directory. The retained source identifies subscription metadata and reasoning-text event discriminants; it contains no credentials or account response.
- Added red-first sanitized regressions for the reported prompt shape and the current upstream subscription event shape.
- Restored the unsafe parser branch after the red proof and then applied the bounded parser repair; no uncertain interrupted tool call was replayed.
- Ran:

```text
cargo fmt --all -- --check
cargo test -p vesper-memory --test skill_routing
cargo test -p vesper-provider-openai
cargo test -p agent-vesper-acp --features integration-test-harness --test skill_routing_controls
cargo test -p agent-vesper-acp --features integration-test-harness --test openai_native
cargo test -p agent-vesper-acp --features integration-test-harness --test process_blockers post_output_interruption_continues_from_partial_state_and_session_recovers -- --exact
cargo test -p agent-vesper-acp --features integration-test-harness --test process_blockers cancellation_before_dispatch_observes_zero_http_requests -- --exact
cargo build -p agent-vesper-tui --features integration-test-harness
python3 apps/agent-vesper-tui/tests/skill_routing_submission_pty.py target/debug/agent-vesper-tui
cargo xtask architecture
cargo xtask verify
cargo build --release -p agent-vesper-tui -p agent-vesper-acp
```

No live provider request, credential mutation, installer, release tag, push or local-application replacement occurred.

## Diagnosis

### Reported operation

The captured error occurred during the ordinary OpenAI provider turn that was executing this repair. It was not a `/usage` lookup, authenticated model discovery or memory-extraction/auxiliary operation. Those paths have separate bounded response readers and now report distinct stages (`usage-*`, `discovery-*`, `auxiliary-*`). The ordinary inference path is:

```text
OpenAiSession::start
  -> HTTP Responses SSE
  -> transport::drive
  -> wire::Decoder::event
  -> unknown/malformed event rejection
```

### Proven rejecting branch and defect

The original raw provider event was not retained, so the exact event that triggered the historical incident cannot be reconstructed. A sanitized fixture based on current first-party subscription source nevertheless reproduced the same error tuple before repair:

```text
ProviderError { provider_id: ProviderId("openai"), provider_code: None,
http_status: None, ... category: MalformedProtocol, ...
safe_message: "OpenAI returned malformed or oversized Responses data",
diagnostics: RedactedDiagnostics { fields: ExtensionMap({}) } }
```

The red fixture failed at `wire::Decoder::event` because the decoder's catch-all branch rejected informational `response.metadata` / `codex.response.metadata`; the decoder also lacked the current visible `response.reasoning_text.delta` shape. This is a demonstrated adapter defect consistent with the incident, not proof that this exact event caused the unretained live failure.

The repair treats the two metadata events as non-executable informational events and maps `response.reasoning_text.delta` to provider-visible reasoning. It does not enlarge `MAX_EVENT`, suppress unknown-event failures, weaken tool-call validation, expose encrypted reasoning, disable tools or add automatic replay.

### Diagnostics

All production callers of the former generic malformed-protocol constructor now identify their branch with only:

- `stage`;
- an allowlisted `event_type` or `<unrecognized>`;
- rejected `field`;
- `observed_bytes`;
- applicable `bound`.

Rejected values, prompts, tool arguments, tokens, headers, credentials, encrypted reasoning and raw account/provider responses are not attached. The safe user-facing error remains unchanged.

## Implemented behavior

### Shared skill routing

- Preliminary explicit-request detection and final orchestration now use the same original masked invocation view.
- `use skill <name>`, `with skill <name>` and `use the <name> skill` accept one validated identifier token, case-insensitively, with the existing underscore-to-hyphen compatibility alias and 64-character bound.
- The natural-language marker requires singular whole-word `skill` in the same local construction.
- Parsing continues after malformed ordinary prose so a later valid directive remains reachable.
- Paths, sentence/paragraph/list spans, `skills`, `skillset` and `skillful` remain ordinary prompt data.
- Missing valid explicit names still fail closed; automatic skill ranking and `/skill` behavior remain intact.

### OpenAI transport

- Current subscription metadata and visible reasoning-text events decode without a false malformed-protocol error.
- Every malformed-protocol branch in generation, discovery, usage, auxiliary processing and fixture-route validation now emits bounded secret-safe branch metadata.
- Existing visible-output, call/result identity, cancellation and no-ambiguous-replay behavior remains enforced.

### Host paths

- The ACP process test sends the ordinary multiline prompt through a real session to loopback transport exactly once, verifies preserved prompt bytes, then completes a later turn.
- The production TUI PTY test sends the prompt as bracketed paste through terminal input/composer handling to integration-only loopback transport exactly once, verifies the exact prompt and a later turn, and uses isolated state.
- Native OpenAI ACP fixtures now accept metadata/reasoning events, preserve call identity, and complete the next user turn in API-key and subscription modes.

## Files

### Production and regression files

- `crates/vesper-memory/src/skill_orchestrator.rs`
- `crates/vesper-memory/tests/skill_routing.rs`
- `crates/vesper-provider-openai/src/{wire,transport,discovery,usage,factory}.rs`
- `crates/vesper-provider-openai/src/tests.rs`
- `apps/agent-vesper-acp/tests/{skill_routing_controls,openai_native}.rs`
- `apps/agent-vesper-tui/tests/skill_routing_submission_pty.py`

### Contracts and user documentation

- `crates/vesper-memory/AGENTS.md`
- `crates/vesper-provider-openai/AGENTS.md`
- `apps/agent-vesper-acp/AGENTS.md`
- `apps/agent-vesper-tui/tests/AGENTS.md`
- `docs/skills.md`
- `docs/openai-provider-prd.md`
- `docs/foundation/AGENTS.md`
- `docs/foundation/evidence-index.md`
- this report

## Exact evidence

### Red-to-green skill parser

Pre-repair:

```text
running 1 test
test ordinary_use_the_prose_is_not_an_explicit_skill_request ... FAILED
ordinary prose was classified as explicit: "Use the actual repository review links.\nExplain tools, permissions, skills and memory."
test result: FAILED. 0 passed; 1 failed
```

Unsafe-branch restoration repeated the same failure before the final edit. Final shared suite:

```text
running 17 tests
...
test bounded_natural_language_invocation_scans_for_a_later_directive ... ok
test ordinary_use_the_prose_is_not_an_explicit_skill_request ... ok
test bounded_explicit_diagnostics_preserve_unknown_and_punctuated_forms ... ok
test result: ok. 17 passed; 0 failed
```

### Red-to-green OpenAI decoder

Pre-repair:

```text
running 1 test
test tests::http::subscription_metadata_and_reasoning_content_events_are_not_malformed ... FAILED
called `Result::unwrap()` on an `Err` value: ProviderError { ...
category: MalformedProtocol, ... http_status: None, ...
safe_message: "OpenAI returned malformed or oversized Responses data" ... }
test result: FAILED. 0 passed; 1 failed
```

Final adapter suite:

```text
running 32 tests
...
test tests::malformed_event_diagnostics_are_bounded_and_secret_safe ... ok
test tests::tool_round_trip_preserves_call_identity_and_opaque_reasoning ... ok
test result: ok. 32 passed; 0 failed
```

### Production host paths and recovery

```text
ACP skill routing: 2 passed; 0 failed
ACP native OpenAI: 5 passed; 0 failed
post_output_interruption_continues_from_partial_state_and_session_recovers ... ok
cancellation_before_dispatch_observes_zero_http_requests ... ok
PASS: bracketed paste reached the TUI provider once, preserved prompt bytes, and accepted a later turn
```

### Repository gates

```text
architecture boundaries validated for 31 packages
cargo xtask verify: PASS
Acceptance regression gate: 23 exact cases passed in 51692 ms.
Offline fixture model cost: zero; live-model effectiveness is not measured.
```

The first `cargo xtask verify` attempt reached the later standalone ACP transcript phase after the workspace suite, where five one-second process reads timed out. Immediate standalone rerun passed 7/7, and the complete `cargo xtask verify` rerun then passed. This transient is preserved rather than omitted.

### Runnable candidates

```text
version: 0.24.3
source code commit: 560b32cdf9c209283c620aaf259790c8f61e8177
source describe: v0.24.3-2-g560b32c
TUI: target/release/agent-vesper-tui
SHA-256: 2e34fa9db81043786736b1dabb1eb835a9a70d5086f1191293edd612898cbfd1
ACP: target/release/agent-vesper-acp
SHA-256: e83e426e9c0fa0b309b020a2369781f3f464c5faa3901896da064f489593252b
```

Launch the TUI candidate from the repository root with:

```text
./target/release/agent-vesper-tui
```

The installed application was not replaced.

## Deviations

- The original live Responses event and the complete original 2,095-byte user paste were not retained. The regression uses the preserved ordinary sentence plus a labelled reconstruction and current first-party event discriminants. No claim is made that the sanitized event was byte-identical to the historical response.
- `cargo test -p vesper-memory --test skill_orchestrator_routing` was initially attempted with a nonexistent target name; the actual target is `skill_routing` and passed 17/17.
- `cargo xtask source-guard` was initially attempted, but no such subcommand exists. The supported `cargo xtask architecture` gate passed.
- One filtered cancellation command used a nonmatching test name and therefore ran zero tests; the two exact test names were then run and passed.
- The first release identity command assumed a nonexistent `target/release/agent-vesper` filename. The actual TUI binary is `agent-vesper-tui`; both corrected hashes are recorded above.

## Unresolved items

- No live OpenAI replay was performed, so service-side behavior, account entitlement and whether the repaired metadata branch was the exact historical event remain unmeasured.
- No Windows/macOS release matrix or public release gate was run. These are local Linux candidates, not release artifacts.
- The complete original paste cannot be proven byte-for-byte because it was not persisted after the failure; production-path tests prove the retained multiline reproducer exactly.
- The repository still contains unrelated pre-existing uncommitted files and documentation work. They were preserved and excluded from repair commit `560b32c`.

## Readiness effect

The shared false-positive explicit-skill parser defect and a proven native OpenAI subscription-event decoder defect are repaired at offline Linux scope. TUI and ACP production submission paths, adapter call identity, partial-output recovery, cancellation, next-turn recovery, architecture boundaries and the complete local verification contract pass. This does not assert live OpenAI readiness beyond existing gates, publish a release, close unexecuted cross-platform/live acceptance, alter VRO-19's hold, or replace Alex's installation.
