# VRO-18 PR-2 catalog, reasoning and structured-output execution

Status: **PASS at offline fixture scope — not registered or advertised**  
Date: 2026-09-25

## Objective

Replace PR-1's one-model fixture catalog with authenticated API availability
intersected against an evidence-backed capability index. Add model-specific
reasoning, endpoint constraints, alias handling, structured-output validation,
and model-gated image/tool behavior without host-specific branches.

Requirements: [`../vro18-native-xai-provider-prd.md`](../vro18-native-xai-provider-prd.md).  
Predecessor: [`xai-provider-pr1-execution.md`](xai-provider-pr1-execution.md).

## Evidence refresh

Official xAI documentation was re-read on 2026-09-25 for model details,
reasoning, model-list responses, structured outputs, regional endpoints,
retirements and release notes. This confirmed:

- `/v1/language-models` is the richer account-visible language-model catalog;
- Grok 4.7/4.6 support `low`/`medium`/`high`/`xhigh`; 4.5 omits distinct
  `xhigh`; 4.3 exposes `none`/`low`/`medium`/`high` (the detail page's stray
  `xhigh` row is not promoted over the capability text and retirement guide);
- multi-agent effort controls xAI-side agent count rather than reasoning depth;
- tool schemas are implicitly strict and use xAI's documented practical JSON
  Schema subset;
- the US endpoint currently constrains availability to Grok 4.7 and 4.6.

No live provider/account request ran.

## Implementation

- `XaiCatalog` now has explicit records for Grok 4.7, 4.6, 4.5, 4.3,
  4.20 reasoning, 4.20 non-reasoning, 4.20 multi-agent, and Grok Build 0.1.
  Each record owns context, reasoning choices/default, image, function-tool,
  structured-output, prompt-cache, batch and beta/semantic metadata.
- Multi-agent metadata says `agent-count`; client-side Vesper function tools
  fail closed for that model under the dedicated multi-agent contract.
- `AvailableModels` intersects authenticated discovery with exact catalog or
  explicitly registered current aliases. Unknown IDs stay `unverified` and
  cannot dispatch. Endpoint-incompatible known IDs stay separately
  `endpoint_excluded`.
- Global and US Responses/catalog origins are fixed. US selection rejects any
  model outside the documented 4.7/4.6 set before transport.
- Dispatch requires a current discovered+verified account model. Failed or
  cancelled discovery clears the previous authoritative choices.
- JSON Schema validation accepts the documented guaranteed subset and rejects
  boolean property schemas, empty unions/enums, tuple-form `items`,
  `contains`/conditional/best-effort constructs, unsupported formats/patterns,
  oversized guaranteed constraints, missing/local-circular `$ref`, excessive
  depth or node count. The same validator gates function and response schemas.
- Adapter-owned superpower descriptors project discovered model choices,
  selected-model reasoning choices and the API region; the multi-agent label
  explains its distinct semantics.

## Red to green evidence

The PR-2 anchor was added before expanding the catalog:

```text
cargo test -p vesper-provider-xai current_verified_language_model_families_are_explicit
```

Red result: expected eight verified families; actual catalog contained only
`grok-4.7`. After the static index and intersection logic were implemented, the
full targeted suite reports **20 passed, 0 failed**.

The green suite additionally proves unknown/retired exclusion, canonical alias
collapse, US endpoint constraints, reasoning-mode rejection, multi-agent tool
denial/semantic labeling, supported nested `$defs`/`$ref`, rejected ambiguous
schemas, authenticated loopback discovery, and pre-transport denial when a
model was not discovered.

## Files

Updated production/test files under `crates/vesper-provider-xai/src/`:
`catalog.rs`, `discovery.rs`, `factory.rs`, `lib.rs`, `transport.rs`, `wire.rs`,
and `tests.rs`.

Updated owning records: this report, the VRO-18 PRD, migration status,
foundation evidence index, and the closest documentation/source `AGENTS.md`.

## Verification

```text
cargo fmt --all --check
CARGO_TARGET_DIR=/tmp/agent-vesper-vro18-target cargo test -p vesper-provider-xai --all-features
CARGO_TARGET_DIR=/tmp/agent-vesper-vro18-target cargo clippy -p vesper-provider-xai --all-targets --all-features -- -D warnings
CARGO_TARGET_DIR=/tmp/agent-vesper-vro18-target cargo xtask architecture
CARGO_TARGET_DIR=/tmp/agent-vesper-vro18-target cargo xtask naming-guard
git diff --check
```

Final results: xAI **20/20 PASS**; strict Clippy PASS; format, architecture,
naming and whitespace gates PASS.

## Deviations and unresolved items

The current provider registry still does not retain `ModelCatalog` objects.
That shared composition gap must close before PR-7 host selection; PR-2 does
not add a host-specific xAI catalog branch. Grok session discovery is distinct
and remains PR-3. The conservative fixed `high` control for 4.20 reasoning and
Grok Build reflects the model pages' reasoning capability without inventing an
unverified effort menu; later first-party evidence may safely expand it.

PR-3 through PR-8 and all live acceptance remain open. No credential, user
state, original checkout, installation, version, tag or release changed.

## Readiness effect

PR-2 is closed at offline fixture scope. PR-3 may add generic browser auth plus
native browser/device Grok-session credentials, refresh/logout, proxy discovery
and strict no-fallback billing isolation. xAI remains unavailable in both hosts.
