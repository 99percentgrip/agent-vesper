# VRO-18 PR-7 host composition execution

Status: **PARTIAL — core host composition passes; structured hosted-tool configuration remains open**
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

Focused green receipts include xAI control projection in both hosts, dynamic
TUI command routing, descriptor login metadata, generic hosted-selection
delivery through AgentLoop, and safe citation rendering.

## Verification and environment

`cargo fmt --all -- --check` and `git diff --check` pass. The current candidate
also passes 27 xAI-adapter tests, 487 shared-agent tests across its test
targets, 412 TUI tests and 57 ACP tests. The first ACP link attempt ended in a
linker `SIGBUS` after the 14 GiB temporary filesystem filled with mixed build
graphs; after a Cargo-native clean of only the isolated target, the unchanged
candidate rebuilt and all 57 ACP tests passed with 11 GiB free. Earlier
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

## Deviations and unresolved items

The generic hosted-tool contract and xAI adapter support attachment search,
collections search and Remote MCP configurations, but neither host yet has a
generic native structured-value editor for their IDs/URLs. They remain
unselectable rather than accepting hand-edited or unsafe values. This keeps
HOSTED TOOLS and PR-7 open. Real-account authentication/inference, optional
paid API-key acceptance, exact-commit platform CI and release are PR-8 gates.

## Readiness effect

The production composition and provider-neutral inheritance path are ready for
the remaining structured hosted-tool UI repair and broad offline gates. VRO-18
is not production-complete or release-ready.
