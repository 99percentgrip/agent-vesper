# Z.ai MCP provider-isolation finishing verification

**Date:** 2026-09-28
**Status:** RUNNABLE OFFLINE-VERIFIED CANDIDATES BUILT; RELEASE PAUSED
**Source HEAD:** `1554ec8f1ff97298b3c64228eaa6aa09afc96bb5` (`vro18.1/settings-auth`)
**Live Z.ai acceptance:** **NOT RUN**

## Objective

Continue the existing Z.ai MCP repair without restarting the accepted parser/OpenAI investigation: resolve the three ACP process failures, replace unsafe arbitrary upstream JSON-RPC messages with fixed diagnostics, close focused provider-isolation evidence gaps, and build runnable TUI and ACP candidates while leaving installed applications, credentials, account state, publication, and VRO-19 unchanged.

The durable product rule is: **“Z.ai Coding Plan MCP integrations are scoped to Vesper’s `zai` provider.”** This is Vesper’s chosen provider-isolation policy. It is not a claim that the vendor protocol itself forbids another client/provider arrangement. Provider ID `xai` remains xAI / Grok and never aliases `zai`.

## Implementation and findings

### ACP process-state isolation

The three failing persistence vectors were reproduced from the exact dirty source. Before and after the failing run, `apps/agent-vesper-acp/.config/` contained exactly:

```text
agent-vesper|d|40|1790597597.4321677340
agent-vesper/xai-credentials.lock|f|0|1790597597.4321677340
|d|24|1790597597.4321677340
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  apps/agent-vesper-acp/.config/agent-vesper/xai-credentials.lock
```

That external directory existed before process startup and remained byte/inventory/mtime-identical afterward. It was not the state detected by the test. The test’s isolated `XDG_CONFIG_HOME` was creating its own `agent-vesper/xai-credentials.lock`: HOME/XDG isolation did not isolate the OS credential manager, so an available host xAI credential caused eager xAI discovery and its dispatch operation lock.

The shared ACP process harness now points both OpenAI and xAI credential ports at one private synthetic vault containing explicit signed-out records. This prevents real account discovery and credential-store writes. No assertion was weakened, no directory was ignored or deleted, and the preserved app-local `.config/` was untouched.

Final affected-suite receipt:

```text
running 12 tests
...
test persistence_vectors::corrupt_unsupported_and_secret_bearing_records_fail_without_repair ... ok
test persistence_vectors::fork_close_and_cross_source_collision_are_disk_invariant ... ok
test persistence_vectors::listing_load_resume_and_replay_visibility_are_disk_invariant ... ok
...
test result: ok. 12 passed; 0 failed
APPS_CONFIG_INVARIANT=PASS
```

The complete all-feature ACP process/test command also passed; one explicit real-container swarm acceptance remained ignored:

```text
cargo test -p agent-vesper-acp --tests --all-features -- --test-threads=1
all executed tests passed; native_acp_settings_run_executes_three_workers_and_captures_tool_continuations ignored
```

### Diagnostic safety

`McpError::RemoteResponse` no longer retains arbitrary upstream `error.message` text. It retains only numeric HTTP status, numeric JSON-RPC code, and one fixed local category such as `authentication-rejected`, `authorization-or-entitlement-rejected`, `remote-rate-limited`, or `remote-jsonrpc-failure`.

The loopback regression supplied a synthetic credential canary, token-bearing URL, raw-data canary, and terminal escape. Both `Display` and `Debug` projections excluded every canary, URL, address, and control sequence while preserving HTTP `403`, JSON-RPC `-32001`, and fixed category `authorization-or-entitlement-rejected`. Since the upstream string is discarded at the MCP decode boundary, it cannot subsequently enter model-visible tool results, TUI/ACP rendering, or logs through this error value.

```text
running 3 tests
adapter_resolver_authenticates_each_http_stage_and_call_name_is_exact ... ok
http_and_jsonrpc_diagnostics_use_fixed_categories_without_upstream_leakage ... ok
missing_credentials_fail_before_any_http_request ... ok
test result: ok. 3 passed; 0 failed
```

### Selective provider isolation

Focused offline evidence now covers:

- Initial advertisement and forged direct execution: non-owner providers receive no protected schema and execution stops before the service executor; switching owner → other → owner removes and restores eligibility.
- Deferred injection: a Z.ai-scoped injected schema is discarded for `xai` and admitted for `zai`.
- Direct wrapper, explicit discovery, list, call, and `mcp__server__tool` gateway alias denial: a provider-scoped loopback server received zero connections and the synthetic resolver received zero calls under `xai`.
- Independent capability preservation: `browser_ui` and generic `mcp_search` remain advertised while Z.ai Search, Reader, and Vision do not.
- Positive Search/Reader transport: the synthetic resolver authenticated all loopback stages; Search called exact remote name `webSearchPrime` with `{"search_query":"rust"}` and Reader called `webReader` with the expected URL argument.
- Missing credentials: failed locally as `auth unavailable`, with one resolver attempt and zero HTTP connections.
- Worker identity: `run_provider_worker` clones `WorkerFactory.config`, and `AgentLoop` derives advertisement, deferred admission, and execution from that configuration’s own `provider_id`; no parent `ToolContext.provider_id` is passed into the worker. Existing worker and harness suites passed against this final source.
- Host composition: the TUI xAI loopback composition proved protected tools absent while ordinary shared tools remained; ACP’s complete all-feature suite and per-session MCP ownership test passed.

Focused receipts:

```text
provider_scope_filters_advertisement_and_blocks_forged_execution ... ok
deferred_injection_follows_the_turn_provider_and_restores_on_switch_back ... ok
non_zai_direct_discovery_and_gateway_aliases_stop_before_credentials_or_http ... ok
xai_grok_session_plain_code_turn_reaches_loopback_with_full_registry ... ok
```

The positive route was not disabled globally: both `webSearchPrime` and `webReader` completed against the loopback fixture with the synthetic Z.ai credential.

### Preserved adjacent repairs

```text
vesper-provider-openai: 32 passed; 0 failed
vesper-memory skill_routing: 17 passed; 0 failed
ACP openai_native: 5 passed; 0 failed
ACP skill_routing_controls: 2 passed; 0 failed
```

Two OpenAI test fixtures were updated to populate the new default provider-scope field; production OpenAI decoding was not changed.

## Files changed in this continuation

- `apps/agent-vesper-acp/tests/support/mod.rs`
- `crates/vesper-agent/src/agent_loop.rs`
- `crates/vesper-agent/tests/provider_tool_scope.rs`
- `crates/vesper-harness/src/lib.rs`
- `crates/vesper-mcp/src/error.rs`
- `crates/vesper-mcp/src/mcp.rs`
- `crates/vesper-mcp/tests/http_contract.rs`
- `crates/vesper-provider-openai/src/tests.rs`
- applicable `AGENTS.md` contracts
- this report and `evidence-index.md`

All earlier Z.ai MCP repair files remain part of the runnable candidate. Unrelated modified/untracked work was not reset or cleaned.

## Verification commands

```text
cargo fmt --all -- --check
git diff --check
cargo xtask architecture
cargo clippy -p vesper-domain -p vesper-agent -p vesper-mcp -p vesper-harness -p vesper-provider-openai -p agent-vesper-acp -p agent-vesper-tui --all-targets --all-features -- -D warnings
cargo test -p vesper-agent --test provider_tool_scope
cargo test -p vesper-agent deferred_injection_follows_the_turn_provider_and_restores_on_switch_back
cargo test -p vesper-mcp
cargo test -p vesper-harness
cargo test -p vesper-provider-openai
cargo test -p vesper-memory --test skill_routing
cargo test -p agent-vesper-tui --features integration-test-harness xai_grok_session_plain_code_turn_reaches_loopback_with_full_registry -- --nocapture
cargo test -p agent-vesper-acp --lib --bins
cargo test -p agent-vesper-acp --test process_blockers -- --test-threads=1
cargo test -p agent-vesper-acp --tests --all-features -- --test-threads=1
cargo build --release -p agent-vesper-tui --bin agent-vesper-tui -p agent-vesper-acp --bin agent-vesper-acp
```

Final repository gates:

```text
architecture boundaries validated for 31 packages
selected all-target/all-feature clippy: PASS with -D warnings
cargo fmt --all -- --check: PASS
git diff --check: PASS
```

## Runnable candidates

```text
TUI launch command:
/home/Alex/Projects/agent-vesper-candidates/zai-mcp-provider-isolation-2026-09-28/agent-vesper-tui

TUI version: agent-vesper-tui 0.24.3
TUI SHA-256: 0545541a9f36783cfc3695e35bd68ccef2f5403e8a3171099141f2a415e862f4

ACP path:
/home/Alex/Projects/agent-vesper-candidates/zai-mcp-provider-isolation-2026-09-28/agent-vesper-acp
ACP version: agent-vesper-acp 0.24.3
ACP SHA-256: 02012db8b90d2c86d1ceedb8aa7a242a671d708fab8f8af7b220aef5d3abaf89
```

Both are preserved outside `target/` and other disposable test directories. The installed application was not replaced.

Source identity:

```text
commit: 1554ec8f1ff97298b3c64228eaa6aa09afc96bb5
branch: vro18.1/settings-auth
code diff SHA-256: 4f8c5c381e2775676f4ef896af834bddd2a870f51da2d8fc4ee46584893c2114
tracked working diff SHA-256: a539e76728b29bdfc098a0a8e369b3f1e77a7545f73291dd6b0f62920061309b
status inventory SHA-256: 90525721cf47e3aa91879642d5a98329bfca35b92d1204d771a9461e59c15c75
```

The code-diff digest is computed from `git diff --binary HEAD -- apps crates Cargo.toml Cargo.lock` plus the complete bytes of the two untracked focused test files (`crates/vesper-agent/tests/provider_tool_scope.rs` and `crates/vesper-mcp/tests/http_contract.rs`). The tracked/status digests include unrelated pre-existing workspace changes and are inventory receipts, not a claim that every dirty path belongs to this repair.

## Fresh pre-v0.24.4 local test candidate (2026-09-29)

### Objective

Build new release-profile TUI and ACP binaries from the complete current repaired working tree, without changing implementation, versioning, credentials, installed applications, or release state.

### Methods and commands

```text
rm -f /home/Alex/Projects/agent-vesper-candidates/pre-v0.24.4-final-test/{agent-vesper-tui,agent-vesper-acp}
cargo clean --release -p agent-vesper-tui -p agent-vesper-acp
cargo build --release -p agent-vesper-tui --bin agent-vesper-tui -p agent-vesper-acp --bin agent-vesper-acp
install -m 0755 target/release/agent-vesper-{tui,acp} /home/Alex/Projects/agent-vesper-candidates/pre-v0.24.4-final-test/
sha256sum /home/Alex/Projects/agent-vesper-candidates/pre-v0.24.4-final-test/agent-vesper-{tui,acp}
agent-vesper-tui --version
agent-vesper-acp --version
```

Package-specific release artifacts were removed before compilation, so the candidate files were not copied from the older candidate directory. Cargo recompiled and relinked both application packages from the dirty working tree:

```text
Removed 47 files, 138.4MiB total
Compiling agent-vesper-acp v0.24.3 (/home/Alex/Projects/agent-vesper/apps/agent-vesper-acp)
Compiling agent-vesper-tui v0.24.3 (/home/Alex/Projects/agent-vesper/apps/agent-vesper-tui)
Finished `release` profile [optimized] target(s) in 50.20s
```

### Exact evidence

```text
fresh build: 2026-09-29T00:43:26Z through 2026-09-29T00:44:16Z
TUI: /home/Alex/Projects/agent-vesper-candidates/pre-v0.24.4-final-test/agent-vesper-tui
TUI size: 23569712 bytes
TUI SHA-256: 0545541a9f36783cfc3695e35bd68ccef2f5403e8a3171099141f2a415e862f4
TUI reported version: agent-vesper-tui 0.24.3
ACP: /home/Alex/Projects/agent-vesper-candidates/pre-v0.24.4-final-test/agent-vesper-acp
ACP size: 23310384 bytes
ACP SHA-256: 02012db8b90d2c86d1ceedb8aa7a242a671d708fab8f8af7b220aef5d3abaf89
ACP reported version: agent-vesper-acp 0.24.3
source commit: 1554ec8f1ff97298b3c64228eaa6aa09afc96bb5
tracked diff SHA-256: a539e76728b29bdfc098a0a8e369b3f1e77a7545f73291dd6b0f62920061309b
status inventory SHA-256: 90525721cf47e3aa91879642d5a98329bfca35b92d1204d771a9461e59c15c75
```

### Files changed, deviations, unresolved items, and readiness effect

No implementation, version, credential, installation, or release file was changed. This execution updated only this already-linked evidence report. No compilation repair was required. No test prompt or live Z.ai request was executed. The resulting binaries are fresh local Linux test candidates only; all previously recorded live and cross-platform gaps remain open, and release remains paused.

## Deviations and unresolved items

- Live Z.ai discovery, authentication, inference, Search, Reader, Vision, and entitlement acceptance: **NOT RUN**. Alex has no active GLM Coding Plan; no account-access request, renewal, credit purchase, credential mutation, or environment change was performed.
- Vision remains provider-scoped and preserved, but no Vision transport redesign was attempted.
- No cross-platform CI, live browser MCP acceptance, or real-container swarm acceptance was run. The local candidates are Linux x86-64 ELF binaries.
- No version bump, commit, push, tag, publication, release, or installation occurred. Release remains paused for review. VRO-19 remains on hold.

## Readiness effect

The prior ACP environmental failures are resolved through credential isolation, not assertion weakening. The final offline source passes the affected process suite, focused provider-isolation and diagnostic-safety checks, adjacent OpenAI/skill regressions, architecture, formatting, and strict Clippy. Runnable TUI and ACP candidates are available for review. This does not establish current Z.ai Coding Plan entitlement or live remote compatibility.
