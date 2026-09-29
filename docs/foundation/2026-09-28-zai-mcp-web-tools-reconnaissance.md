# Z.ai MCP web-tools reconnaissance

**Date:** 2026-09-28
**Status:** INVESTIGATION COMPLETE; REPAIR NOT IMPLEMENTED
**Scope:** shared `web_search` and `web_reader` behavior under Z.ai, OpenAI, and xAI reasoning sessions
**Source HEAD:** `1554ec8f1ff97298b3c64228eaa6aa09afc96bb5` (`vro18.1/settings-auth`)
**Published-main baseline:** `c6b7969b0a053894939c594fba1e312a6bdc583c`

## Objective

Determine why the shared first-party `web_search` and `web_reader` tools are visible under multiple reasoning providers yet fail with an MCP HTTP error. Trace both native hosts end to end, compare current Z.ai documentation, identify what the retained evidence does and does not prove, classify defects versus hypotheses, and propose the smallest repair and regression matrix. This was reconnaissance only.

## Constraints held

- No production source, Settings, credential, tool registration, endpoint, subscription, version, installation, tag, push, or release was changed.
- No credential value, credential file content, authorization header, or remote response body was read or printed.
- No live authenticated MCP request or quota-consuming remote tool call was made.
- No `401`, `403`, or `429` is inferred from a generic error.
- Z.ai MCP tools remain distinct from Vesper's opt-in native web tools and xAI provider-hosted Web/X Search.
- The accepted OpenAI Responses and shared skill-routing repair remains untouched.
- VRO-19 remained on hold.

## Identity and source relationship

| Item | Exact evidence |
|---|---|
| Branch / HEAD | `vro18.1/settings-auth` / `1554ec8f1ff97298b3c64228eaa6aa09afc96bb5` |
| Published main | `c6b7969b0a053894939c594fba1e312a6bdc583c` |
| Package version | Installed and local TUI both report `agent-vesper-tui 0.24.3` |
| Running process | PID `2873571`, `/home/Alex/.local/share/agent-vesper/agent-vesper-tui --resume 5be98333-4d76-4b80-9062-3e5800bb6394` |
| Running/installed TUI SHA-256 | `8bee47231c26931a009c3ea95a4c7222c912ddb03d02d3b8866d9afec411152f` |
| Local TUI candidate SHA-256 | `2e34fa9db81043786736b1dabb1eb835a9a70d5086f1191293edd612898cbfd1` |
| Local ACP candidate SHA-256 | `e83e426e9c0fa0b309b020a2369781f3f464c5faa3901896da064f489593252b` |

The running installed TUI is **not byte-identical** to the local TUI candidate. Its exact source commit is therefore not established from the binary alone. It is version `0.24.3`, and its observed `auth unavailable` behavior matches the inspected source. The four relevant source files have no `origin/main...HEAD` diff:

- `crates/vesper-harness/src/lib.rs`
- `crates/vesper-mcp/src/mcp.rs`
- `apps/agent-vesper-tui/src/main.rs`
- `apps/agent-vesper-acp/src/lib.rs`

The route and preset lines originated in commit `020f406d`; conversation-owned MCP session plumbing was later touched by `8f258ba2`. The pending OpenAI/skill repair does not alter this MCP route.

## Tool-to-backend and credential-source table

| Advertised Vesper tool | Vesper input | Shared executor mapping | Built-in MCP server / endpoint | Remote tool | Credential source actually used |
|---|---|---|---|---|---|
| `web_search` | required `query: string`; wrapper caps at 2,000 characters | `query` → `{"search_query": query}` | `zai_search` → `https://api.z.ai/api/mcp/web_search_prime/mcp` | Source: `web_search_prime`; current Z.ai docs: `webSearchPrime` | Direct process environment `ZAI_API_KEY`, then `Z_AI_API_KEY`; no native credential-store lookup |
| `web_reader` | required `url: string`; HTTP(S), max 2,000 characters | `url` → `{"url": url}` | `zai_reader` → `https://api.z.ai/api/mcp/web_reader/mcp` | Source and current Z.ai docs: `webReader` | Direct process environment `ZAI_API_KEY`, then `Z_AI_API_KEY`; no native credential-store lookup |

The selected OpenAI, xAI, or Z.ai reasoning credential is not passed to either MCP server. That separation is intentional at the provider boundary: these are Z.ai services and require a Z.ai key. The defect is narrower: native Vesper can hold a valid selected/stored Z.ai credential, but the MCP transport bypasses the provider credential resolver and reads only `std::env`.

## End-to-end trace

### Shared registration

1. `HarnessToolService::definitions()` statically constructs both definitions on every shared hosted service (`crates/vesper-harness/src/lib.rs:2936-3232`, rows at `3199-3208`). They are not created by provider metadata or MCP discovery.
2. Both are `ReadOnly`, not deferred, so `ToolRegistry::definitions_for(...)` advertises them in both Code and Plan modes (`crates/vesper-agent/src/registry.rs:199-225`).
3. `build_hosted_registry(...)` combines parity-default tools, the shared hosted service, and the `mcp__` dynamic gateway (`crates/vesper-harness/src/lib.rs:249-275`).

### TUI path

1. `TuiToolService::new(...)` wraps `HarnessToolService` (`apps/agent-vesper-tui/src/main.rs:11474-11501`).
2. Its `definitions()` delegates to the shared service; only VesperLens definitions are conditional (`11522-11550`).
3. `build_agent_loop(...)` passes that same service to `build_hosted_registry(...)` regardless of the selected provider (`5655-5699`).
4. `AgentLoop` obtains `definitions_for(mode)` before creating the selected provider session, then copies those definitions into every `ProviderRequest.tools` (`crates/vesper-agent/src/agent_loop.rs:815-824`, `1519-1549`).
5. A model call to `web_search` or `web_reader` returns through the shared registry to `HarnessToolService::execute`, which delegates to `execute_extended_tui_tool` (`crates/vesper-harness/src/lib.rs:3290-3319`).
6. The wrappers select `zai_search` + `web_search_prime` or `zai_reader` + `webReader` and call `mcp_result(...)` (`452-490`).
7. `mcp_result(...)` opens the MCP registry, resolves the protected built-in, and calls the conversation-owned `McpSession` (`284-317`).

TUI provider switching does not mutate this surface: it saves a provider preference and requires restart (`apps/agent-vesper-tui/src/main.rs:2698-2704`). The rebuilt loop gets the same shared definitions. Therefore these names are neither stale definitions retained from a prior GLM loop nor provider-injected tools.

### ACP path

1. Full-harness ACP constructs one `HarnessToolService` independently of the initial provider (`apps/agent-vesper-acp/src/lib.rs:99-141`; multi-provider boot follows the same composition).
2. Each ACP session receives `hosted.fork_mcp_session()` so MCP lifecycle state is conversation-owned (`381-387`).
3. `tool_registry(...)` calls `hosted.build_default_registry()` and adds Lens/acceptance services (`390-411`).
4. On an ACP footer provider switch, `turn_configuration(...)` changes `config.provider_id` and model, but the shared tool registry is unchanged (`414-528`).
5. The loop advertises and executes through the same shared harness and MCP route as TUI.

ACP can switch providers in-session; that does not carry definitions from the old provider. Each turn rebuilds the registry from the same static shared service. Dynamic definitions discovered by `mcp_list_tools` are a separate mechanism: they are named `mcp__<server>__<tool>` and are injected only into later iterations of that turn (`crates/vesper-harness/src/lib.rs:566-633`; `crates/vesper-agent/src/agent_loop.rs:1777-1823`). They do not explain static `web_search` or `web_reader` visibility.

### MCP transport, handshake, and failure retention

For each HTTP tool call, `McpClient::call_tool` performs:

1. `initialize` JSON-RPC request with protocol version `2025-06-18` and client info.
2. `notifications/initialized`.
3. `tools/call` with the hard-coded remote tool name and mapped arguments.

Requests use POST, `Content-Type: application/json`, `Accept: application/json, text/event-stream`, `MCP-Protocol-Version`, optional `Mcp-Session-Id`, and bearer authorization. The implementation also sends informational `Mcp-Method` and, for calls, `Mcp-Name` headers (`crates/vesper-mcp/src/mcp.rs:380-414`, `476-583`).

Credential resolution occurs while building the request, before `send()`: direct `std::env::var("ZAI_API_KEY")`, then direct `std::env::var("Z_AI_API_KEY")`. Missing values become `McpError::Http("auth unavailable")` at lines `503-517` (and equivalently for notifications).

## Exact current failure stage

Three current-session `web_search` documentation queries all returned the exact receipt:

```text
tool error: tool execution failed: web_search failed: mcp http request failed: auth unavailable
```

Process inspection recorded:

```text
pid=2873571 ... ZAI_API_KEY_present=0 Z_AI_API_KEY_present=0
```

A local, non-network credential-presence check against the installed ACP returned:

```text
installed_acp_check_auth_exit=0
Z.ai credentials are configured.
```

This is consistent because `vesper-provider-glm::EnvironmentCredentialSource` first checks scoped/environment values and then the native `SecureCredentialStore`, while `vesper-mcp` checks only direct process environment. The current failure is therefore at **local MCP credential resolution before outbound HTTP**. There is no HTTP status or JSON-RPC error code for this attempt because no request was sent.

This current receipt does **not** recover the exact stage of Alex's older truncated screenshot. That event may have occurred with an environment credential present and reached the remote service. Its status/body was not retained.

## What error evidence is lost

The source proves three observability losses:

- Any non-2xx response to `initialize`, `tools/list`, or `tools/call` becomes only `mcp http request failed: non-success response`; the numeric HTTP status is discarded (`mcp.rs:518-521`).
- A non-2xx initialized notification becomes only `notification failed`; its status is discarded (`579-582`).
- Any JSON-RPC payload containing `error` becomes only `remote error`; its JSON-RPC integer code and bounded message are discarded (`537-540`).

Accordingly, the older screenshot cannot honestly be relabeled `401`, `403`, or `429`. No retained evidence distinguishes credential rejection, entitlement, balance/quota, stale tool name, or another remote error.

## Current Z.ai documentation comparison

Public Markdown pages were fetched read-only on 2026-09-28 with bounded `curl --compressed`; no account session or private endpoint was used.

| Topic | Current Z.ai documentation | Vesper source | Result |
|---|---|---|---|
| Search endpoint | `https://api.z.ai/api/mcp/web_search_prime/mcp` | Same | Match |
| Reader endpoint | `https://api.z.ai/api/mcp/web_reader/mcp` | Same | Match |
| Search remote tool | `webSearchPrime` | `web_search_prime` | **Mismatch** |
| Reader remote tool | `webReader` | `webReader` | Match |
| Authentication | `Authorization: Bearer <API key>` | `bearer_auth(token)` | Match when a token resolves |
| Credential entitlement | Coding Plan-exclusive; obtain an activated API key; docs direct users to check sufficient balance | Separate Z.ai environment key only at MCP layer | Native stored-key path missing; remote validity/entitlement untested |
| Handshake | Page says Streamable HTTP/HTTP MCP; does not publish protocol-version or initialize details | `initialize` → initialized notification → call/list using `2025-06-18` | No documented contradiction; exact server negotiation unverified |
| Search MCP input schema | MCP page does not publish `inputSchema`; REST API documents required `search_engine` and `search_query` | Wrapper sends only `search_query` | MCP schema remains unverified; cannot import REST requirements into MCP by assumption |
| Reader MCP input schema | MCP page does not publish `inputSchema`; REST API requires `url` and lists optional controls | Wrapper sends `url` | Required field aligns; exact MCP schema remains unverified |
| Usage | All Coding Plan tiers include Web Search and Reader MCP; MCP calls consume plan credits | No preflight entitlement/quota query | An expired/empty plan remains possible but unproven |

Documentation artifacts and SHA-256 receipts:

```text
e419629ea7ca38c52294048f44e86a6f07e9585efb232e016b8e1ef54ca0dcd9  search-mcp-server.md
9f628dfa7bfa836976ae390f67d0049b1e10dff9e32a2996433b0641e5a3fe48  reader-mcp-server.md
869c967ce3140207535e1818e8c8e8672bde895443e5a2317767c55914db896a  overview.md
6e27b8cd16763c85ed2e20574045c5015c6bae58dde22101755b104d358d6def  quick-start.md
```

Sources:

- <https://docs.z.ai/devpack/mcp/search-mcp-server.md>
- <https://docs.z.ai/devpack/mcp/reader-mcp-server.md>
- <https://docs.z.ai/devpack/overview.md>
- <https://docs.z.ai/devpack/quick-start.md>
- <https://docs.z.ai/api-reference/tools/web-search.md>
- <https://docs.z.ai/api-reference/tools/web-reader.md>

## Why the tools appear under OpenAI, xAI, and GLM

`web_search` and `web_reader` are Vesper-hosted, provider-neutral definitions registered by `HarnessToolService`. They are not declared by the active provider, not copied from a prior GLM request, and not xAI hosted-search features. The shared `AgentLoop` sends them to whichever reasoning adapter is active.

This matches the OpenAI PRD boundary that legacy Z.ai MCP services retain their own credentials and remain provider-independent. It also preserves the xAI PRD distinction between Vesper tools and xAI server-executed Web Search/X Search/Remote MCP. An OpenAI or xAI model may therefore choose a Z.ai MCP wrapper, but execution must use a separately valid Z.ai MCP credential and entitlement.

The only later-turn injected definitions are explicitly discovered `mcp__...` names. Provider switching does not make the two static wrappers stale; their unconditional shared registration explains their repeat visibility.

## Confirmed defects

1. **Native stored Z.ai credentials do not reach the Z.ai MCP transport.** The provider resolver reads the native secure store; MCP reads only direct environment variables. This is confirmed by source, absent process variables, successful local `--check-auth`, and pre-send `auth unavailable`.
2. **The hard-coded search remote tool name is stale against current first-party documentation.** Source calls `web_search_prime`; docs advertise `webSearchPrime`. The endpoint's snake_case path is not the remote tool name. This defect is currently masked by the earlier credential failure.
3. **HTTP and JSON-RPC diagnostics discard the discriminating code.** Numeric HTTP status and JSON-RPC code are not retained, preventing exact diagnosis of any remote failure.

## Hypotheses and missing evidence

| Hypothesis | Status | Required falsifier/evidence |
|---|---|---|
| The older search failure reached `tools/call` and failed because `web_search_prime` is unknown | Plausible, not proven | Authenticated `tools/list` receipt or sanitized JSON-RPC error showing accepted names/code |
| The older Reader failure was entitlement, expired plan, insufficient credits, or invalid key | Plausible, not proven | Retained HTTP status/JSON-RPC code or account-side entitlement receipt; generic MCP HTTP text is insufficient |
| Current handshake headers/protocol version are rejected by Z.ai | No supporting evidence | A bounded initialize/list trace preserving negotiated protocol and status |
| Search requires additional MCP arguments such as `search_engine` | Unknown | Authenticated `tools/list` `inputSchema`; the REST schema alone is not MCP evidence |
| Tool visibility is stale after provider switching | Refuted by source | Both hosts rebuild/use the same static shared definitions independent of provider; dynamic injected tools use different names |

## Smallest proposed correction

No correction was applied in this work unit. The smallest coherent repair is:

1. Add an injectable, secret-safe MCP credential resolver at the `vesper-mcp` boundary. At host composition, resolve the preset's `ZAI_API_KEY` reference using the same precedence as the GLM adapter: scoped/environment `ZAI_API_KEY`, legacy `Z_AI_API_KEY`, then Vesper's stored Z.ai credential. Do not copy an OpenAI/xAI credential, persist a second key, mutate global environment, or make `vesper-mcp` depend on a concrete provider crate.
2. Change only the search `tools/call` name from `web_search_prime` to `webSearchPrime`; keep the endpoint path unchanged.
3. Replace static HTTP error labels with a bounded secret-safe error carrying the numeric HTTP status, and preserve JSON-RPC integer `code` plus a tightly bounded sanitized message/category. Continue to suppress endpoints, headers, tokens, and unrestricted bodies.
4. Keep the two wrappers shared/provider-neutral and keep xAI native hosted search distinct. Missing Z.ai credentials should produce an actionable local availability error and must not trigger a network request.

## Required regression cases

1. **Stored credential bridge:** with both Z.ai environment variables absent and a test secure-store key present, loopback Search and Reader initialize successfully; canary never appears in output/debug.
2. **Credential precedence:** scoped/environment primary beats legacy alias, and both beat stored credential; absent credentials produce `auth unavailable` before any loopback request.
3. **Search route:** `web_search({query})` emits endpoint `/web_search_prime/mcp`, remote name exactly `webSearchPrime`, and arguments exactly `{"search_query": ...}`.
4. **Reader route:** `web_reader({url})` emits endpoint `/web_reader/mcp`, remote name exactly `webReader`, and arguments exactly `{"url": ...}`.
5. **Status retention:** loopback `401`, `403`, `429`, and `500` remain distinguishable without retaining bodies or secrets.
6. **JSON-RPC retention:** an HTTP 200 JSON-RPC error preserves integer code and bounded sanitized message/category.
7. **Handshake:** initialize, initialized notification, session header propagation, JSON response, and event-stream response remain covered.
8. **Provider matrix:** Z.ai, OpenAI, and xAI direct loops in TUI and ACP advertise the same two shared wrappers and route them through the same MCP credential port.
9. **Switching:** TUI restart switch and ACP in-session switch neither duplicate nor drop the wrappers; discovered `mcp__...` definitions remain a separate later-turn mechanism.
10. **Capability separation:** xAI hosted Web/X Search selection neither enables nor substitutes the Z.ai MCP wrappers; opt-in native Vesper web tools remain separately controlled.

## Bounded live diagnostic requiring approval

No live authenticated diagnostic was run. If Alex approves one, the minimum discovery-only probe is:

- exactly one `initialize` + `notifications/initialized` + `tools/list` sequence against each of the two documented endpoints;
- use the existing stored Z.ai credential only through an ephemeral secret-safe resolver/process scope;
- perform **zero** `tools/call` operations;
- cap each request at 15 seconds and response at 1 MiB;
- record only HTTP status, negotiated/session-header presence, JSON-RPC error code/category, tool names, and `inputSchema` hashes/field names;
- never print the credential, Authorization header, endpoint query secrets, unrestricted body, or credential-store path/content;
- stop after two endpoints with no retry.

Because Z.ai documents MCP calls as plan-credit usage and because the current client cannot consume the stored credential without code or ephemeral credential bridging, this probe requires explicit approval. After a repair, end-to-end acceptance would separately require at most one benign search call and one public-page Reader call, also explicitly approved.

## Methods and commands

Read-only/source commands included:

```text
git status --short
git diff --stat origin/main...HEAD -- <four relevant files>
git blame -L ... crates/vesper-harness/src/lib.rs
git blame -L ... crates/vesper-mcp/src/mcp.rs
sha256sum /proc/<pid>/exe target/release/agent-vesper-{tui,acp}
/proc/<pid>/environ presence-only checks for ZAI_API_KEY and Z_AI_API_KEY
agent-vesper-acp --check-auth
curl -LfsS --compressed --max-time 20 <public Z.ai Markdown URL>
cargo test -p vesper-mcp --lib --locked
```

Verification receipt:

```text
running 25 tests
test result: ok. 24 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out
```

The ignored test requires explicitly supplied local Node/Playwright and is unrelated to these HTTP presets. Existing tests prove the MCP crate remains green; they do not cover native stored-credential resolution, the documented remote Search name, or status/code retention.

## Files changed

Documentation only:

- `docs/foundation/2026-09-28-zai-mcp-web-tools-reconnaissance.md`
- `docs/foundation/evidence-index.md`
- `docs/openai-provider-prd.md`
- `docs/vro18-native-xai-provider-prd.md`
- `docs/web-oracle-extraction-prd.md`
- `docs/foundation/AGENTS.md` (nearest-owner DOX index)

No production file was edited.

## Deviations and unresolved items

- The running installed TUI differs from both local candidate binaries; exact build commit remains unidentified.
- Current runtime evidence is from the running TUI-host session. ACP was traced from source and parity composition, not reproduced against a live ACP client.
- The older screenshot's complete text/status/body was unavailable.
- Authenticated `tools/list`, remote input schemas, account entitlement, plan validity, balance, quota, and service acceptance of the handshake remain unexecuted.
- No regression test was added because this work was explicitly investigation-only.

## Readiness effect

This reconnaissance is complete and narrows the immediate failure to a local credential-source split. It also identifies a masked Search naming defect and an observability defect. Production readiness is **not improved** until those corrections and regressions are implemented and the bounded live acceptance is approved and executed. OpenAI/skill repairs remain preserved; VRO-19 remains unchanged and on hold.
