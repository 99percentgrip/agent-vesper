# Native OpenAI provider

Status: native adapter and both-host integration implemented for v0.21.0.
Publication remains exact-commit gated by canonical, MSRV, five-target
foundation, and contained web-driver workflows. Live account entitlement is
not asserted by offline acceptance. User setup is in [the guide](openai-provider.md).

## Required outcome

- One `openai` provider, with API-key and ChatGPT subscription sign-in choices.
- Settings → Providers → OpenAI owns normal setup, authentication selection,
  model selection, and supported reasoning controls. No manual file editing
  or copying authentication tokens as the normal workflow.
- Both TUI and ACP retain shared tools, network/browser tools, permission
  gates, memory, skills, plans, checkpoints under each host's existing policy,
  compaction, MCP, workers, and reasoning orchestration.
- Preserve GLM and LM Studio behavior. OpenAI-specific authentication,
  model metadata, request serialization, and stream decoding belong to a
  real adapter, not the GLM transport or provider-neutral runtime.
- Codex parity must be evaluated capability by capability. Model access
  alone is not proof of parity with every Codex application feature.

## Evidence and architecture decision

- [v0.24.4 combined corrective release](foundation/2026-09-29-v0.24.4-combined-corrective-release.md) tracks the exact release source, gates, publication, assets, Registry update, manual acceptance, and retained historical-event limitation.
- [2026-09-28 malformed-Responses and shared skill-routing repair](foundation/2026-09-28-openai-skill-routing-repair.md) records the current subscription metadata/reasoning decoder fix, bounded secret-safe rejection diagnostics, both-host offline receipts, runnable candidate identities and the unexecuted live/cross-platform gaps.
- [2026-09-30 Responses decoder reinspection](foundation/2026-09-30-openai-responses-reinspection.md) rechecks the historical and current first-party event shapes, preserves the already-correct acceptance of subscription metadata/reasoning events, and adds expected-shape detail to separate structured diagnostics.
- [2026-09-30 Responses streaming boundary audit](foundation/2026-09-30-openai-responses-streaming-boundary-audit.md) classifies the recorded failure as a bounded JSON event with an unsupported string event type while preserving the unknown exact discriminant, adds framing/boundary/no-replay regressions, labels diagnostic byte measurements, and enforces safe user prose versus separate structured diagnostics in both hosts.
- [2026-09-30 Responses live prerelease candidate](foundation/2026-09-30-openai-responses-live-prerelease-candidate.md) records the clean isolated debug source/binary identity, complete 60-test adapter receipt, TUI secret-canary surface proof and one successful direct OpenAI runtime turn. It does not reproduce the unavailable historical event or establish release/cross-platform readiness.
- [2026-10-06 catalog audit and GPT-6.1 Sol implementation](foundation/2026-10-06-openai-catalog-audit-and-gpt-6-1-sol.md) records current primary-source metadata, the complete stale/missing-row audit, native catalog/discovery/Responses changes, both-host hermetic evidence, and the no-release/no-live-account boundary.
- [2026-10-06 independent prerelease review and repair](foundation/2026-10-06-openai-catalog-prerelease-review-and-repair.md) records the separate ACP review and limitations, red-first correction of subscription-refresh API-key loss and stale out-of-order discovery publication, the disproved logout concern, current acceptance/canonical/MSRV receipts, and pending exact-version hosted gates.

Official documentation inspected on 2026-09-08:

- [Authentication](https://learn.chatgpt.com/docs/auth): Codex distinguishes
  ChatGPT subscription sign-in from usage-based API-key sign-in.
- [App server](https://learn.chatgpt.com/docs/app-server): the embedding
  protocol exposes authentication, streaming, and approvals. Managed ChatGPT
  authentication owns browser/device login, credential persistence, and
  refresh. Dynamic tools and externally managed tokens have experimental
  contracts; they must not be treated as stable native-provider interfaces.
- [Responses migration](https://developers.openai.com/api/docs/guides/migrate-to-responses):
  public API integration must use the actual Responses request/output shape,
  not rename a Chat Completions adapter.

The documentation reviewed does not establish a standalone third-party
ChatGPT-subscription inference/OAuth contract independent of the Codex runtime.
This is a documentation gap, not a finding that native subscription support
is impossible. Local third-party implementations are not authoritative proof
of client registration, endpoint stability, or entitlement.

User decision: both modes must have no Codex runtime dependency. Do not
install, bundle, or launch Codex CLI or app-server. Implement authentication
and transport directly in the Rust adapter, retaining Vesper's agent loop,
tools, permissions, and credential ownership. Verify the upstream native
authentication/transport contract and permitted client identity before wiring
production sign-in. Do not substitute a ChatGPT token for an API key or
silently fall back from subscription mode to API billing.

The frozen GLM oracle was not available at either documented case variant of
its filesystem path during this inspection. No exclusion or parity claim is
based on an assumed oracle implementation.

Native protocol implementation evidence: OpenAI's source repository inspected
at commit `8e694e955ae02ca737230a5468c55d5847074072`, specifically
`codex-rs/login/src/device_code_auth.rs`, `login/src/server.rs`, and
`login/src/auth/manager.rs`. Device authorization uses the accounts deviceauth
usercode/token routes, followed by a PKCE authorization-code exchange. Refresh
uses the OAuth token endpoint. Source inspection required no Codex installation
or execution and did not access existing credentials. This is implementation
evidence, not proof of a stable third-party API or live account entitlement.

## Implementation and acceptance gates

1. Enforce the native-only authentication architecture and adapter boundary.
2. Implement bounded, secret-safe authentication, cancellation, expiration,
   refresh/logout, and isolated credential storage. Do not read unrelated
   Codex credentials or start a real login during tests.
3. Implement an adapter-owned, evidence-backed catalog for each authentication
   mode. Validate model ownership, reasoning values, modalities, and limits;
   unknown metadata fails closed. Never infer capabilities from model names.
4. Implement Responses serialization, multimodal inputs, tool-call/result
   round trips, structured output where supported, usage, errors, and ordered
   streaming. Preserve opaque reasoning continuation without exposing it as
   user-visible reasoning. Ambiguous tool fragments must never be replayed.
5. Wire both hosts, native Settings, authentication UX, capability gates,
   shared harness services, worker routing, and compaction model limits.
6. Add offline and loopback fixtures for both modes: authentication failure,
   refresh, cancellation, fragmented SSE, interrupted tools, strict schemas,
   image inputs, usage, and permission denial. Add cross-host integration
   evidence proving actual tool execution rather than schema advertisement.
7. Run canonical verification, MSRV, and platform gates before claiming the
   provider is available. Any live account acceptance is a separate explicit
   operation, not foundation verification. Record remaining Codex capability
   gaps honestly; do not label partial implementation full parity.

## Implemented acceptance evidence

- Native-only boundary: `vesper-provider-openai` owns direct HTTP OAuth and
  Responses; architecture checks prohibit agent subprocess dependencies.
- Authentication: fixed TLS origins, redirect refusal, bounded device polling,
  PKCE exchange, one-shot refresh, cancellation, secure selected-mode records,
  local logout, redacted secret wrappers, and cross-process credential locks.
  Subscription login and refresh preserve a separately stored valid API key while
  retaining subscription as the selected mode; explicit logout replaces the whole
  stored record. Auth fixtures cover pending/approved/expired/cancelled/oversized/invalid
  responses and refresh rotation. Storage tests use temporary private vaults.
- Catalog: GPT-6 Astra, GPT-6.1 Sol, GPT-6 Sol/Luna, GPT-5.6 Sol/Terra/Luna,
  GPT-5.5, GPT-5.4, GPT-5.2, GPT-5.3 Codex, and GPT-5.3 Codex Spark;
  per-model/auth-mode effort choices and explicit capability controls. GPT-6.1
  Sol supports low through max but rejects none/minimal; GPT-6 Sol/Luna allow
  API-only none. Codex-host `ultra` remains excluded. Spark is text-only with a
  128K context budget and no reasoning-summary parameter; other entries use a
  conservative 272K.
  Native authenticated discovery intersects API model identifiers or visible subscription
  entries with this capability index. Both hosts filter choices and the shared factory
  blocks dispatch outside its account snapshot. Failed refresh clears old choices.
  Concurrent discovery is generation-ordered, so an older completion cannot replace
  the latest-started account snapshot; credential mutations supersede in-flight rows.
  Public model documentation and the pinned upstream catalog are evidence;
  model identifiers alone never establish capabilities or account entitlement.
- Usage: `ProviderSession::query_usage` and the shared bordered renderer replace
  both GLM-only host branches. Native OpenAI subscription GET
  `/backend-api/wham/usage` preserves primary, secondary, and additional windows,
  percentages, and reset timestamps. API mode explicitly distinguishes API
  billing from subscription quotas; missing windows remain unknown. The real
  ACP process fixture verifies a quota-only turn, account header, and 57%/63%
  remaining presentation without inference.
- Transport: native function schemas, linked calls/results, image inputs,
  structured output, usage, encrypted reasoning continuation, fragmented UTF-8
  SSE, typed interruptions, cancellation, and bounded buffers/deadlines.
  No ambiguous stream is replayed as a tool execution.
- Shared harness: the agent loop now retains assistant calls and typed results
  with valid IDs. Interruptions return before executing collected calls. The
  same permission/worker/skill/compaction paths serve both hosts and both modes.
- Hosts: native Settings auth choice, masked API-key entry, device-code flow,
  cancellation/logout, real model/effort execution controls, ACP provider/model
  controls and native login CLI. Both hosts use native OpenAI memory extraction;
  embeddings remain independently configured rather than requiring Z.ai.
- `apps/agent-vesper-acp/tests/openai_native.rs` runs the real ACP binary and
  confined read executor, then checks the next Responses request contains the
  original call ID and actual file contents in both billing modes. TUI tests
  verify real registry registration and selected-model/effort agent configuration.
  ACP process fixtures also switch away/back, change model/effort, and verify
  read-only permission denial. Provider-owned controls refresh per session;
  reasoning selections update real execution rather than only runtime display.
- Adapter tests cover native memory extraction in both modes, unsupported
  required capabilities, malformed-after-visible streams, incomplete tools,
  usage, image/JSON serialization, authentication, and storage/lock behavior.
- Verification commands: `cargo xtask verify`; `cargo +1.88.0 test --workspace
  --all-features --locked`; strict workspace Clippy; supply-chain checks; the
  four exact-commit CI workflows before tagging. Release evidence is the
  immutable tag's successful GitHub workflow runs, not this checklist alone.

## Explicit deployment boundaries

No live account authentication/inference was performed by foundation tests.
The inspected subscription client/endpoint protocol is not a promise of stable
third-party registration or account entitlement. Failure never enables API
billing. Subscription visible-byte output bounds are not server-side reasoning
token/billing limits. The static capability index is a verified subset, not a claim
of access to every Codex model. Account discovery is a snapshot, refreshed on TUI
Settings entry/reauthentication and ACP startup; service authorization can change
later. Unknown discovered identifiers remain excluded until adapter support is verified.
No live account discovery or inference is part of foundation verification.

Memory extraction follows the host's launch provider; ACP footer switching
does not replace its already-open memory extractor. Restart with OpenAI to use
native extraction. Legacy Z.ai MCP services retain their own credentials;
native Vesper web tools remain provider-independent and opt-in.

[The 2026-09-28 Z.ai MCP web-tools reconnaissance](foundation/2026-09-28-zai-mcp-web-tools-reconnaissance.md)
traces the original exposure and defects. The subsequent
[repair](foundation/2026-09-28-zai-mcp-web-tools-repair.md) scopes the protected
Z.ai wrappers to Z.ai reasoning turns, bridges the existing Z.ai credential
source, and leaves native Vesper web tools provider-independent and opt-in. The
[finishing verification](foundation/2026-09-28-zai-mcp-web-tools-finishing.md)
records final process isolation, diagnostics, and runnable candidates. It does
not add OpenAI-hosted search or change this provider's credential path.

Codex cloud tasks, hosted computer use, audio, and application-specific UX are
not parity claims. Native Vesper file/shell/browser/memory/skill/worker features
remain available under their existing permissions and host-specific policies.

## Model and usage follow-up evidence (2026-09-09)

Public model pages were fetched for [Sol](https://developers.openai.com/api/docs/models/gpt-5.6-sol),
[Terra](https://developers.openai.com/api/docs/models/gpt-5.6-terra),
[Luna](https://developers.openai.com/api/docs/models/gpt-5.6-luna),
[GPT-5.5](https://developers.openai.com/api/docs/models/gpt-5.5),
[GPT-5.4](https://developers.openai.com/api/docs/models/gpt-5.4),
[GPT-5.2](https://developers.openai.com/api/docs/models/gpt-5.2), and
[GPT-5.3 Codex](https://developers.openai.com/api/docs/models/gpt-5.3-codex).
The same pinned upstream source's `models-manager/models.json`,
`core/src/client_tests.rs::reasoning_effort_for_requests_uses_multi_agent_override_for_ultra`,
and `backend-client/src/client/rate_limit_resets.rs` establish subscription
efforts, the host-owned ultra mapping, and the passive usage path. Ultra is
automatic task delegation, not an extra HTTP effort accepted verbatim.
Vesper does not advertise it as an alias for max.

Local follow-up verification: `cargo xtask verify` passed (full workspace,
strict Clippy, architecture and fixture gates), as did Rust 1.88 locked tests
for `vesper-provider`, `vesper-provider-openai`, and `vesper-provider-glm`.
The catalog and initial provider-neutral status work shipped in v0.21.1.
The v0.21.2 follow-up removes the ASCII-art status frame, aligns and compacts
usage fields, uses solid allowance meters and human-scale reset intervals,
and reuses valid stored OpenAI authentication across provider switches.
Publication remains exact-commit gated by canonical, MSRV, five-target
foundation, and web-driver acceptance workflows before tagging.

## Account model discovery evidence

The public [Models list API](https://developers.openai.com/api/reference/resources/models/methods/list)
returns available API identifiers; it does not establish tool, vision or reasoning
capabilities. Subscription request shape follows the pinned OpenAI Codex source's
[`codex-api/src/endpoint/models.rs`](https://github.com/openai/codex/blob/d63a9b8344cfe58bc78bbe319b560378fc8756ef/codex-rs/codex-api/src/endpoint/models.rs)
(`models` plus `client_version`). Vesper identifies itself in User-Agent and uses
only its own credentials. The query's `client_version=0.155.0` is a separate
protocol compatibility floor, matching GPT-6 Sol/Luna's `minimal_client_version`
in the pinned
[`models.json`](https://github.com/openai/codex/blob/d63a9b8344cfe58bc78bbe319b560378fc8756ef/codex-rs/models-manager/models.json).
It must not follow Vesper's independent release number. Subscription
picker visibility comes from the returned rows, not a copied screenshot.
[OpenAI's Spark announcement](https://openai.com/index/introducing-gpt-5-3-codex-spark/)
documents text-only input and the 128K context window. Native wire and real ACP
loopback tests prove Spark tool transactions and summary omission; they do not
prove a particular account's entitlement or live service availability.

Verification lives in adapter `discovery.rs`/`tests.rs`, TUI account-choice/context
unit tests, and ACP `openai_native`/`openai_rejection` process fixtures. All use
synthetic credentials and isolated storage; failed, malformed, oversized, redirected,
cancelled and stalled discovery never restores the static menu.

The empty-picker regression was reproduced with a separate native, read-only account
diagnostic on 2026-09-11: Vesper's 0.21.8 query version yielded no usable models;
changing only that query to 0.153.0 returned Astra, Sol, Terra, Luna, GPT-5.5 and
Codex Spark. No inference request was made. This is evidence for that account at
that time, not a universal entitlement claim or a foundation test. The loopback
request assertion now pins the protocol version; TUI tests cover visible failure
reasons, retry keyboard/mouse actions and empty-to-populated menu state.
The rebuilt TUI was also exercised in a temporary-workspace PTY: native account
discovery displayed those six choices, selecting a model stayed in Settings, and
Esc returned to the landing page. A separate network-isolated PTY used a synthetic
credential vault to verify visible connection failure, retry and back navigation.
Neither walkthrough submitted a prompt. Workspace verification passed 2,129 tests
with zero failures and 34 explicitly ignored tests; Clippy and the 27-package
architecture check passed. This does not certify unexecuted release/platform gates.

## Catalog audit and GPT-6.1 Sol evidence (2026-10-06)

Current official model pages establish that
[GPT-6.1 Sol](https://developers.openai.com/api/docs/models/gpt-6.1-sol)
accepts text and image, returns text, advertises a 1,050,000-token context window
and 128,000 maximum output, and accepts exactly low, medium, high, xhigh, and max
reasoning effort. None and minimal are explicitly unsupported; Responses is required
for tool calling. The [GPT-6 Sol](https://developers.openai.com/api/docs/models/gpt-6-sol)
and [GPT-6 Luna](https://developers.openai.com/api/docs/models/gpt-6-luna) pages
establish the same modalities and public limits, with API none also supported.
Vesper continues to use the pinned Codex catalog's conservative 272,000-token
operational input budget rather than advertise the larger public capacity as its
working budget.

The OpenAI Codex catalog at commit
[`d63a9b8344cfe58bc78bbe319b560378fc8756ef`](https://github.com/openai/codex/blob/d63a9b8344cfe58bc78bbe319b560378fc8756ef/codex-rs/models-manager/models.json)
(SHA-256 `943ca7fe1d19ed019054158303f0dbbd80b3bef43b3aa425a71aa7cb3fb52c2b`)
lists Astra, GPT-6.1 Sol, GPT-6 Sol/Luna, GPT-5.6 Sol/Terra/Luna and GPT-5.5 as
visible API-supported rows. GPT-6.1 Sol and Astra require catalog protocol 0.153.0;
GPT-6 Sol/Luna require 0.155.0, which is now the discovery floor. Its `ultra` row
remains excluded because it is a Codex-host delegation mode, not a literal Vesper
Responses effort.

The audit removed no existing entry. GPT-5.3 Codex is deprecated with an announced
April 1, 2027 shutdown, not yet removed; other older verified entries have no
applicable base-model shutdown in the current
[deprecations table](https://platform.openai.com/docs/deprecations). Preserving them
also preserves saved selections and API accounts that still return them. Authenticated
discovery remains the availability authority, so absent/hidden rows never appear or
dispatch merely because they remain in the capability index. The linked execution
report records hermetic implementation evidence; no live account call, user-state
write, installation, version change, push, tag or release was performed. The later
[independent prerelease review](foundation/2026-10-06-openai-catalog-prerelease-review-and-repair.md)
found and repaired two state-integrity blockers outside the catalog metadata itself:
subscription refresh now retains another valid stored API key, and only the latest-
started discovery may publish shared account availability. Its 64-test adapter result
is joined by current both-host focused, 100-case acceptance, canonical and Rust 1.88
MSRV receipts; exact-version hosted release gates remain separate.

## Device sign-in UI repair evidence (2026-10-05)

[Execution report](foundation/release-recovery-controller-deep-debug-execution.md#openai-sign-in-follow-up)
records the confirmed TUI URL-discard defect, native browser launch and explicit
verification-link/code fallback, retry/copy controls and credential-preserving
cancellation. Twelve authentication UI tests, all 300 TUI library tests and
48 native adapter tests passed offline. Live account/browser completion and
cross-platform UI observation remain unexecuted; RRC's five-target candidate
predates this additional UI repair. The adapter still uses its native credential
port and no Codex runtime or credential files.

## RRC initial-request recovery evidence

The [RRC autonomy audit](foundation/2026-10-06-rrc-autonomy-and-prd-audit.md)
records the stopped worker request and verified adapter-owned HTTP 500/503 retry
metadata. Shared repair code honors that metadata before any output/tools, permits
one bounded retry and retains typed failure diagnostics. Both authentication modes
retain Never for authentication/payment/quota rejections. These offline receipts
do not establish the HTTP status of the earlier live error or a vendor outage.
