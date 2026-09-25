# VRO-18 PR-7 host composition execution

Status: **PASS — offline host composition scope**
Date: 2026-09-25

## Objective

Compose the native xAI adapter into TUI and ACP while preserving the shared
provider registry, AgentLoop, tool permissions, voice, skills, memory, VRO and
worker paths. Requirements: [`../vro18-native-xai-provider-prd.md`](../vro18-native-xai-provider-prd.md).

## Implementation

Both production hosts register `vesper-provider-xai`. Authentication is
descriptor-driven: xAI advertises browser and device-code flows for Grok
sessions and a separate API-key method; OpenAI continues to advertise only its
device flow. ACP exposes equivalent explicit CLI auth actions without writing
to protocol stdout. Neither host silently changes billing mode.

Authenticated discovery populates the model surfaces. TUI refreshes xAI models
after sign-in and when Settings opens; ACP uses its startup account snapshot.
Model, reasoning/multi-agent scale, region, HTTP/WebSocket, native-compaction,
Web Search, X Search and remote Code Execution choices reach the shared
`AgentLoopConfig`. Generic dynamic superpower aliases now appear in native
Settings and persist with the existing draft Save/Discard contract.

The provider-neutral superpower contract now includes a bounded 2,048-byte
free-text value with one shared parser and configuration projection. This
closes the prior structured-value gap without adding xAI controls to shared
runtime code. TUI Settings exposes attachment IDs/URLs, collection IDs/result
limit, and Remote MCP URL/label/description/tool allowlist. ACP exposes the
three enablement switches in its footer controls and resolves the same
provider-advertised bounded aliases as session commands. The xAI adapter's
`hosted_tool_selections` function owns projection; its wire layer rejects
missing, insecure, oversized, or malformed values before dispatch.

`AgentLoopConfig.hosted_tools` is a default-empty provider-neutral selection
list. The xAI host projection populates it; all local Vesper tools still come
from the unchanged shared registry. ACP's public boot dispatcher now accepts
`xai`, and both hosts derive context limits from verified xAI metadata.

Memory extraction uses the xAI session's bounded auxiliary port and therefore
the same selected authentication/billing mode. Voice, skills, VRO and workers
inherit xAI from the existing registry/AgentLoop composition. A source scan
found no `xai`/`xAI` branch in `vesper-agent`, `vesper-runtime`, `vesper-voice`
or `vesper-harness` production sources.

Provider citations previously survived in history but disappeared from both
host outputs. A shared safe renderer now emits only validated citation title
and URL metadata; encrypted reasoning, compaction and every other opaque item
remain hidden.

## Red-to-green evidence

- ACP `boot("xai")` was unreachable because the final public dispatcher omitted
  the token; the dispatcher now routes it to the registered adapter.
- The TUI authentication menu always called `device_login`; descriptors now
  advertise interactive login kinds and the host offers browser or device code
  without a provider-ID branch.
- Generic Settings exposed only legacy aliases; arbitrary advertised bounded
  choice controls now resolve, render and persist provider-neutrally.
- Citations were dropped by text-only final-output rendering. Shared citation
  projection tests prove citation visibility and encrypted-state secrecy.
- The first repository architecture run rejected both host dependencies on the
  new adapter because the explicit dependency allowlist still stopped at
  OpenAI. The allowlist now admits `vesper-provider-xai` only for the two
  composition hosts; the adapter's existing process-runtime/frontend bans are
  unchanged.
- The first structured hosted-tool projection regression failed because the
  helper emitted a new envelope namespace while xAI wire validation accepts
  the established versioned `provider.xai` namespace. Reusing the existing
  envelope contract flipped the full attachment/collection/Remote-MCP case
  green; an insecure Remote-MCP URL remains a red-path rejection.

Focused green receipts include xAI control projection in both hosts, dynamic
TUI/ACP command routing, descriptor login metadata, all six hosted selections,
generic hosted-selection delivery through AgentLoop, and safe citation
rendering. The affected all-feature package run passes 58 ACP library tests,
285 TUI library tests, 161 TUI binary tests, 19 provider-contract tests and 40
xAI adapter tests, plus the applicable integration and doc-test targets.

## Verification and environment

`cargo fmt --all -- --check` and `git diff --check` pass. The current candidate
passes the complete workspace all-feature test suite. The first ACP link attempt ended in a
linker `SIGBUS` after the 14 GiB temporary filesystem filled with mixed build
graphs; after a Cargo-native clean of only the isolated target, the unchanged
candidate rebuilt and all ACP tests passed with 11 GiB free. Earlier
all-feature host checks and focused provider/AgentLoop tests also passed during
implementation. `/tmp` is a
14 GiB quota filesystem; mixing default and all-feature artifact graphs filled
the isolated target twice. Only `/tmp/agent-vesper-vro18/target` and later the
dedicated `/tmp/agent-vesper-vro18-target` were cleaned with `cargo clean` after
process checks. Source, the original checkout, user state, models and installed
Agent Vesper were untouched.

The repository-owned architecture gate passes for 31 packages after the
allowlist repair, the naming guard passes with all 36 existing hits frozen, and
`cargo xtask acceptance` passes all 23 exact cases in 152,320 ms with zero
live-model cost. Strict workspace/all-target/all-feature Clippy initially
rejected one test-only field reassignment after `Default`; the test now uses a
direct struct initializer and the unchanged runtime candidate passes with
`-D warnings`.

After structured controls were added, strict Clippy also rejected the local
projection helper's large `ProviderError` result. The helper now returns a
small adapter-owned `HostedToolSettingsError`; full typed provider errors remain
at the wire/transport boundary. The repaired tree passes strict workspace
Clippy and the complete workspace all-feature test suite.

## Deviations and unresolved items

ACP's standard session-control protocol represents enumerated choices but has
no arbitrary bounded text field. ACP therefore uses provider-advertised
session commands for structured values while its footer controls enable each
remote capability. This is an explicit host-presentation difference; both
routes persist the same provider configuration and reach the same adapter
projection. Real-account authentication/inference, optional paid API-key
acceptance, exact-commit platform CI and release remain PR-8 gates.

## Readiness effect

PR-7 is closed at offline scope. The production composition and
provider-neutral inheritance path are ready for PR-8 live acceptance and
exact-commit release gates. VRO-18 is not production-complete or release-ready.
