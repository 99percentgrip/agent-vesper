# VRO-18 PR-5 provider-hosted tools execution

Status: **PASS at offline adapter scope — host opt-in UI remains PR-7**  
Date: 2026-09-25

## Objective

Represent xAI server-executed tools without conflating them with Vesper's
client-side tools, then map the applicable current Responses API tools with
explicit egress, billing and configuration boundaries.

Requirements: [`../vro18-native-xai-provider-prd.md`](../vro18-native-xai-provider-prd.md).  
Predecessor: [`xai-provider-pr4-execution.md`](xai-provider-pr4-execution.md).

## Shared contract repair

Reconnaissance confirmed that `ProviderRequest.tools` is exclusively the local
Vesper function registry and the existing `vesper-agent::HostedTool` is a
Vesper-side executor. Neither can truthfully represent execution on provider
infrastructure. The provider-neutral boundary now defines:

- `HostedToolDescriptor`, including stable identity, safe description, egress
  class, separate-billing signal and non-secret configuration schema; and
- `HostedToolSelection`, an explicit per-request opt-in with a bounded,
  versioned provider-owned configuration envelope.

All existing request and descriptor construction sites were migrated with an
empty default. OpenAI, GLM and both LM Studio host adapters reject non-empty
hosted-tool selections instead of silently dropping them.

## xAI mapping

The xAI descriptor advertises six distinct, default-off tools:

| Vesper ID | Responses mapping | Egress/billing treatment |
| --- | --- | --- |
| `web-search` | `web_search` | xAI remote search; separately billed |
| `x-search` | `x_search` | xAI/X remote search; separately billed |
| `code-execution` | `code_interpreter` | xAI remote execution; separately billed |
| `attachment-search` | explicit `input_file` references; provider activates search | xAI file processing; separately billed |
| `collections-search` | `file_search` | xAI stored collections; separately billed |
| `remote-mcp` | `mcp` | xAI connects to explicit HTTPS endpoint; token billing only |

Attachment references, collection IDs/result limits and Remote MCP URL/label/
allowed-tool lists are bounded. Remote MCP rejects non-HTTPS URLs, embedded URL
credentials, secret/header fields and unknown configuration. Vesper MCP never
enables xAI Remote MCP, and xAI code execution never maps to `run_command`.

Hosted tools are currently allowed only in Global API-key mode. Current
first-party evidence does not establish the same contract for the Grok session
proxy or US regional endpoint, so those intersections fail closed.

Completed server-tool items and citations are retained as bounded
provider-owned transcript content. They are not converted into executable local
tool calls or fake Vesper web events.

Provider-hosted image generation is deliberately not advertised: Vesper has no
provider-neutral generated-media output/asset port. This follows the PRD's
adjacent-media exclusion rather than hardcoding xAI Imagine into provider or
host UI. It remains a separately gated provider-neutral media initiative.

## Red-to-green evidence

`hosted_tools_are_explicit_and_distinct_from_vesper_functions` was added first
and failed with an empty descriptor. The green suite additionally proves exact
wire mapping, local-function coexistence, attachment/collection/MCP validation,
duplicate/unsafe configuration rejection, result/citation preservation and
session-mode denial before network dispatch.

## Verification

```text
cargo check --workspace --all-features
cargo test -p vesper-provider -p vesper-provider-xai --all-features
cargo clippy -p vesper-provider -p vesper-provider-xai \
  -p vesper-provider-openai -p vesper-provider-glm \
  --all-targets --all-features -- -D warnings
cargo xtask architecture
cargo xtask naming-guard
cargo fmt --all -- --check
git diff --check
```

Results: workspace all-feature compile PASS; provider **21/21** and xAI
**33/33** PASS; strict Clippy, architecture (31 packages), naming guard,
formatting and whitespace PASS. The first broad check exhausted the `/tmp`
quota because two isolated build trees occupied 8.6 GB; only the obsolete
`/tmp/agent-vesper-vro18/target` was removed with `cargo clean` (4.7 GiB), then
the exact check passed. No source, installed binary, model or user state was
touched.

## Deviations and unresolved items

Host rendering/persistence of explicit selections is PR-7. Authenticated Remote
MCP is not accepted because the generic selection envelope is intentionally
non-secret; a future credential-reference contract is required before such
tokens or headers can be configured. Provider-native compaction and WebSocket
remain PR-6. No live hosted tool or charge was incurred.

## Readiness effect

PR-5 is closed at offline adapter scope. The generic distinction between local
Vesper tools and provider-hosted execution is now enforceable. The xAI adapter
remains unregistered and unadvertised; PR-6 through PR-8 remain open.
