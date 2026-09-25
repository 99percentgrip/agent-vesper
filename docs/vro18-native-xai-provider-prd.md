# VRO-18 — Native xAI Provider for Agent Vesper

**Status:** PR-1–PR-6 COMPLETE; PR-7 HOST COMPOSITION IMPLEMENTED — ACCEPTANCE/RELEASE OPEN
**Date:** 2026-09-25
**Target repository:** `99percentgrip/agent-vesper`
**Baseline:** post-`v0.23.6` `main` (`c64e78f46065f1fbaae899ab9914f3d9b3f7023d`)
**Provider ID:** `xai`
**Display name:** `xAI / Grok`
**Primary objective:** first-party-quality native xAI reasoning-provider support with user-selectable Grok-account/subscription authentication or xAI API-key authentication, while preserving Agent Vesper's provider-neutral architecture.

---

## 0. Executive decision

Agent Vesper will support xAI as a **native provider**, not as a generic OpenAI-compatible endpoint and not as a thin "prompt works" integration.

The implementation must preserve xAI's documented semantics where they matter:

- Grok account/browser or device-code authentication through the first-party Grok session path;
- xAI API-key authentication through the public xAI API;
- current account-visible model discovery;
- per-model context/modalities/capabilities;
- native reasoning controls;
- xAI Responses semantics where supported;
- subscription-proxy semantics where that is the first-party supported path;
- function calling through Vesper's existing shared tool loop;
- structured outputs;
- image input;
- streaming, usage and errors;
- encrypted reasoning continuation;
- prompt caching;
- context compaction;
- provider-hosted tools, citations and their billing/egress distinctions;
- first-party session refresh/logout;
- TUI and ACP parity;
- automatic compatibility with Vesper voice, skills, memory, VRO, Swarm/Hive, permissions, MCP and other provider-neutral systems.

The presence of xAI must **not** cause xAI-specific branches to spread through Agent Vesper's shared runtime.

The architecture invariant is:

```text
                         ProviderRegistry
                              │
          ┌───────────────────┼───────────────────┐
          │                   │                   │
          ▼                   ▼                   ▼
       OpenAI               Z.ai                 xAI
    native adapter      native adapter      native adapter
          │                   │                   │
          └───────────────────┼───────────────────┘
                              ▼
                          AgentLoop
                              │
       ┌──────────────────────┼──────────────────────┐
       ▼                      ▼                      ▼
     Tools                  Skills                 Voice
       │                      │                      │
       └──────── permissions / memory / VRO / workers ────────
                              │
                         TUI / ACP hosts
```

Adding `xai` must not require `if provider == "xai"` logic inside shared tool execution, voice conversation, skills, VRO orchestration, memory, compaction policy, worker execution or permission enforcement merely to make those systems function.

If implementation reveals such coupling, repair the shared abstraction first.

---

## 1. Product goal

A user must be able to open:

```text
Settings
→ Providers
→ xAI / Grok
```

and choose one of two explicit billing/authentication modes:

```text
Grok account / SuperGrok session
    Browser sign-in
    Device-code fallback for headless machines
    Refreshable session credential
    Uses the account's Grok/Grok Build entitlement

xAI API key
    Masked API-key entry
    Usage-based xAI API billing
```

These modes are **not interchangeable**.

There must be:

```text
NO silent Grok-session → API-key fallback
NO silent API-key → Grok-session fallback
NO hidden change of billing path
NO reading ~/.grok/auth.json by default
NO requirement to install or run Grok Build
NO requirement to install an xAI Python/JS SDK
```

The user selects the mode. Vesper preserves it until the user explicitly changes it.

---

## 2. Definition of "native provider support"

For VRO-18, "native xAI support" does not mean API compatibility alone.

The xAI provider is production-supported only when all applicable first-party capabilities in the reasoning-provider boundary have an evidence-backed implementation or an explicit fail-closed unsupported classification.

### 2.1 Required native areas

The adapter must own:

1. provider identity and configuration;
2. authentication and credential lifecycle;
3. account/model discovery;
4. static evidence-backed capability metadata;
5. model/auth/endpoint capability intersection;
6. request serialization;
7. stream/event decoding;
8. native reasoning controls;
9. function/tool-call translation;
10. structured output translation;
11. image input;
12. continuation and opaque reasoning state;
13. prompt-cache routing;
14. provider-native context compaction;
15. provider-hosted tool declarations/results/citations;
16. usage/account reporting when first-party evidence supports it;
17. provider error classification and retry rules;
18. cancellation;
19. TUI/ACP settings and execution integration;
20. provider-maintenance/catalog freshness.

### 2.2 Native does not mean duplicate transports

xAI exposes REST and gRPC APIs. Agent Vesper does **not** need to implement every equivalent transport merely because it exists.

The production transport should be the smallest first-party-supported transport that preserves the required semantics:

- public API-key mode: native xAI REST Responses API is the primary path;
- Grok-account/session mode: use the first-party Grok session/proxy contract documented and implemented by xAI;
- alternate transports are added only when they expose a unique required capability or provide a measured product benefit.

This is semantic parity, not protocol-count parity.

---

## 3. Upstream evidence snapshot — 2026-09-25

This section is reconnaissance input, not a frozen implementation assumption. PR-0 must re-read current official documentation immediately before coding.

### 3.1 Public API

Current xAI documentation identifies:

```text
Public inference base:
https://api.x.ai

Responses:
POST /v1/responses

Models:
GET /v1/models

Primary public authentication:
Authorization: Bearer <xAI API key>
```

The current docs describe Responses as the primary interface for text generation, reasoning and tool use.

### 3.2 Current flagship

As of 2026-09-25, xAI documents `grok-4.7` as its current frontier/flagship coding and agentic model.

Documented `grok-4.7` properties include:

```text
context: 500,000
input: text + image
output: text
reasoning effort: low / medium / high / xhigh
default reasoning: high
function calling: yes
structured outputs: yes
Responses API: yes
Chat Completions: yes
public API Grok 4.7 Fast: no
```

Responses returns `reasoning.encrypted_content`; clients are instructed to preserve reasoning items unchanged across turns.

### 3.3 Current documented language-model families

The implementation must not treat this table as a permanent hardcoded catalog. It is the current capability-recon starting point.

| Family | Current documented characteristics relevant to Vesper |
|---|---|
| `grok-4.7` | 500K; text/image → text; function calling; structured output; low/medium/high/xhigh reasoning; high default |
| `grok-4.6` | 500K; text/image → text; function calling; structured output; low/medium/high/xhigh reasoning; high default |
| `grok-4.5` | 500K; text/image → text; function calling; structured output; current reasoning guide lists low/medium/high and treats unsupported xhigh as high |
| `grok-4.3` | 1M; text/image → text; function calling; structured output; configurable reasoning including `none`; model-page summary/detail wording must be reconciled rather than guessed |
| `grok-4.20` reasoning | 1M; text/image → text; function calling; structured output; reasoning |
| `grok-4.20` non-reasoning | 1M; distinct non-reasoning model/aliases; function calling and structured output |
| `grok-4.20-multi-agent` | 1M; beta server-side multi-agent research; dedicated guide currently rejects client-side/custom tools despite broader model-page badges |
| `grok-build-0.1` | 256K; coding model; text/image → text; function calling; structured output; reasoning |

Retired aliases must not be presented as independent current models. Runtime discovery and explicit alias metadata must determine what the account can actually use.

### 3.4 Model discovery is authoritative for availability, not capability

`GET /v1/models` reports models available to the authenticating API key and includes model metadata such as identifiers, aliases, context length and pricing.

Vesper therefore uses:

```text
authenticated discovery
        ∩
verified capability index
        =
selectable production model
```

A discovered model ID alone is **not** permission to guess reasoning, modality, structured-output or tool capabilities.

Unknown discovered models must be represented as one of:

```text
DISCOVERED — CAPABILITY METADATA PENDING
EXCLUDED FROM EXECUTION UNTIL VERIFIED
```

They must not inherit capability metadata from a similarly named model.

### 3.5 Region

xAI currently documents a US regional API endpoint:

```text
https://us.api.x.ai/v1
```

At the current documentation revision, only `grok-4.7` and `grok-4.6` are available there, and voice/image/video APIs are not served from that endpoint.

If Vesper offers regional endpoint selection, the selected endpoint must constrain the model catalog and capabilities truthfully.

### 3.6 Grok-account/session authentication is first-party behavior

Current first-party Grok Build documentation and source document:

- browser OAuth/OIDC login;
- device-code login for headless systems;
- refreshable session credentials;
- `auth.x.ai` as the first-party authentication host;
- `cli-chat-proxy.grok.com` as the Grok Build inference/session proxy;
- an API-key alternative;
- direct use of a cached session token against the CLI chat proxy with required first-party headers.

Therefore VRO-18 may implement **native Grok-account/session authentication** without treating it as an invented or reverse-engineered consumer login hack.

However, Vesper must pin and audit the current first-party Grok Build implementation before production wiring.

### 3.7 Grok subscription and xAI API billing are distinct

Grok/SuperGrok account usage and xAI API-key usage are separate billing/entitlement paths.

The UI must make this distinction explicit.

A successful Grok sign-in does not authorize Vesper to consume API-key credits.

A configured API key does not authorize Vesper to silently consume a Grok subscription session.

---

## 4. Upstream source oracle

PR-0 must pin a specific current commit of:

```text
https://github.com/xai-org/grok-build
```

The repository is first-party xAI source and is Apache-2.0 licensed at the time of this PRD.

Use it as an implementation oracle for:

- public-client OAuth/OIDC behavior;
- browser flow;
- device-code flow;
- credential refresh;
- logout;
- session-proxy request headers;
- model override behavior;
- session model discovery;
- timeout/cancellation behavior;
- any documented subscription/session usage surface.

Do not depend on the Grok Build executable at runtime.

Do not silently import its credential file.

If source code is copied rather than independently reimplemented, preserve all applicable Apache-2.0 notices and repository licensing requirements.

Record:

```text
upstream repository
upstream commit SHA
files/symbols inspected
license
docs snapshot date
```

in the implementation evidence.

---

## 5. Architecture

### 5.1 New provider crate

Preferred ownership:

```text
crates/vesper-provider-xai/
```

The crate owns xAI-specific:

```text
auth
catalog/discovery
configuration
request serialization
Responses transport
subscription-proxy transport
stream decoding
reasoning mapping
hosted-tool mapping
usage mapping
error/retry mapping
opaque continuation state
prompt caching
native compaction
```

It implements existing provider contracts:

```text
ProviderFactory
ProviderSession
ModelCatalog
ProviderCredentialPort
ProviderSuperpowers
ProviderRequest / ProviderStreamEvent
```

where applicable.

### 5.2 No host-owned provider transport

The following are prohibited:

```text
apps/agent-vesper-tui/src/xai_http.rs
apps/agent-vesper-acp/src/xai_responses.rs
special xAI tool executor
special xAI voice submission path
special xAI AgentLoop
```

Hosts may compose the provider and render generic provider-owned controls. Provider wire logic belongs in `vesper-provider-xai`.

### 5.3 Generic capability gap rule

If xAI exposes a capability that Agent Vesper's current provider-neutral contract cannot represent:

1. prove the capability is first-party and useful;
2. prove the shared contract genuinely lacks the concept;
3. add the smallest provider-neutral abstraction;
4. add non-xAI regression fixtures;
5. only then map xAI into it.

Do **not** solve the gap with a scattered `provider_id == xai` branch.

`ProviderRequest.provider_extensions` may carry xAI-specific values that do not belong in the shared semantic contract, provided they remain adapter-owned and versioned.

---

## 6. Authentication architecture

### 6.1 Authentication modes

Stable adapter-owned mode IDs should be equivalent to:

```text
grok-session
api-key
```

User-facing labels should clearly explain billing:

```text
Grok account / SuperGrok
Uses your signed-in Grok account and its available Grok/Grok Build allowance.

xAI API key
Uses xAI API billing/credits separately from a Grok subscription.
```

### 6.2 Grok session flow

Implement directly in Rust using the current first-party xAI public-client contract.

Required behaviors:

```text
browser OAuth/OIDC sign-in
device-code fallback
bounded polling
PKCE where required by current contract
fixed trusted auth origins
state/nonce/PKCE validation
refresh token lifecycle
expiry handling
one bounded retry after refreshable auth failure
logout
cancellation
cross-process credential locking
secret-safe errors
```

The normal Vesper workflow must not require the Grok CLI.

### 6.3 Subscription inference transport

Based on the current first-party Grok Build documentation, the session path uses the first-party Grok CLI chat proxy rather than the API-key `api.x.ai` endpoint.

The current documented contract includes:

```text
Authorization: Bearer <session token>
X-XAI-Token-Auth: xai-grok-cli
x-grok-model-override: <selected model>
```

PR-0 must re-derive the exact current headers, base URL, model-routing behavior and request shape from pinned xAI source.

Do not assume subscription mode supports every Responses-only API feature.

Build an auth-mode capability matrix and fail closed when a feature is unavailable on one path.

### 6.4 API-key flow

API mode uses:

```text
https://api.x.ai/v1
Authorization: Bearer <XAI API key>
```

The normal UI stores the credential through Vesper's existing credential-management boundary.

Environment-variable support may exist for headless/CI use, but masked Settings entry is the normal interactive workflow.

### 6.5 No silent fallback

The following is a security/billing invariant:

```text
session auth failure
≠
permission to try XAI_API_KEY
```

and:

```text
API-key failure
≠
permission to use cached session entitlement
```

Fallback between authentication/billing modes requires a new explicit user choice.

### 6.6 Credential isolation

Do not read:

```text
~/.grok/auth.json
```

by default.

Vesper owns its own credential records.

A future explicit "Import Grok login" feature, if desired, is a separate user-authorized migration and is not required for VRO-18.

---

## 7. Model catalog and maintenance

### 7.1 Dynamic discovery

API-key mode must use current account discovery rather than a stale static menu.

Subscription/session mode must use the first-party model-discovery mechanism used by the pinned Grok Build source or another current official xAI session endpoint.

The two catalogs are separate snapshots because account entitlements may differ.

### 7.2 Static capability index

Dynamic discovery answers:

> What model identifiers can this account use?

The static/evidence-backed capability index answers:

> What does Vesper know this model can safely do?

Both are required.

For every model, capture where supported:

```text
canonical ID
aliases
display name
context limit
output limit
input modalities
output modalities
reasoning controls
reasoning default
function-calling capability
structured-output capability
prompt caching
continuation capability
native compaction capability
batch support
hosted-tool support
endpoint/region constraints
beta/experimental status
```

### 7.3 Aliases

Preserve xAI's distinction:

```text
moving alias
dated/fixed model
deprecated/redirected alias
```

Settings should make consistency-vs-latest behavior understandable.

Do not duplicate dozens of retired aliases in the main picker merely because requests still redirect.

### 7.4 Freshness

Provider quality requires ongoing catalog maintenance.

Add an explicit freshness workflow:

```text
xAI docs/source changes
→ capability recon
→ fixtures update
→ static capability index update
→ discovery compatibility tests
→ release
```

A runtime-discovered unknown model must not silently receive capabilities.

---

## 8. Reasoning semantics

### 8.1 Native effort levels

Map xAI's actual per-model reasoning levels, not a single global boolean.

Examples from the current docs include:

```text
grok-4.7: low / medium / high / xhigh
grok-4.6: low / medium / high / xhigh
grok-4.5: low / medium / high; do not expose xhigh unless newer first-party
          evidence supersedes the current reasoning guide
grok-4.3: supports a no-reasoning mode plus reasoning levels; reconcile current
          documentation wording during PR-0 before freezing the exact UI list
```

Settings must derive available levels from the selected model's capability record.

### 8.2 No invented disable switch

Models that cannot disable reasoning must not expose an "off" control.

Models that have a documented non-reasoning mode may expose it.

### 8.3 Multi-Agent Beta is not ordinary reasoning depth

`grok-4.20-multi-agent` is a distinct beta execution mode where multiple xAI-side agents collaborate.

Current xAI documentation shows the Responses reasoning-effort control can influence that multi-agent setup.

Vesper must not flatten this into a misleading generic "thinking depth" label.

Represent it using:

- model-specific capability metadata; and/or
- a provider superpower / provider-specific versioned option whose UI explains the semantics.

### 8.4 Hidden reasoning

Never display private chain-of-thought merely because an API field contains provider reasoning state.

Encrypted reasoning is opaque continuation state.

User-visible reasoning diagnostics may show only provider-supported summaries or safe metadata when the provider explicitly exposes them as such.

---

## 9. Public API transport

### 9.1 Responses API is primary

For API-key mode, use the xAI Responses API as the primary production interface.

Implement:

```text
text input
image input
system/developer/user/assistant history as supported
streaming output
function calls
function_call_output
previous_response_id where selected
structured output
usage
reasoning metadata
encrypted reasoning preservation
errors
cancellation
```

Do not implement xAI merely by changing the base URL of the OpenAI adapter.

Protocol compatibility is useful evidence, not ownership.

xAI gets its own adapter so xAI-specific semantics can evolve independently.

### 9.2 Chat Completions

Chat Completions may be used where required by the first-party Grok session proxy or when a provider capability is documented only there.

Do not silently switch API-key mode between Responses and Chat Completions when semantics differ.

### 9.3 Streaming

Streaming must be bounded and cancellation-safe.

Tests must cover:

```text
fragmented SSE
fragmented UTF-8
message deltas
reasoning metadata
tool calls
hosted-tool events
citations
usage
terminal completion
provider error after visible output
disconnect
timeout
cancellation
oversized event/data
```

Ambiguous or incomplete tool calls must never be replayed as if successful.

---

## 10. Tool support

### 10.1 Vesper native tools remain client-side functions

Agent Vesper's existing tools:

```text
read_file
list_directory
search_files
grep
write_file
edit_file
apply_patch
run_command
update_plan
...
```

must be passed to xAI as ordinary custom function definitions and resolved through the existing shared `AgentLoop`.

The xAI adapter translates schema/name/call IDs.

It does **not** execute these tools.

No new xAI-specific tool executor is permitted.

### 10.2 Tool-surface parity gate

Given the recently repaired shared executor incident, VRO-18 must explicitly prove:

```text
OpenAI tool surface
Z.ai tool surface
xAI tool surface
```

all originate from the same intended shared registry after normal model/capability filtering.

Provider-specific wire names may differ, but all expected Vesper tools must remain executable.

### 10.3 Strict tool schemas

xAI documentation states function-call arguments conform strictly to the declared JSON schema.

Validate Vesper's tool schemas against the xAI-supported JSON Schema subset before dispatch.

Fail closed on schemas xAI cannot represent.

Do not weaken the shared schema to make a request pass.

---

## 11. xAI provider-hosted tools

xAI also offers server-executed tools such as:

```text
Web Search
X Search
Code Execution / Code Interpreter
Image Generation
Attachment Search
Collections Search / File Search
Remote MCP
```

These are **not the same thing** as Vesper's local/native tools.

### 11.1 Explicit opt-in

Provider-hosted tools that cause extra egress, remote execution or additional charges must be disabled unless the user explicitly enables them.

Settings should distinguish:

```text
Vesper Web Tools
Runs through Vesper's own web subsystem and permissions.

xAI Web Search
Runs on xAI infrastructure and may have separate provider charges.
```

Likewise:

```text
Vesper run_command
Local/contained Vesper execution.

xAI Code Execution
Remote provider-side execution.
```

Never substitute one for the other silently.

### 11.2 Generic hosted-tool abstraction

If Agent Vesper does not yet have a provider-neutral representation for server-side provider tools, PR-0 must audit the gap.

Preferred solution:

```text
generic HostedToolDescriptor / capability
        ↓
provider advertises hosted tools
        ↓
generic Settings/host projection
        ↓
provider maps enabled descriptors to wire format
```

Avoid hardcoded xAI toggles scattered across TUI/ACP/runtime.

### 11.3 X Search

X Search is an xAI-specific capability, but it can still be represented as a provider-hosted tool descriptor.

It does not justify an xAI branch in `AgentLoop`.

### 11.4 Remote MCP

xAI Remote MCP means **xAI servers connect to the MCP endpoint**.

That is not Vesper-native MCP.

The UI and security model must distinguish them clearly.

Remote MCP must never inherit Vesper's existing local MCP enablement implicitly.

### 11.5 Citations

Preserve provider citations and source metadata as typed provider output where possible.

Render citations in both TUI and ACP without converting them into fake Vesper web-tool events.

---

## 12. Structured outputs

Implement current xAI structured-output semantics.

Support the existing provider-neutral intents where compatible:

```text
None
JsonObject
JsonSchema
```

Validate against xAI's supported JSON Schema subset.

Do not claim full arbitrary Draft 2020-12 behavior where xAI documents only a practical subset.

Tests must cover:

```text
valid schema
unsupported schema keyword
nested schema
$ref/$defs within supported limits
tool calling + structured output combination
malformed provider output
schema mismatch
```

---

## 13. Image input

For models that support image input:

- route Vesper's existing image content through the provider-neutral media contract;
- enforce current xAI size/type constraints from capability metadata;
- preserve text/image ordering;
- fail before dispatch on unsupported model/media combinations.

Do not add an xAI-specific image-input path to the AgentLoop.

---

## 14. Continuation and encrypted reasoning

### 14.1 Responses continuation

Support xAI's current continuation choices where appropriate:

```text
previous_response_id
client-managed input history
opaque encrypted reasoning items
```

### 14.2 Opaque reasoning

`reasoning.encrypted_content` must be:

```text
provider-owned
opaque
versioned/persisted safely if continuation needs it
passed back unchanged
never parsed
never exposed as hidden CoT
```

If persistence cannot safely round-trip an opaque provider item, extend the provider-neutral versioned-extension boundary rather than dropping it silently.

### 14.3 Interruption

Cancellation must preserve the same Agent Vesper rule:

```text
ambiguous interrupted tool transaction
→ never replay automatically
```

Provider continuation after interruption must not resurrect a tool call that Vesper has classified as ambiguous/cancelled.

---

## 15. Prompt caching

xAI supports automatic prompt caching and recommends stable conversation routing:

```text
Responses:
prompt_cache_key

Chat Completions:
x-grok-conv-id
```

Use a stable Vesper-owned non-secret session/conversation identifier.

Do not include credentials, user secrets, raw paths or private prompt content in the cache key.

Expose cached-token usage through the provider usage mapping when returned.

Cache misses are normal and must not affect correctness.

---

## 16. Native context compaction

xAI exposes:

```text
POST /v1/responses/compact
```

with an opaque encrypted compaction item that is passed back verbatim.

### 16.1 Integration rule

Agent Vesper already owns provider-neutral compaction policy and persistence.

xAI-native compaction is an adapter capability, not permission for xAI to replace Vesper's global context-governance architecture.

Implement a clean provider-native compaction port/strategy if the existing abstraction can support it.

If a new generic capability is required, make it provider-neutral.

### 16.2 Opaque item handling

Never parse or reorder `encrypted_content`.

Preserve transactional rollback: if native compaction fails, the previous usable Vesper context remains intact.

### 16.3 Policy

Do not automatically enable provider-native compaction merely because it exists.

The final policy must be explicit about:

```text
when Vesper's existing compaction is used
when xAI native compaction is eligible
how cost/latency/context thresholds are chosen
how persisted sessions resume
```

---

## 17. WebSocket Responses mode

xAI currently offers Responses API WebSocket mode for long-lived agentic workloads.

VRO-18 includes it as a native optimization phase after HTTP/SSE semantics are green.

Required properties:

```text
same ProviderRequest semantics
same ProviderStreamEvent semantics
same cancellation
same tool IDs
same opaque continuation
bounded connection lifecycle
reconnect rules that do not replay ambiguous work
no auth-mode switching
```

HTTP/SSE remains a correctness fallback within the **same authentication/billing mode**.

Do not make WebSocket a separate provider.

---

## 18. Usage, limits and billing presentation

### 18.1 API-key mode

Surface what the public API truthfully returns:

```text
input tokens
cached input tokens
output tokens
reasoning tokens
server-side tool usage/cost metadata when available
rate-limit information when available
model context limit
```

Do not estimate account balance unless xAI exposes an authoritative endpoint Vesper has verified.

### 18.2 Grok-account mode

PR-0 must inspect the current first-party Grok Build source for the official session usage/quota surface.

If a stable first-party endpoint/contract exists, implement it natively.

If not, display:

```text
Subscription usage: unavailable through the verified session protocol
```

rather than fabricating a quota.

### 18.3 Billing warning

Settings must explicitly say:

```text
Grok account mode uses Grok account entitlement/allowance.

API-key mode uses separately billed xAI API credits.
```

---

## 19. Error and retry semantics

Build an xAI-specific typed error mapper.

At minimum classify:

```text
authentication expired
authentication denied
entitlement/model unavailable
rate limit
quota/usage exhausted
invalid request
unsupported capability
context limit
provider overload
server error
network timeout
stream interruption
malformed protocol
oversized response/event
cancelled
unknown/ambiguous outcome
```

Retry only when xAI's semantics and Vesper's tool-safety rules permit it.

Never retry a provider turn in a way that can duplicate a potentially side-effecting Vesper tool execution.

---

## 20. Settings and host UX

### 20.1 TUI

Settings → Providers → xAI should expose adapter-driven controls:

```text
Authentication
  Grok account / SuperGrok
  xAI API key

Sign in / Sign out / Re-authenticate
Device-code sign-in where needed

Model
  account-visible verified models

Reasoning
  selected-model-supported controls only

API region
  Global
  US regional (API-key mode where supported)

Provider-hosted tools
  explicit opt-in toggles with cost/egress explanation

Status
  auth mode
  model
  reasoning
  endpoint
  verified capability state
  usage if authoritative
```

No hand-editing config is the normal path.

### 20.2 ACP

ACP must expose the same provider choice, auth state, model controls and provider-owned capabilities through its existing native control surfaces.

Terminal-specific browser presentation may differ, but execution semantics must match.

### 20.3 Generic UI rule

Do not clone an xAI-specific settings subsystem if existing provider descriptor/superpower/auth surfaces can render it.

If xAI needs new control kinds, extend the generic provider UI contract.

---

## 21. Interaction with existing Agent Vesper systems

### 21.1 Voice

Selecting xAI as reasoning provider must automatically work with F9 conversation through the existing provider registry and AgentLoop.

Expected architecture:

```text
STT provider
→ final transcript
→ generic AgentLoop
→ xAI provider
→ text response
→ TTS provider
```

No `voice_xai.rs` reasoning integration is allowed.

### 21.2 Skills and routing

Skills, enhanced routing and explicit skill invocation must work through the same provider-neutral request flow.

No xAI-specific skill path.

### 21.3 Memory and compaction

Memory extraction/recall must use normal provider-neutral composition unless a provider-native auxiliary path is explicitly required and capability-gated.

### 21.4 Swarm/Hive/sub-agents

xAI must be eligible anywhere the selected provider can already be used through `ProviderRegistry`.

Do not create xAI-specific worker factories outside the normal provider factory path.

### 21.5 VRO / Plan / tools

Direct, VRO, ReAct and Plan workflows must use the same provider instance and the same shared tool registry.

---

## 22. Adjacent xAI platform APIs

The public xAI platform also exposes:

```text
Voice API
  speech-to-speech
  speech-to-text
  text-to-speech

Imagine API
  image generation/editing
  video generation/editing
```

These are real xAI services but they are not the same abstraction as the LLM reasoning provider.

### 22.1 Voice

Agent Vesper already has provider-neutral:

```text
VoiceStt
VoiceTts
```

A later VRO-18.x phase may add xAI cloud speech adapters through those ports.

Do not wire xAI speech into `vesper-provider-xai`.

Do not claim xAI cloud voice is supported until the actual speech adapters and privacy/egress/billing gates pass.

### 22.2 Image/video generation

If Agent Vesper lacks a provider-neutral media-generation port, do not hardcode xAI Imagine into the TUI.

Track it as a separate provider-neutral media capability initiative.

### 22.3 Completion labels

Use two different claims:

```text
NATIVE xAI REASONING PROVIDER — PRODUCTION
```

when this PRD's provider acceptance gates pass.

Do **not** say:

```text
FULL xAI PLATFORM — COMPLETE
```

until separately implemented Voice/Imagine service integrations are also accepted.

This distinction prevents "supported provider" from becoming a misleading umbrella claim.

---

## 23. Security requirements

### R-S1 — Fixed origins

Production auth/inference endpoints must be allowlisted by adapter configuration and current official evidence.

Redirects that could leak credentials fail closed.

### R-S2 — Secret isolation

Credentials never appear in:

```text
logs
errors
telemetry
session transcripts
provider extensions visible to the model
tool outputs
```

### R-S3 — Session/API separation

Grok session tokens and xAI API keys are stored and resolved separately.

### R-S4 — Explicit remote-tool egress

xAI hosted tools are separate egress/processing paths and require explicit user opt-in.

### R-S5 — No foreign credential harvesting

Do not read Grok Build, browser or other application credentials by default.

### R-S6 — Cancellation

Browser/device auth, model discovery and provider requests must be cancellation-aware and bounded.

### R-S7 — Credential refresh

Refresh is one explicitly bounded credential operation. A refresh failure does not switch billing modes.

### R-S8 — Logging

Headers such as Authorization/session token values are never logged.

---

## 24. Dependency policy

Prefer existing workspace primitives:

```text
reqwest/rustls
tokio
tokio-util
serde/serde_json
existing credential store
existing provider runtime contracts
```

Do not add an xAI SDK dependency merely to avoid implementing the small wire contract.

Do not embed Python, Node, Grok Build, browser automation or a separate agent runtime.

Any new dependency must pass:

```text
license
MSRV 1.88
five-target support
supply-chain review
maintenance need
binary-size impact
```

---

## 25. Requirements ledger

### R1 — Native provider crate

`vesper-provider-xai` owns xAI-specific production behavior.

### R2 — Two explicit auth modes

Grok-account/session and API-key modes are first-class user choices.

### R3 — Native browser login

Interactive Grok-account login works without Grok Build installed.

### R4 — Native device login

Headless/device-code login works without Grok Build installed.

### R5 — Refresh/logout

Session refresh, expiry, revocation and logout are bounded and tested.

### R6 — Billing-path isolation

No implicit switch between session and API billing.

### R7 — Dynamic model discovery

Account-visible model discovery exists separately for each auth path.

### R8 — Verified capability index

No capability is inferred from model-name similarity.

### R9 — Current model coverage

Every currently account-accessible xAI language model with verified metadata is selectable; unverified models fail closed.

### R10 — Reasoning parity

Model-specific reasoning modes and defaults are faithfully represented.

### R11 — Multi-agent semantics

xAI multi-agent models are not mislabeled as ordinary reasoning depth.

### R12 — Responses transport

API-key mode supports the current xAI Responses API semantics.

### R13 — Subscription transport

Session mode uses the current first-party Grok session/proxy semantics.

### R14 — Streaming

Streaming, usage, terminal state and cancellation are bounded and truthful.

### R15 — Function calling

All eligible Vesper tools round-trip through the existing AgentLoop.

### R16 — Structured outputs

Supported provider-neutral structured-output intents map faithfully.

### R17 — Image input

Supported models accept image content through the generic media path.

### R18 — Opaque reasoning preservation

Encrypted reasoning continuation round-trips unchanged and is never exposed as hidden CoT.

### R19 — Prompt caching

Stable non-secret conversation routing is implemented and usage mapped.

### R20 — Native compaction

xAI compaction is represented as a provider-native capability without breaking Vesper context governance.

### R21 — Hosted tools

xAI server-side tools have explicit provider-owned capability/Settings support and never silently replace Vesper tools.

### R22 — Citations

Provider citations survive into both hosts.

### R23 — WebSocket mode

Responses WebSocket mode reaches parity with the HTTP semantic contract before being enabled as an optimization.

### R24 — TUI/ACP parity

Both hosts expose the same provider execution capabilities within host presentation constraints.

### R25 — Provider-neutral inheritance

Voice, skills, memory, VRO, workers and permissions require no xAI-specific execution wiring.

### R26 — Provider-maintenance policy

Catalog/docs/source provenance and freshness are recorded and maintainable.

### R27 — Five-target quality

MSRV and all five target families pass before release.

---

## 26. Reconnaissance phase — PR-0

**No production implementation before this phase closes.**

PR-0 must produce:

```text
docs/architecture/recon_xai_native_provider.md
docs/foundation/xai-provider-recon-execution.md
```

or repository-equivalent paths.

### 26.1 Read current Vesper ownership

Trace:

```text
ProviderFactory
ProviderSession
ProviderRegistry
ProviderCredentialPort
ProviderSuperpowers
ModelCatalog
ProviderCapabilities
ProviderRequest
ProviderStreamEvent
AgentLoop
Settings provider hub
ACP provider controls
OpenAI adapter
GLM adapter
LM Studio adapter
voice reasoning submission path
worker/swarm provider composition
```

### 26.2 Audit xAI official documentation

At minimum:

```text
overview
REST API reference
Responses
Models
Grok 4.7
Reasoning
Function Calling
Structured Outputs
Tools Overview
Web Search
X Search
Code Execution
Files / Collections Search
Remote MCP
Prompt Caching
Context Compaction
WebSocket Mode
Regional Endpoints
Grok Build overview
Grok Build authentication
Grok Build enterprise auth/networking
current release notes
model retirement/migration notices
```

### 26.3 Pin first-party Grok Build source

Pin the current commit and inspect exact source for:

```text
OAuth client/public registration
auth issuer/endpoints
PKCE
device-code endpoint/flow
scope list
token refresh
credential expiration
logout
session proxy URL
required token-auth header
model-override header
model discovery
usage/quota if implemented
streaming request path
```

### 26.4 Produce a capability matrix

Required dimensions:

```text
API key / Global
API key / US regional
Grok session / browser
Grok session / device code
```

against:

```text
models
reasoning
function calling
structured output
image input
hosted tools
citations
encrypted reasoning
prompt cache routing
native compaction
WebSocket
usage/quota
```

### 26.5 Stop rule

If first-party current evidence does not establish a stable Grok session protocol that Vesper can implement directly, stop **subscription production wiring only** and report the exact blocker.

Do not replace the requested subscription mode with an unofficial workaround.

API-key implementation may proceed independently if its contracts are complete.

---

## 27. Implementation plan

### PR-1 — Provider scaffold + API-key Responses transport

Deliver:

```text
vesper-provider-xai
ProviderDescriptor
ProviderFactory
API-key credential path
base configuration
Responses serializer
SSE decoder
typed errors
cancellation
loopback fixtures
```

No Settings claim beyond controlled developer composition yet.

### PR-2 — Catalog + native reasoning + structured/media input

Deliver:

```text
dynamic API model discovery
static capability index
alias/retirement handling
model limits
reasoning levels/defaults
multi-agent metadata
structured outputs
image input
capability denial
```

### PR-3 — Native Grok-account authentication + subscription proxy

Deliver:

```text
browser OAuth/OIDC
device-code flow
credential persistence
refresh
logout
subscription proxy
session model discovery
billing-path isolation
no Grok runtime dependency
```

Red-first tests must prove a failed session does not fall through to API billing.

### PR-4 — Tools + continuation + usage

Deliver:

```text
Vesper function-call parity
call ID mapping
tool result continuation
encrypted reasoning preservation
previous_response_id
usage
prompt_cache_key / conversation routing
citations
```

### PR-5 — xAI hosted tools

Deliver provider-neutral hosted-tool representation if required, then:

```text
Web Search
X Search
Code Execution
attachment/file search
Collections Search
Remote MCP
provider-hosted image generation tool where appropriate
```

All remote tools opt-in and separately labeled from native Vesper equivalents.

### PR-6 — Native compaction + WebSocket

Deliver:

```text
Responses compaction
opaque item persistence
rollback/failure behavior
WebSocket Responses
semantic parity with HTTP
safe reconnect/no replay
```

### PR-7 — Hosts + cross-system production composition

Deliver:

```text
TUI Settings
ACP controls
provider switch
auth switch
model switch
reasoning switch
usage/status
direct/VRO/ReAct/Plan
voice reasoning-provider inheritance
skills
memory
compaction
workers/swarm
MCP
permissions
resume/persistence
```

### PR-8 — Full acceptance + release

Deliver:

```text
live user acceptance
exact-commit canonical
MSRV
five-target
web-driver
supply-chain
docs
release
```

No release tag before exact-commit gates pass.

---

## 28. Red-first test charter

### Authentication

```text
browser success
browser cancel
state mismatch
PKCE mismatch
device pending
device slow_down
device expired
device denied
device cancelled
refresh success
refresh rotation
refresh failure
logout
oversized auth response
redirect refusal
wrong origin
cross-process lock
session does not fall back to API key
API key does not fall back to session
```

### Model discovery

```text
valid catalog
empty catalog
unknown model
retired alias
moving alias
malformed metadata
oversized response
endpoint-specific catalog
account loses entitlement
refresh changes model list
```

### Reasoning

```text
each supported effort serialized exactly
unsupported effort rejected before dispatch
model without off refuses off
non-reasoning model maps correctly
multi-agent semantics not mislabeled
```

### Streaming

```text
fragmented UTF-8
fragmented SSE boundaries
visible text then transport failure
usage terminal
encrypted reasoning item
server-tool event
citation event
provider error
cancel before first byte
cancel after visible byte
oversized event
```

### Function tools

```text
full shared registry offered
named/auto/required/none where supported
one call
multiple calls
call ID round trip
tool result continuation
invalid args
unknown tool
incomplete call
interruption before tool execution
interruption after ambiguous provider event
no replay
```

### Hosted tools

```text
disabled → absent from request
enabled → exact wire descriptor
billing/egress text present
native Vesper tool remains distinct
server code execution never maps to run_command
xAI Remote MCP never inherits Vesper MCP silently
citations preserved
```

### Continuation

```text
previous_response_id
client-managed history
encrypted reasoning round trip byte-identical
compaction round trip byte-identical
resume persistence
cancelled/ambiguous tool not replayed
```

### Host composition

```text
TUI API-key mode
TUI Grok-session mode
ACP API-key mode
ACP Grok-session mode
provider switch away/back
model switch
reasoning switch
Settings discard
Settings save
voice F9 with xAI reasoning provider
skill-assisted turn
VRO turn
worker/sub-agent turn
read-only permission denial
```

---

## 29. Provider-neutrality acceptance

Before VRO-18 can close, prove the following by source-diff audit and executable tests.

### 29.1 Forbidden integration pattern

The implementation must not require new xAI checks in:

```text
vesper-agent tool executor
voice capture/transcription/playback
generic skill router
generic memory store
generic permission gate
generic VRO planner
generic worker task execution
```

### 29.2 Allowed composition changes

Expected changes include:

```text
new provider crate
workspace dependency
provider registry composition
generic provider settings/auth projection
documentation
capability abstractions proven generically necessary
tests/fixtures
```

### 29.3 Cross-provider regression

Run a provider-neutral fixture matrix covering at least:

```text
OpenAI
Z.ai
xAI
synthetic/fake provider
```

through the same:

```text
typed user turn
tool call
tool result
continuation
final response
cancellation
```

Any xAI implementation change that breaks OpenAI/GLM provider parity blocks release.

---

## 30. Live acceptance

Foundation tests must not consume user quota.

Live acceptance is an explicit user-operated phase after offline/loopback gates pass.

### 30.1 Grok-account/session acceptance

With Alex's authorization:

```text
sign in through Vesper
confirm no Grok Build process installed/launched
discover account models
select current Grok model
select reasoning level
simple chat
shared read_file tool
shared run_command tool
multi-turn continuation
cancellation
resume
image input
one hosted-tool case if enabled
usage/status if supported
logout / re-login
```

Confirm the session path is actually used and no API key billing fallback occurs.

### 30.2 API-key acceptance

Only if Alex chooses to provide an xAI API key/credits:

```text
authenticate
discover API models
one ordinary turn
one function tool
reasoning control
structured output
image input
continuation
```

Do not require paid API acceptance to prove Grok-session support, and vice versa.

---

## 31. Performance and reliability

Measure, do not guess:

```text
time to first content
time to first tool call
stream gap behavior
auth refresh latency
catalog refresh latency
cache hit usage
WebSocket vs HTTP latency
memory bounds
connection cleanup
```

No performance optimization may weaken:

```text
cancellation
tool identity
error truthfulness
billing-path isolation
provider-neutral architecture
```

---

## 32. Release gates

The final release candidate must pass the repository's current equivalents of:

```text
cargo fmt --check
strict Clippy
workspace/default tests
workspace/all-feature tests
architecture
naming guard
acceptance
Cargo Deny
RustSec
MSRV 1.88
five-target foundation
web-driver/native-host gates
release metadata/package validation
```

Exact commit only.

Do not use older CI as release evidence.

If cross-platform CI exposes platform-specific auth/browser/device behavior, repair and rerun before tagging.

---

## 33. Documentation deliverables

Required user documentation should include:

```text
docs/xai-provider.md
```

or repository-equivalent ownership.

It must explain:

- Grok-account vs API-key billing;
- browser login;
- device-code login;
- model selection;
- reasoning controls;
- current model discovery;
- Global vs US endpoint if exposed;
- hosted-tool privacy/cost implications;
- prompt caching;
- native compaction;
- usage limitations;
- authentication reset/logout;
- explicit unsupported adjacent xAI services;
- troubleshooting.

Engineering evidence should include:

```text
xAI reconnaissance
auth execution report
transport/tool execution report
host-parity report
live-acceptance report
release report
```

Each must distinguish tests from real-account evidence.

---

## 34. Definition of done

VRO-18's native xAI reasoning provider is complete only when:

```text
AUTH — GROK SESSION: PASS
AUTH — API KEY: PASS at implementation/fixture scope
BILLING MODE ISOLATION: PASS
MODEL DISCOVERY: PASS
CURRENT VERIFIED MODEL COVERAGE: PASS
REASONING: PASS
MULTI-AGENT SEMANTICS: PASS
RESPONSES API: PASS
SUBSCRIPTION PROXY: PASS
STREAMING: PASS
FUNCTION TOOLS: PASS
STRUCTURED OUTPUT: PASS
IMAGE INPUT: PASS
ENCRYPTED REASONING CONTINUATION: PASS
PROMPT CACHING: PASS
NATIVE COMPACTION: PASS
HOSTED TOOLS: PASS
CITATIONS: PASS
WEBSOCKET: PASS
TUI: PASS
ACP: PASS
VOICE PROVIDER INHERITANCE: PASS
SKILLS/VRO/MEMORY/WORKERS: PASS
PROVIDER NEUTRALITY: PASS
MSRV: PASS
FIVE TARGETS: PASS
SECURITY/SUPPLY CHAIN: PASS
LIVE GROK-SESSION ACCEPTANCE: PASS
EXACT-COMMIT RELEASE: PASS
```

If one required provider capability is unimplemented, state it explicitly.

Do not relabel partial coverage as "fully native."

---

## 35. Explicit non-goals

VRO-18 does not authorize:

```text
copying Grok Build as the Vesper runtime
shelling out to Grok Build for inference
using OpenAI adapter with only a changed base URL
reading Grok's auth.json without consent
silently moving between subscription and API billing
replacing Vesper tools with xAI hosted tools
replacing Vesper MCP with xAI Remote MCP
exposing encrypted chain-of-thought
implementing xAI-specific branches throughout shared core
claiming xAI Voice/Imagine platform completeness from reasoning-provider work
```

---

## 36. Decision register

### D1 — Native adapter

Create a real `vesper-provider-xai` adapter.

### D2 — Dual authentication

Grok-account/session and API-key are explicit first-class choices.

### D3 — No runtime dependency

Grok Build is an upstream behavioral/source oracle, not a production dependency.

### D4 — Dynamic availability + static capability truth

Discovery establishes entitlement; verified metadata establishes capability.

### D5 — Responses-first API mode

Use xAI Responses as the primary public API-key transport.

### D6 — First-party subscription proxy

Use the current first-party Grok session transport for Grok-account mode; do not force public API semantics onto it where xAI does not expose them.

### D7 — No billing fallback

Authentication mode never silently changes.

### D8 — Shared Vesper tools

Client-side function calls always execute through the existing AgentLoop.

### D9 — Hosted tools are distinct

xAI server tools are explicit provider-hosted capabilities with separate egress/cost semantics.

### D10 — Encrypted reasoning is opaque

Preserve, never interpret or expose.

### D11 — Native compaction is provider capability

It composes with, rather than replaces, Vesper context governance.

### D12 — WebSocket is an optimization

It must preserve the same semantic contract as HTTP/SSE.

### D13 — Adjacent xAI services remain correctly scoped

xAI Voice and Imagine get separate provider-neutral service integrations; the reasoning provider must not falsely claim them.

### D14 — Maintenance is part of support

A stale xAI catalog is a product defect, not acceptable long-term behavior.

---

## 37. Official source dossier used for this PRD

Re-read all of these during PR-0 because xAI is moving quickly.

### xAI developer docs

```text
https://docs.x.ai/overview
https://docs.x.ai/developers/models
https://docs.x.ai/developers/models/grok-4.7
https://docs.x.ai/developers/models/grok-4.6
https://docs.x.ai/developers/models/grok-4.5
https://docs.x.ai/developers/models/grok-4.3
https://docs.x.ai/developers/models/grok-4.20
https://docs.x.ai/developers/models/grok-4.20-non-reasoning
https://docs.x.ai/developers/models/grok-4.20-multi-agent-0309
https://docs.x.ai/developers/models/grok-build-0.1
https://docs.x.ai/developers/model-capabilities/text/reasoning
https://docs.x.ai/developers/model-capabilities/text/structured-outputs
https://docs.x.ai/developers/model-capabilities/text/multi-agent
https://docs.x.ai/developers/tools/overview
https://docs.x.ai/developers/tools/function-calling
https://docs.x.ai/developers/tools/web-search
https://docs.x.ai/developers/tools/x-search
https://docs.x.ai/developers/tools/remote-mcp
https://docs.x.ai/developers/files
https://docs.x.ai/developers/advanced-api-usage/prompt-caching
https://docs.x.ai/developers/advanced-api-usage/context-compaction
https://docs.x.ai/developers/advanced-api-usage/websocket-mode
https://docs.x.ai/developers/advanced-api-usage/regions
https://docs.x.ai/developers/rest-api-reference/inference
https://docs.x.ai/developers/rest-api-reference/inference/responses
https://docs.x.ai/developers/rest-api-reference/inference/models
https://docs.x.ai/developers/release-notes
https://docs.x.ai/developers/migration/may-15-retirement
```

### Grok / subscription docs

```text
https://docs.x.ai/grok/overview
https://docs.x.ai/grok/faq
https://docs.x.ai/build/overview
https://docs.x.ai/build/enterprise
https://docs.x.ai/build/cli/reference
```

### First-party source

```text
https://github.com/xai-org/grok-build
```

Important source/document areas:

```text
authentication user guide
OAuth/OIDC implementation
device-code implementation
credential refresh
CLI chat proxy transport
model routing/override
model discovery
configuration
```

At the time of PRD research, `xai-org/grok-build` is public, Rust-based, and Apache-2.0 licensed. PR-0 must pin the actual commit used for implementation evidence rather than relying on `main`.

---

## 38. Final implementation principle

The xAI project succeeds when adding xAI feels like adding **one excellent provider adapter**, not adding another alternate Agent Vesper.

The quality bar is:

```text
xAI changes how xAI talks to Vesper.

xAI does not change how Vesper is Vesper.

## 39. Current implementation status — 2026-09-25

PR-0 pinned and audited `xai-org/grok-build@f0e3be1100ef5252488e3be8bb0e91cf68d8c305`.
PR-1 through PR-6 are closed at offline scope. The isolated PR-7 candidate now
registers xAI in TUI and ACP, projects browser/device/API-key authentication,
refreshes account-visible verified models, routes provider controls and hosted
tool selections into the shared AgentLoop, renders citations safely in both
hosts, and uses xAI through the existing voice, skill, VRO, memory and worker
composition. Shared runtime/tool/voice code contains no xAI execution branch.

PR-8 remains open. No live xAI request has been made, no real Grok-account or
API-key acceptance has passed, five-target exact-commit CI has not run, and no
release is authorized. Configured attachment/collection/Remote-MCP host entry
still needs a generic native structured-configuration surface before HOSTED
TOOLS can be marked complete; the adapter wire support remains fail-closed and
offline-tested meanwhile.
```

---

## 39. PR-0 reconciliation — 2026-09-25

PR-0 is closed by
[`architecture/recon_xai_native_provider.md`](architecture/recon_xai_native_provider.md)
and
[`foundation/xai-provider-recon-execution.md`](foundation/xai-provider-recon-execution.md).
No production implementation or live provider request occurred.

The source baseline is `c64e78f46065f1fbaae899ab9914f3d9b3f7023d`.
The first-party source oracle is pinned to
`xai-org/grok-build@f0e3be1100ef5252488e3be8bb0e91cf68d8c305`
(2026-09-23, Apache-2.0).

Current first-party evidence supports beginning the public API-key transport and
a fixture-first Grok-session implementation. Session mode remains a distinct
billing/authentication route through `auth.x.ai` and
`cli-chat-proxy.grok.com`; unsupported session-path features fail closed rather
than borrowing API-key behavior.

Two reconnaissance corrections supersede earlier table wording:

- Current xAI reasoning documentation lists Grok 4.5 as
  `low`/`medium`/`high`; sending `xhigh` is treated as `high`. Vesper must not
  advertise `xhigh` for that model unless newer first-party evidence changes
  the capability record.
- The dedicated multi-agent guide says the current multi-agent model does not
  support client-side/custom tools, despite a broader model-detail capability
  badge. Vesper therefore marks shared Vesper function tools unsupported for
  that model until xAI reconciles the contract and fixtures prove otherwise.

PR-1 must first close the generic gaps identified by PR-0 where its scope needs
them: registry-routed model catalogs, browser authentication initiation, and
removal of host-side provider-ID defaults. Hosted-tool/citation types are a
separate provider-neutral prerequisite for PR-5. The existing image,
structured-output, opaque continuation, device-login, stream, tool-loop, and
auxiliary compaction contracts are reusable.


## 40. PR-1 closeout — 2026-09-25

PR-1 is closed at offline fixture scope by
[`foundation/xai-provider-pr1-execution.md`](foundation/xai-provider-pr1-execution.md).
The new `vesper-provider-xai` crate owns API-key credential resolution, the
fixed public Responses endpoint, request serialization, bounded SSE decoding,
typed safe errors, cancellation and loopback fixtures. The red strict-function
schema and oversized-event settlement cases now pass; the complete targeted
suite is 11/11.

This does not make xAI user-selectable. The crate is not registered in TUI or
ACP, its static Grok 4.7 entry is fixture/configuration metadata rather than
account availability, and no live request was made. PR-2 through PR-8 remain
open under their original acceptance boundaries.


## 41. PR-2 closeout — 2026-09-25

PR-2 is closed at offline fixture scope by
[`foundation/xai-provider-pr2-execution.md`](foundation/xai-provider-pr2-execution.md).
Authenticated language-model discovery now intersects eight exact capability
records and current aliases; unknown, retired and endpoint-excluded identifiers
remain non-executable. Model-specific reasoning, multi-agent semantics, the US
endpoint set, image/function gates and the documented strict JSON Schema subset
are enforced. The targeted suite is 20/20. No host registration or live request
has occurred; PR-3 through PR-8 remain open.

## 42. PR-3 closeout — 2026-09-25

PR-3 is closed at offline loopback scope by
[`foundation/xai-provider-pr3-execution.md`](foundation/xai-provider-pr3-execution.md).
Vesper now owns fixed-origin browser OIDC and device authorization, PKCE/state/
nonce and signed-token validation, serialized credential refresh/logout,
first-party subscription-proxy routing and strict session/API billing isolation.
The targeted suite is 27/27. No host registration or live account request has
occurred; PR-4 through PR-8 remain open.

## 43. PR-4 closeout — 2026-09-25

PR-4 is closed at offline fixture scope by
[`foundation/xai-provider-pr4-execution.md`](foundation/xai-provider-pr4-execution.md).
The adapter now maps explicit stored Responses continuation, stable bounded
prompt-cache routing and citation annotations while retaining the existing
shared-tool call/result path, opaque encrypted reasoning and normalized usage.
The targeted all-feature suite is 29/29. No host registration or live request
has occurred; PR-5 through PR-8 remain open.

## 44. PR-5 closeout — 2026-09-25

PR-5 is closed at offline adapter scope by
[`foundation/xai-provider-pr5-execution.md`](foundation/xai-provider-pr5-execution.md).
A provider-neutral hosted-tool descriptor/selection contract now separates xAI
server execution from Vesper functions. Web Search, X Search, Code Execution,
attachment search, Collections Search and unauthenticated HTTPS Remote MCP map
only after explicit opt-in and only on the verified Global/API-key path.
Provider-hosted image generation is excluded until a provider-neutral generated
media output/asset port exists, as required by §22.2. Host controls remain PR-7;
PR-6 through PR-8 remain open.

## 45. PR-6 closeout — 2026-09-25

PR-6 is closed at offline transport and shared-policy scope by
[`foundation/xai-provider-pr6-execution.md`](foundation/xai-provider-pr6-execution.md).
The provider-neutral AgentLoop now has an explicit, default-off native
compaction policy and transactional opaque-prefix commit. xAI implements the
bounded Global/API-key compaction endpoint and an explicit WebSocket transport
whose pre-dispatch fallback cannot replay sent or visible work. The targeted
AgentLoop and xAI suites are green. Host controls, composition, live acceptance
and exact-commit release remain PR-7/PR-8 work.

## 46. PR-7 closeout — 2026-09-25

PR-7 is closed at offline host-composition scope by
[`foundation/xai-provider-pr7-execution.md`](foundation/xai-provider-pr7-execution.md).
Both production hosts register the native adapter, project explicit auth and
account-discovered model controls, preserve citations, and inherit the shared
tool/voice/skill/memory/VRO/worker paths. A provider-neutral bounded text
control closes the attachment/collection/Remote-MCP configuration gap; the xAI
adapter remains the sole owner of hosted-tool projection and validation. Live
Grok-account acceptance, optional paid API-key acceptance, exact-commit CI and
release remain PR-8 work.
