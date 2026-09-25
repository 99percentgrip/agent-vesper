# VRO-18 native xAI provider reconnaissance

Status: **PR-0 PASS — implementation not started**
Workspace baseline: `c64e78f46065f1fbaae899ab9914f3d9b3f7023d`
Documentation snapshot: 2026-09-25
First-party source pin: `xai-org/grok-build@f0e3be1100ef5252488e3be8bb0e91cf68d8c305`

## Objective and boundary

This report maps the current xAI public API and first-party Grok-session
implementation onto Vesper's existing provider-neutral contracts. It decides
whether VRO-18 may proceed and identifies shared abstractions that must be
repaired before xAI production wiring.

The user explicitly required named xAI documentation and inspection of pinned
Grok Build production source. That direction authorizes the narrow deviation
from this directory's usual alias/config-only rule. No source was copied into
Vesper, no production code changed, no credential was read, and no live
provider request was made.

## Proven upstream contract

### Public API-key route

Official documentation currently establishes:

- global inference at `https://api.x.ai`, bearer API-key authentication, and
  `POST /v1/responses` as the primary agentic interface;
- account-visible model discovery at `/v1/models`, with the richer
  `/v1/language-models` response documenting modalities, aliases, pricing, and
  reasoning capabilities;
- Responses streaming, custom function calls and `function_call_output`, image
  input, structured outputs, `previous_response_id`, encrypted reasoning,
  `prompt_cache_key`, `POST /v1/responses/compact`, and WebSocket Responses;
- provider-hosted tools as a separate server-executed class with distinct
  output-item types, usage, citations, egress, and charges;
- `https://us.api.x.ai/v1` as the US endpoint. On this snapshot it lists only
  Grok 4.7 and 4.6. Function tools, structured output, files, and hosted tools
  use the same request surface, but files/hosted tools are outside the regional
  processing guarantee.

The public API route is stable enough for fixture-first PR-1 implementation.

### Grok-account/session route

The pinned first-party source is Apache-2.0 and was committed 2026-09-23. The
following symbols establish a direct, first-party implementation path:

| Contract | Pinned source evidence |
|---|---|
| issuer, public client, scopes | `crates/codegen/xai-grok-login/src/config.rs`: `XAI_OAUTH2_ISSUER`, `OAuth2ProviderConfig`, `default_oauth2_scopes`, `GrokComConfig::default` |
| browser OIDC/PKCE | `xai-grok-login/src/oidc/protocol.rs`: discovery validation, `generate_pkce`, state/nonce/JWT validation, token exchange and refresh |
| device authorization | `xai-grok-login/src/device_code.rs`: `/oauth2/device/code`, `/oauth2/token`, pending/slow-down/denied/expired handling and bounded expiry |
| credential refresh | `xai-grok-login/src/oidc/protocol.rs` and manager modules: refresh token rotation and bounded refresh coordination |
| session proxy | `xai-grok-env/src/lib.rs`: production base `https://cli-chat-proxy.grok.com/v1` |
| proxy authentication | `xai-grok-login/src/grok_auth_credentials.rs` and `xai-grok-shell/src/agent/proxy_headers.rs`: bearer token plus `X-XAI-Token-Auth: xai-grok-cli`, client version/identifier/mode headers |
| model routing | `xai-grok-sampler/src/client.rs`: `x-grok-model-override` and `x-grok-conv-id` |
| session catalog | `xai-grok-shell/src/remote/model_source/oai.rs`: authenticated proxy `/v1/models`, no API-key fallback for session mode |
| backend/capability metadata | `xai-grok-shell/src/remote/client.rs::parse_remote_model_value`: context, Responses/Chat/Messages backend, reasoning efforts/default, search, compaction and limits |
| subscription state | `xai-grok-shell/src/agent/subscription_check.rs`: bounded `/user?include=subscription` query exposes tier, not quantitative allowance |

The repository README also documents direct proxy use after `grok login`,
including the bearer token, `X-XAI-Token-Auth`, and model-override header. This
is sufficient evidence that native Vesper session wiring is a supported
first-party integration, rather than credential harvesting or an invented
consumer endpoint. Vesper must independently implement the pinned contract,
own its credentials, and must not read `~/.grok/auth.json` by default.

The session route does not inherit every public Responses feature. Its model
catalog selects the backend and reports capabilities. Vesper must intersect
that discovery with verified static metadata and fixtures; absent evidence is
unsupported, never an API-key fallback.

## Capability matrix

`YES` means the current first-party contract is established. `CONDITIONAL`
means model/catalog/endpoint evidence is required. `UNKNOWN` fails closed.

| Capability | API key / global | API key / US | session / browser | session / device |
|---|---|---|---|---|
| authentication | YES | YES | YES, OIDC/PKCE | YES, RFC 8628 |
| model discovery | YES | YES, endpoint-limited | YES, proxy catalog | YES, proxy catalog |
| reasoning controls | CONDITIONAL by model | 4.7/4.6 only | CONDITIONAL by catalog | CONDITIONAL by catalog |
| Vesper function tools | CONDITIONAL by model | CONDITIONAL by model | CONDITIONAL by model/backend | CONDITIONAL by model/backend |
| structured output | CONDITIONAL by model/schema | CONDITIONAL by model/schema | UNKNOWN until proxy fixture | UNKNOWN until proxy fixture |
| image input | CONDITIONAL by model | CONDITIONAL by model | UNKNOWN until proxy fixture | UNKNOWN until proxy fixture |
| hosted tools/citations | YES, explicit opt-in | YES; outside region guarantee | UNKNOWN; live proxy rejection, fail closed | UNKNOWN; live proxy rejection, fail closed |
| encrypted reasoning | CONDITIONAL, Responses | CONDITIONAL, Responses | CONDITIONAL on Responses backend | CONDITIONAL on Responses backend |
| prompt-cache routing | YES | YES | YES, conversation header | YES, conversation header |
| native compaction | YES, Responses | YES, Responses | UNKNOWN | UNKNOWN |
| Responses WebSocket | YES | YES | UNKNOWN | UNKNOWN |
| token/tool usage | YES | YES | CONDITIONAL by backend | CONDITIONAL by backend |
| account quota | no verified balance endpoint | no verified balance endpoint | tier only; allowance unavailable | tier only; allowance unavailable |

The PR-8 live run refined the session transport evidence: successful proxy
inference requires the pinned first-party authenticate-response, protocol
version, client identity/mode, model-override, and per-turn correlation headers
in addition to bearer and token-auth. A controlled Grok-session hosted Web
Search attempt was rejected by the proxy, so hosted tools remain unavailable
in session mode instead of inheriting public API capability.

### Current model corrections

The submitted PRD is directionally correct but two current documents conflict
with its starting table:

1. The current reasoning guide lists Grok 4.5 as low/medium/high. It says
   `xhigh` on models without support is treated as `high`; Vesper must not
   advertise that as a distinct supported level.
2. The Grok 4.20 multi-agent model detail page carries broad function/structured
   capability badges, while the dedicated multi-agent guide says client-side
   and custom tools are unsupported. The dedicated guide is the narrower
   execution contract. Vesper must reject its own function tools for this model
   until xAI reconciles the documentation and fixtures establish support.

Discovered model names never confer capabilities by similarity. Retired aliases
that redirect to Grok 4.3 remain aliases, not independent selectable models.

## Current Vesper mapping

| xAI concern | Existing Vesper seam | Verdict |
|---|---|---|
| provider identity/config | `ProviderFactory`, `ProviderDescriptor`, versioned `ProviderConfiguration` | reuse |
| API/session credential isolation | adapter-owned credential records plus configuration envelope | reuse, require separate record keys |
| device login/logout | `ProviderCredentialPort::device_login` and `logout` | reuse |
| browser login | no generic credential operation | shared gap before PR-3 |
| dynamic catalog | `ModelCatalog`/`ModelCatalogSnapshot` | trait exists; registry projection gap |
| per-model capabilities | `ModelDescriptor` + `ProviderCapabilities` | reuse and extend only when proven |
| reasoning controls | `ReasoningIntent`, `ProviderSuperpowers`, `SuperpowerPolicy` | reuse; model-filtered choices |
| text/image/history | `ProviderRequest` and ordered `ContentPart` | reuse |
| structured output | `StructuredOutputIntent` | reuse with xAI subset validator |
| client function tools | normalized `ToolDefinition`, `ToolChoice`, shared `AgentLoop` | reuse |
| opaque reasoning | `ReasoningBlock`/`OpaqueContinuation` and `ProviderOpaque` | reuse |
| response continuation | `ContinuationContext::NativeContinuation` | reuse |
| prompt caching | capability + adapter-owned request extension | reuse |
| native compaction | `AuxiliaryRequestIntent::Compaction` plus opaque content | likely reuse; prove transactional integration in PR-6 |
| hosted tools | no typed descriptor/enablement distinction | shared gap before PR-5 |
| citations/source annotations | no typed stream/content citation event | shared gap before PR-5 |
| usage | normalized token/rate/quota types | reuse; unavailable values stay unavailable |
| stream settlement | ordered `ProviderStreamEvent` contract | reuse |

## Host and registry findings

The runtime registry is provider-neutral for creation, credentials,
descriptors, superpowers, and policy. `AgentLoop` receives a `ProviderId` and
normalized tools and therefore needs no xAI branch.

Three composition problems must be addressed generically:

1. `ModelCatalog` exists but `ProviderRegistry` does not retain or expose a
   catalog trait object. Dynamic LM Studio catalogs are separately host-owned.
   xAI must not create a third host-specific catalog path.
2. TUI functions `provider_configuration_for`, `model_id_for_provider`, and
   `default_context_window_for_provider` contain concrete provider match arms.
   ACP similarly constructs provider defaults/catalog windows at composition.
   Registration may name a concrete crate, but execution defaults must become
   descriptor/catalog-driven before xAI is selectable.
3. The credential port supports device challenges but no browser initiation or
   explicit generic auth-method selection operation. Add the smallest neutral
   operation rather than an xAI host callback.

PR-5 resolved the hosted-tool gap with shared descriptor/selection types;
citations remain bounded provider-owned content until PR-7 host rendering.
They must remain distinct from Vesper local tools and local MCP.

Voice, skill routing, VRO, compaction policy, memory, and swarm workers already
compose the selected provider through `ProviderRegistry`/`AgentLoop`. The source
trace found no need for xAI-specific execution wiring in those systems.

## Proposed phase dependencies

- **PR-1:** add the xAI crate and API-key Responses transport. Generalize
  provider defaults/catalog lookup only as needed for controlled composition.
- **PR-2:** implement discovery intersected with a versioned static capability
  index; freeze the two documented contradictions as denial fixtures.
- **PR-3:** add generic browser-login/auth-method selection, then implement
  first-party OIDC/device/session proxy. Red-first billing-isolation tests are
  mandatory.
- **PR-4:** reuse the shared tool loop and opaque continuation; add typed
  citations only if brought forward from PR-5.
- **PR-5:** introduce generic hosted-tool descriptors, consent/config state,
  typed results/citations, then map xAI tools.
- **PR-6:** validate native compaction through the existing policy and add
  WebSocket only after HTTP/SSE semantic parity.
- **PR-7:** remove remaining host-specific execution defaults and prove TUI/ACP,
  voice, skills, VRO, memory, and worker inheritance.

## Stop-rule verdict

The stop rule does **not** block Grok-session implementation. Current official
documentation and pinned first-party source establish a stable browser/device
authentication flow, refresh lifecycle, proxy origin/header contract, model
override, conversation routing, and session catalog. Implementation must remain
fixture-first and pinned; any session feature outside that evidence stays
unsupported without switching billing routes.

PR-0 is PASS. PR-1 may begin. VRO-18 remains open and makes no production xAI
support claim.

## Official documentation snapshot

- [Inference API overview](https://docs.x.ai/developers/rest-api-reference/inference)
- [Responses reference](https://docs.x.ai/developers/rest-api-reference/inference/responses)
- [Models reference](https://docs.x.ai/developers/rest-api-reference/inference/models)
- [Grok 4.7](https://docs.x.ai/developers/models/grok-4.7)
- [Reasoning](https://docs.x.ai/developers/model-capabilities/text/reasoning)
- [Structured outputs](https://docs.x.ai/developers/model-capabilities/text/structured-outputs)
- [Tools overview](https://docs.x.ai/developers/tools/overview)
- [Multi-agent](https://docs.x.ai/developers/model-capabilities/text/multi-agent)
- [Prompt caching](https://docs.x.ai/developers/advanced-api-usage/prompt-caching)
- [Context compaction](https://docs.x.ai/developers/advanced-api-usage/context-compaction)
- [WebSocket mode](https://docs.x.ai/developers/advanced-api-usage/websocket-mode)
- [Regional endpoints](https://docs.x.ai/developers/advanced-api-usage/regions)
- [Grok Build enterprise authentication](https://docs.x.ai/build/enterprise)
- [Grok Build CLI reference](https://docs.x.ai/build/cli/reference)
- [First-party Grok Build source](https://github.com/xai-org/grok-build)
