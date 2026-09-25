# VRO-18 PR-1 native xAI API-key transport execution

Status: **PASS at PR-1 fixture scope — not registered or advertised**  
Date: 2026-09-25

## Objective

Create the xAI-owned provider leaf and prove the public API-key Responses
boundary offline before host composition. The owning requirements are
[`../vro18-native-xai-provider-prd.md`](../vro18-native-xai-provider-prd.md),
and PR-0 evidence is
[`xai-provider-recon-execution.md`](xai-provider-recon-execution.md).

## Baseline and isolation

- Isolated worktree: `/tmp/agent-vesper-vro18`, branch `vro18/pr0`.
- Baseline: `c64e78f46065f1fbaae899ab9914f3d9b3f7023d` plus PR-0 commit
  `6f721a87cea9eb6735b0632799e6e1a403c72cd4`.
- Build output: `/tmp/agent-vesper-vro18-target`, outside the repository.
- Original checkout, installed Agent Vesper, credentials, provider accounts,
  version, tag and release state were not changed. No live provider request ran.

## Implementation

`vesper-provider-xai` is a separate production crate. It owns:

- stable provider identity `xai`, display name `xAI / Grok`, and the explicit
  `xai-api-key` authentication descriptor;
- Vesper-owned secure API-key persistence plus `XAI_API_KEY` headless scope,
  without reading Grok Build or browser state;
- fixed redirect-free `https://api.x.ai/v1/responses` production dispatch and
  a feature-gated loopback-only test route;
- xAI Responses request serialization for text, image references, reasoning
  effort, strict custom functions, structured output, encrypted-reasoning
  inclusion, output limits and shared tool-choice intents;
- bounded SSE accumulation, fragmented UTF-8 handling, function-call identity,
  opaque encrypted reasoning, normalized usage, truthful interruption and one
  terminal outcome;
- bounded safe HTTP rejection classification that never returns provider prose;
- an explicit unavailable-account-balance notice for API-key status rather than
  an invented quota.

The crate is not in either host and is not selectable. Dynamic model discovery,
full capability catalog coverage, Grok session authentication, hosted tools,
native compaction, WebSocket, and host composition remain later PRs.

## Red to green evidence

The first compiling adapter-boundary run intentionally retained the copied
non-strict function setting. Command:

```text
CARGO_TARGET_DIR=/tmp/agent-vesper-vro18-target \
  cargo test -p vesper-provider-xai --all-features
```

Red result: **8 passed, 1 failed**. The exact failure was
`responses_request_uses_xai_strict_function_schema`: serialized
`tools[0].strict` was `false`, expected `true`.

The xAI serializer was corrected to emit strict function definitions. A later
new oversized-SSE regression initially failed because the byte cap classified
the condition as a generic transport interruption. The decoder was corrected
to settle oversized/invalid-UTF-8 events as `ProtocolError`.

Final targeted result: **11 passed, 0 failed**. Coverage includes the full nine
shared tool definitions in registry order, tool-call correlation, incomplete
call fail-closed behavior, opaque reasoning round trip, cached/reasoning usage,
fragmented UTF-8/SSE, cancellation after response headers, oversized event
settlement, visible-output EOF truthfulness, and isolated credentials.

## Files

Created:

- `crates/vesper-provider-xai/AGENTS.md`
- `crates/vesper-provider-xai/Cargo.toml`
- `crates/vesper-provider-xai/src/{lib,catalog,credentials,factory,http_error,transport,wire,tests}.rs`
- `docs/foundation/xai-provider-pr1-execution.md`

Updated:

- `Cargo.toml`, `Cargo.lock`
- `crates/AGENTS.md`
- `xtask/src/main.rs`, `xtask/AGENTS.md`
- `docs/AGENTS.md`, `docs/foundation/AGENTS.md`
- `docs/foundation/evidence-index.md`
- `docs/migration-status.md`
- `docs/vro18-native-xai-provider-prd.md`

## Verification

```text
cargo fmt --all --check
CARGO_TARGET_DIR=/tmp/agent-vesper-vro18-target cargo test -p vesper-provider-xai --all-features
CARGO_TARGET_DIR=/tmp/agent-vesper-vro18-target cargo clippy -p vesper-provider-xai --all-targets --all-features -- -D warnings
CARGO_TARGET_DIR=/tmp/agent-vesper-vro18-target cargo xtask architecture
CARGO_TARGET_DIR=/tmp/agent-vesper-vro18-target cargo xtask naming-guard
git diff --check
```

Results: format PASS; xAI tests **11/11 PASS**; strict Clippy PASS;
architecture PASS for 31 packages; naming guard PASS (36 frozen hits, no new
violation); whitespace PASS.

## Deviations and unresolved items

PR-0 identified registry-routed catalogs, browser authentication, and removal of
host provider-ID defaults as generic prerequisites for production selection.
PR-1 does not yet compose the provider into a registry or host, so those gaps do
not need an xAI-specific workaround here and remain explicitly assigned to the
phases that introduce discovery/session auth/host selection.

PR-1's one-model static record is configuration/fixture evidence only, not
account availability. PR-2 must replace selectability with authenticated
discovery intersected against the complete evidence-backed catalog. PR-3 owns
browser/device session auth and strict billing-path isolation. PR-4 through PR-8
remain open. No live xAI acceptance has occurred.

## Readiness effect

PR-1 is closed at offline fixture scope. PR-2 may implement authenticated model
discovery, endpoint-aware capability intersection, native reasoning metadata,
structured-output schema validation and model-gated image input. VRO-18 remains
open and xAI remains unavailable in production hosts.
