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
  Auth fixtures cover pending/approved/expired/cancelled/oversized/invalid
  responses and refresh rotation. Storage tests use temporary private vaults.
- Catalog: GPT-5.4 and GPT-6 Astra, supported effort subsets, tool/vision/JSON
  capabilities, conservative 272K context, and explicit unsupported controls.
  Public model documentation and the pinned upstream catalog are evidence;
  model identifiers alone never establish capabilities or account entitlement.
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
token/billing limits. The static catalog is a supported subset, not account
discovery or a claim of access to every Codex model.

Memory extraction follows the host's launch provider; ACP footer switching
does not replace its already-open memory extractor. Restart with OpenAI to use
native extraction. Legacy Z.ai MCP services retain their own credentials;
native Vesper web tools remain provider-independent and opt-in.

Codex cloud tasks, hosted computer use, audio, and application-specific UX are
not parity claims. Native Vesper file/shell/browser/memory/skill/worker features
remain available under their existing permissions and host-specific policies.
