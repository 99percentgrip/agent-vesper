# Z.ai MCP web-tools repair

**Date:** 2026-09-28
**Status:** IMPLEMENTED AND OFFLINE-VERIFIED; LIVE Z.AI ACCEPTANCE NOT RUN
**Source HEAD:** `1554ec8f1ff97298b3c64228eaa6aa09afc96bb5` (`vro18.1/settings-auth`)
**Scope:** shared hosted-tool eligibility, Z.ai MCP credential resolution, Search route correction, and bounded MCP error diagnostics

## Objective

Repair the confirmed defects from `2026-09-28-zai-mcp-web-tools-reconnaissance.md` without conflating Z.ai MCP services with xAI provider-hosted search or Vesper's opt-in native web tools. The correction must:

1. expose protected Z.ai Search/Reader tools only while the Z.ai/GLM reasoning provider is active;
2. reject stale, injected, gateway, or forged calls after a provider switch;
3. resolve the existing native stored/scoped Z.ai credential through an on-demand secret-safe MCP port, retaining the legacy environment fallback;
4. call the documented Search remote tool name `webSearchPrime` while leaving the endpoint path unchanged; and
5. retain bounded HTTP/JSON-RPC diagnostics without exposing credentials, headers, URLs, or unrestricted response bodies.

No provider call, credential mutation, installation, version bump, push, tag, release, or VRO-19 work was authorized or performed.

## Implementation

### Provider-scoped tool contract

- `vesper-domain` adds `ToolProviderScope::{Any, Provider(ProviderId)}` to `ToolDefinition`, with a serde default of `Any` for compatibility.
- `ToolRegistry::definitions_for_provider` filters initial advertisement by the active provider.
- `AgentLoop` carries the active `ProviderId` in every `ToolContext`, filters initial and deferred definitions, and repeats the scope check before permission/execution.
- `ToolRegistry::execute` independently blocks direct forged calls to provider-ineligible registered definitions.
- Harness MCP discovery, list, search, direct call, and gateway routes verify the selected server's provider scope. Dynamically discovered definitions inherit the server scope.
- Protected `zai_search`, `zai_reader`, and `zai_vision` presets are scoped to provider ID `zai`; Playwright and custom MCP servers remain provider-neutral by default.
- ACP derives advertisement from the already-resolved per-turn provider configuration. The TUI regression drives an xAI turn and verifies that ordinary shared tools remain present while the three Z.ai definitions are absent.

### Credential path

- `vesper-mcp::McpCredentialResolver` is an object-safe, provider-neutral on-demand secret port.
- HTTP initialize, initialized notification, list, and call stages resolve authorization for each dispatch through that port. If it returns no value, the existing `ZAI_API_KEY` then `Z_AI_API_KEY` environment behavior remains.
- TUI composes `vesper_provider_glm::EnvironmentCredentialSource`, which already applies scoped/environment validation and native stored-key fallback.
- ACP uses `HarnessToolService::with_mcp_credential_resolver_fn`; the harness owns the MCP trait adapter so ACP does not gain a forbidden direct dependency on `vesper-mcp`.
- Forked/read-only conversation services preserve resolver authority but create fresh MCP session ownership.

### Route and diagnostics

- Search now sends remote tool name `webSearchPrime`; the endpoint remains `https://api.z.ai/api/mcp/web_search_prime/mcp` and arguments remain `{"search_query": ...}`.
- Reader remains `webReader` with `{"url": ...}`.
- `McpError::RemoteResponse` retains optional numeric HTTP status, optional JSON-RPC integer code, and at most 256 control-stripped message characters.
- The implementation never retains or displays the authorization token, headers, URL, or unrestricted body.

## Files changed by this repair

Production and tests:

- `apps/agent-vesper-acp/src/lib.rs`
- `apps/agent-vesper-tui/Cargo.toml`
- `apps/agent-vesper-tui/src/main.rs`
- `crates/vesper-domain/src/lib.rs`
- `crates/vesper-domain/src/tool.rs`
- `crates/vesper-agent/src/agent_loop.rs`
- `crates/vesper-agent/src/executor.rs`
- `crates/vesper-agent/src/registry.rs`
- `crates/vesper-agent/src/tools.rs`
- `crates/vesper-agent/tests/command_settlement.rs`
- `crates/vesper-agent/tests/provider_tool_scope.rs`
- `crates/vesper-harness/src/host_commands.rs`
- `crates/vesper-harness/src/lens_tools.rs`
- `crates/vesper-harness/src/lib.rs`
- `crates/vesper-harness/src/mcp_session_tests.rs`
- `crates/vesper-harness/src/web_service.rs`
- `crates/vesper-mcp/src/error.rs`
- `crates/vesper-mcp/src/lib.rs`
- `crates/vesper-mcp/src/mcp.rs`
- `crates/vesper-mcp/src/playwright_live_tests.rs`
- `crates/vesper-mcp/src/session.rs`
- `crates/vesper-mcp/src/session_tests.rs`
- `crates/vesper-mcp/tests/http_contract.rs`

DOX and evidence:

- `crates/vesper-domain/AGENTS.md`
- `crates/vesper-agent/AGENTS.md`
- `crates/vesper-mcp/AGENTS.md`
- `crates/vesper-harness/AGENTS.md`
- `apps/agent-vesper-acp/AGENTS.md`
- `apps/agent-vesper-tui/AGENTS.md`
- `docs/foundation/2026-09-28-zai-mcp-web-tools-repair.md`
- `docs/foundation/evidence-index.md`
- `docs/foundation/AGENTS.md`
- `docs/openai-provider-prd.md`
- `docs/vro18-native-xai-provider-prd.md`
- `docs/web-oracle-extraction-prd.md`

Pre-existing unrelated modified and untracked paths were not cleaned, reset, installed, committed, or published.

## Methods and commands

```text
cargo fmt --all -- --check
cargo test -p vesper-agent --test provider_tool_scope -- --nocapture
cargo test -p vesper-harness shared_service_advertises_all_hosted_python_tools -- --nocapture
cargo test -p agent-vesper-tui --features integration-test-harness xai_grok_session_plain_code_turn_reaches_loopback_with_full_registry -- --nocapture
cargo test -p vesper-domain -p vesper-agent
cargo test -p vesper-mcp
cargo test -p vesper-harness
cargo test -p agent-vesper-tui
cargo test -p agent-vesper-acp
cargo test -p agent-vesper-acp --test process_blockers -- --test-threads=1
cargo clippy -p vesper-domain -p vesper-agent -p vesper-mcp -p vesper-harness -p agent-vesper-acp -p agent-vesper-tui --all-targets -- -D warnings
cargo xtask architecture
```

No command contacted a live provider endpoint.

## Exact evidence

### Focused scope and host receipts

```text
running 1 test
test provider_scope_filters_advertisement_and_blocks_forged_execution ... ok
test result: ok. 1 passed; 0 failed
```

```text
running 1 test
test tests::shared_service_advertises_all_hosted_python_tools ... ok
test result: ok. 1 passed; 0 failed
```

```text
running 1 test
test tests::xai_grok_session_plain_code_turn_reaches_loopback_with_full_registry ... ok
test result: ok. 1 passed; 0 failed
```

The xAI fixture additionally asserts that `web_search`, `web_reader`, and `vision_analyze` are absent while `read_file`, `run_command`, `update_plan`, and `request_human_input` remain advertised.

### MCP transport receipts

```text
running 2 tests
test http_and_jsonrpc_diagnostics_are_bounded_without_raw_body_leakage ... ok
test adapter_resolver_authenticates_each_http_stage_and_call_name_is_exact ... ok
test result: ok. 2 passed; 0 failed
```

The loopback server verifies bearer authorization at initialize, notification, and call; exact call name `webSearchPrime`; exact mapped arguments; status/code retention; bounded sanitized messages; and absence of the secret/body canaries from rendered errors.

### Package and architecture receipts

```text
vesper-domain + vesper-agent: 427 unit tests passed; 25 agent-loop integration tests passed; provider-scope regression passed; no failures
vesper-mcp: 24 passed, 1 ignored; HTTP contract: 2 passed; no failures
vesper-harness: 124 passed, 2 ignored; all integration suites passed
agent-vesper-tui: 267 library tests + 160 binary tests + 11 capture tests passed; no failures
```

```text
cargo clippy ... -- -D warnings
Finished `dev` profile ...
```

```text
architecture boundaries validated for 31 packages
```

The architecture gate initially rejected an attempted direct `agent-vesper-acp -> vesper-mcp` edge. The composition was corrected to the harness-owned closure adapter, the direct dependency was removed, and the quoted final architecture receipt passed.

## Deviations and failed checks

The complete default ACP test command did not finish green:

```text
test result: FAILED. 9 passed; 3 failed
persistence_vectors::corrupt_unsupported_and_secret_bearing_records_fail_without_repair
persistence_vectors::fork_close_and_cross_source_collision_are_disk_invariant
persistence_vectors::listing_load_resume_and_replay_visibility_are_disk_invariant
```

Each failure reported `<scenario>: application created config state`. A serialized rerun produced the same three failures, ruling out sibling-test concurrency. The workspace already contained the pre-existing untracked `apps/agent-vesper-acp/.config/` directory that these disk-invariance vectors treat as application-created state; this work unit did not delete or alter that user-owned/pre-existing directory. The ACP library suite itself passed 60/60 before the process suite reached this environmental invariant. This remains a non-green command and is not represented as passing.

The ignored MCP test requires an explicitly supplied local Node/Playwright MCP CLI and isolated Chrome. The two ignored harness tests are explicit workspace/runtime acceptance probes. None was run.

## Unresolved items

- No live authenticated `initialize`, `tools/list`, Search call, or Reader call was performed. Actual Z.ai entitlement, account credits, current server schemas, and remote acceptance therefore remain unverified.
- The current correction intentionally removes protected Z.ai MCP wrappers from OpenAI, xAI, LM Studio, and other non-Z.ai reasoning turns. Native Vesper web tools remain separately opt-in and provider-neutral; xAI hosted search remains provider-owned and separately configured.
- `zai_vision` receives the same provider scope, but this work did not redesign its stdio subprocess environment injection. The repaired HTTP credential port directly covers Search and Reader.
- Cross-platform CI and release-profile acceptance were not run. No release readiness claim is made.
- The pre-existing ACP `.config/` state must be isolated or removed by its owner before the three persistence vectors can provide a clean full-suite receipt.

## Audit note — finishing verification

The follow-up [finishing verification](2026-09-28-zai-mcp-web-tools-finishing.md)
corrects two initial conclusions without rewriting this report's historical receipts:

- The app-local `.config/` directory was not the process-suite writer. Before/after inventories proved it invariant. The isolated child root created an xAI operation-lock file after host credential discovery escaped HOME/XDG isolation; explicit signed-out xAI test state fixed the environment and the complete ACP process suite passed.
- A bounded control-stripped upstream message was not credential redaction. The final implementation discards arbitrary upstream message text and retains only numeric codes plus fixed local categories.

## Readiness effect

The source-level defects are repaired and the relevant provider-neutral, host-composition, transport, security, and architecture regressions pass offline. The finishing report supplies the final green process receipt and runnable candidate identities. This remains **not** sufficient evidence for remote-service, entitlement, cross-platform, release, or installed-binary completion, and no local installation was changed.
