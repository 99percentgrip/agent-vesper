# VRO-18 PR-0 reconnaissance execution

Status: **PASS — production implementation not started**
Date: 2026-09-25

## Objective

Reconcile the proposed native xAI provider with current Vesper contracts and
current first-party xAI evidence before any production edit. The owning design
record is [`../vro18-native-xai-provider-prd.md`](../vro18-native-xai-provider-prd.md);
the full technical mapping is
[`../architecture/recon_xai_native_provider.md`](../architecture/recon_xai_native_provider.md).

## Baseline and isolation

- Original checkout: `/home/Alex/Projects/agent-vesper`, branch `main`, HEAD
  `68aaa55216d5efc2a32e2d45661e333eca961c59`, with Alex's pre-existing tracked
  and untracked work left untouched.
- Reconnaissance worktree: `/tmp/agent-vesper-vro18`, branch `vro18/pr0`, exact
  baseline `c64e78f46065f1fbaae899ab9914f3d9b3f7023d` (`origin/main`).
- Input PRD: `/home/Alex/Downloads/VRO-18-native-xai-provider-prd.md`, SHA-256
  `d5b64e8e16122fe1cf479e85806e99aafa5bc0b706ecc305920843ef09df3836`.
- No production source, tests, credentials, provider state, installation,
  version, tag, or release was changed.

## Method and inspected material

The applicable root, documentation, architecture, foundation, crates,
provider, runtime, auth, concrete-adapter, TUI, and ACP `AGENTS.md` contracts
were read before edits.

Repository tracing used bounded `rg`, `sed`, and numbered source slices over:

- `crates/vesper-provider` request, stream, capability, credential, catalog,
  continuation, auxiliary and superpower contracts;
- `crates/vesper-runtime/src/registry.rs`;
- `crates/vesper-agent/src/agent_loop.rs` and compaction paths;
- OpenAI, GLM and LM Studio adapter composition;
- TUI/ACP provider registration, Settings/control, model/default/context
  projection, voice submission, VRO, skill and swarm composition.

Official xAI documentation was read on 2026-09-25 for Responses, model
discovery/details, reasoning, structured output, function and hosted tools,
citations, prompt caching, native compaction, WebSocket, regional endpoints,
release/retirement notices, and Grok Build authentication/enterprise behavior.

The first-party repository was cloned into temporary scratch and pinned:

```text
repository: https://github.com/xai-org/grok-build
commit: f0e3be1100ef5252488e3be8bb0e91cf68d8c305
commit date: 2026-09-23T16:52:41Z
subject: Synced from monorepo
license: Apache-2.0, Copyright 2023-2026 SpaceXAI
```

Inspected source areas included `xai-grok-login` configuration, OIDC protocol,
device authorization, credential refresh/manager code;
`xai-grok-env`; sampler headers; shell proxy headers, model source/parser, and
subscription check; plus the direct-proxy README contract. The scratch clone
was removed after evidence capture.

No live xAI request was made. Documentation/source evidence is not described as
real-account acceptance.

## Evidence and decisions

### Public API

**PASS for implementation readiness.** The API-key route has a current bearer
auth contract, Responses endpoint, catalog endpoints, SSE/event model,
structured output, image input, tools, usage, encrypted reasoning, cache key,
compaction endpoint, and WebSocket specification. Global and US endpoint
constraints are explicit enough for loopback fixtures and fail-closed catalog
intersection.

### Grok session

**PASS for fixture-first implementation readiness.** The pinned source proves:

- `auth.x.ai` public-client OAuth/OIDC with PKCE, state, nonce and ID-token
  validation;
- browser and RFC 8628 device flows;
- refresh-token rotation and bounded coordination;
- `cli-chat-proxy.grok.com/v1` session inference;
- bearer plus `X-XAI-Token-Auth: xai-grok-cli`, client identity/version/mode,
  model override and conversation routing headers;
- session-authenticated `/v1/models` discovery, independently selected from
  API-key discovery;
- a subscription-tier query, with no verified quantitative allowance surface.

This closes the PRD stop rule. It does not make every public Responses feature
available to session mode. Each session model/backend feature remains gated by
the proxy catalog, static evidence, and fixtures.

The later live PR-8 run made the abbreviated header description above
operationally precise. The proxy rejects inference unless Vesper also sends
the pinned Grok Build protocol's `x-authenticateresponse`,
`x-grok-client-version`, truthful client identifier/mode, and per-turn
conversation/request/session/agent correlation headers. That live red result
and the repaired regression are owned by the PR-8 execution report; they
supersede any reading that bearer, token-auth, and model override alone were a
complete request contract.

### Corrections to planning input

- Current reasoning documentation does not advertise distinct `xhigh` for Grok
  4.5; it says unsupported `xhigh` degrades to `high`. The UI must omit it.
- The dedicated multi-agent guide rejects client-side/custom tools even though
  a broad model detail badge says function calling. Vesper tools fail closed
  for that model until xAI reconciles the contract and fixtures prove support.

These corrections are recorded in the PRD instead of silently preserving stale
claims.

### Vesper abstraction result

Reusable without xAI branches:

- `ProviderFactory`, `ProviderSession`, `ProviderRequest`, ordered stream
  events, tool definitions/call IDs/results, structured output, image content,
  opaque provider data/reasoning, native continuation, cancellation, normalized
  usage, superpowers/policy, device login/logout, and auxiliary compaction;
- generic `ProviderRegistry` dispatch and the shared `AgentLoop` tool path;
- provider inheritance by voice, skills, VRO, memory and workers.

Shared gaps to close in the phase that needs them:

1. register and query `ModelCatalog` through `ProviderRegistry`;
2. add generic browser authentication and explicit auth-method selection;
3. replace host provider-ID match arms for defaults/model/context with
   descriptor/catalog-driven composition;
4. add typed provider-hosted tool descriptors/consent and citation output
   before PR-5.

The existing compaction/opaque extension boundaries appear sufficient for a
prototype, but PR-6 must prove transactional native-compaction rollback before
freezing that judgment.

## Files

Created:

- `docs/vro18-native-xai-provider-prd.md`
- `docs/architecture/recon_xai_native_provider.md`
- `docs/foundation/xai-provider-recon-execution.md`

Updated:

- `docs/AGENTS.md`
- `docs/architecture/AGENTS.md`
- `docs/foundation/AGENTS.md`
- `docs/foundation/evidence-index.md`

The DOX updates register the new ownership and the user-authorized named-source
deviation. No source/test file changed.

## Verification

The documentation-only closeout ran:

- repository-relative Markdown link existence checks for changed documents;
- cited workspace-path existence checks;
- first-party commit/license verification;
- whitespace/trailing-space review;
- `git diff --check`;
- changed-file scope and original-worktree invariance checks.

Results:

- changed-document relative links: 0 missing;
- changed-document trailing whitespace: 0 lines;
- all three new records: final newline present;
- `git diff --check`: PASS;
- isolated worktree scope: eight documentation files only;
- original checkout: HEAD remains `68aaa55216d5efc2a32e2d45661e333eca961c59`
  with the same 81 pre-existing status entries and no VRO-18 file;
- upstream scratch clone: absent after evidence capture.

Program, provider, and release suites were not run because PR-0 changes only
documentation and performs no implementation. No live foundation call was
made, consistent with repository policy.

## Deviations and unresolved items

- The architecture subtree normally uses aliases and avoids production-source
  ingestion. Alex's VRO-18 directive explicitly requires xAI names, official
  URLs, a pinned Grok Build repository, and exact auth/proxy symbols. The narrow
  exception is recorded in the owning DOX and does not generalize.
- Session-path structured output, image input, encrypted reasoning, native
  compaction and WebSocket remain unproven where the proxy catalog/source does
  not establish them for a selected model/backend. They are implementation
  gates, not inferred parity.
- Quantitative Grok subscription allowance is unavailable through the verified
  source surface; only subscription tier is established.
- No real-account acceptance occurred. PR-8 remains responsible for separately
  authorized live session/API-key evidence.

## Readiness effect

PR-0 is complete. PR-1 may begin from the exact baseline after review. VRO-18
remains **OPEN**; no native xAI provider is advertised or production-ready.
