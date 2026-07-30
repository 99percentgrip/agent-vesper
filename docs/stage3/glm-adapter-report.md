# Stage 3 GLM Provider Adapter Report

Status: COMPLETE

## Implemented result

`crates/vesper-provider-glm` is a production leaf adapter behind the Stage 2
provider-neutral ports. Its modules separate adapter orchestration,
authentication, auxiliary requests, static catalog, legacy compatibility,
configuration/endpoints, safe errors, factory, quota, request mapping, response
normalization, retry, bounded SSE, and HTTP transport. It implements
`ProviderFactory`, `ModelCatalog`, `ProviderSession`, response streaming, and
`AuxiliaryRequestPort`. No ACP, core runtime, session store, tool executor,
policy orchestration, or frontend was created.

The static catalog contains six provider-qualified frozen models. Coding Plan,
Standard API, and BigModel CN use exact pinned URLs; custom endpoints are
explicit. Authentication checks `ZAI_API_KEY` before `Z_AI_API_KEY`, remains in
`SecretValue`, and is attached only at dispatch. Quota authentication is limited
to exact official HTTPS identities and is independently bounded/nonfatal.

Request serialization covers ordered messages/system instructions, text/image
content, tools/tool choice/tool streaming, thinking, `clear_thinking`,
`reasoning_effort`, preserved reasoning, generation controls, maximum output,
auxiliary requests, fallback visibility, and exact continuation behavior.
Legacy model, plan, reasoning, generation, and auxiliary settings translate
without session I/O.

The harness-owned parser accepts arbitrary chunks, UTF-8 splits, LF/CRLF,
comments/ignored lines, `[DONE]`, usage-only chunks, and source-compatible
malformed JSON. It bounds line/event/tool/metadata/error accumulation.
Normalization preserves reasoning/content/tool/usage order, assembles
interleaved tool indexes, emits checked cumulative continuation usage, and
produces one terminal result.

Retry behavior covers 429/500/502/503/504, numeric and HTTP-date
`Retry-After`, 60-second caps, injectable jitter, cancellation during backoff,
and no replay after visible output. Cancellation is tested before dispatch and
headers, mid-stream, during retry, continuation, and tool assembly; it is
idempotent and emits no post-cancel output. Automatic continuation uses the
exact frozen sentence, excludes tools/auxiliary bounded calls, and caps at 20.

## Evidence and intentional strengthening

Source evidence is `glm_acp/glm_client.py::GlmClient` lines 111–803,
`glm_acp/config.py::RETRYABLE_STATUS_CODES/API_ENDPOINTS/MODELS/THOUGHT_LEVELS`
lines 415–628, focused source tests, and all 21 GLM oracle scenarios.

Intentional strengthening is limited to parsed endpoint identity, bounded wire
accumulators/error bodies, deterministic missing tool IDs, secret-safe
diagnostics, bounded backpressure, checked usage arithmetic, cancellation-safe
cleanup, and strict no-replay after visible output. Successful observable GLM
semantics remain source-compatible.

## Files and dependencies

Created the adapter crate, `fixtures/coverage-stage3.json`, nine Stage 3 reports,
and Stage 3 DOX files. Updated workspace/lock/governance, `xtask`, CI, root
architecture/workspace/dependency/migration/README records, and applicable
AGENTS indexes. Direct additions are `reqwest 0.13.4` (Rustls), `tokio 1.52.0`,
`httpdate 1.0.3`, and dev-only `futures-util 0.3.33`; exact features and license
review are in `docs/dependencies.md`.

## Fixtures and tests

The corpus was unchanged: 76 scenarios, 154 indexed payloads, fixture-index
SHA-256
`d09edfe2169df49e0cfef9a66083a7df046651f441deb0e78bc0c855dec6db7a`.
Stage 3 implements all 21 source GLM scenarios; 51 unrelated runtime scenarios
remain explicitly assigned to future owners. No recapture was required.

Local verification:

| Gate | Result |
| --- | --- |
| Rust 1.95 workspace tests | 88 passed, 0 failed/ignored |
| GLM adapter tests | 25 passed, including all 21 source scenarios |
| Rust 1.88 MSRV tests | 88 passed, 0 failed/ignored |
| format / strict Clippy / check / docs | PASS |
| fixture schema/index/Stage 3 coverage | 76 / 154 / 21 GLM PASS |
| contracts / architecture | PASS; 8-package acyclic graph |
| Cargo Audit | PASS; 231 dependencies, no vulnerability |
| Cargo Deny | PASS with reviewed transitive duplicate warnings |
| workflow YAML / forbidden scans | 4 workflows PASS / no prohibited findings |

Linux x86-64 is locally validated. Linux ARM64, macOS Intel, macOS Apple
Silicon, and Windows x86-64 remain CI-validation pending.

## Invariance, target state, and Stage 4 input

Frozen source HEAD remains
`bf4d4287e2e3320aa3f09015f678e6169d520045`; tracked diff is empty and the only
untracked source file remains `docs/codex-tui-roadmap-prompt.md`. The target is
on `main`, has no remote and no initial commit, and therefore reports all
project content as untracked. No commit was created.

Stage 4 receives `GlmFactory`, `GlmSession`, `GlmCatalog`, neutral events,
owned cancellation, and the executable fixture matrix. It should add only the
ACP adapter/minimal provider runtime and preserve the absence of agent/tool
loop and persistence. Remaining risk is real execution on the four pending CI
target families and later runtime-level backpressure/cancellation integration.

READY FOR STAGE 4 — ACP ADAPTER AND MINIMAL PROVIDER RUNTIME WITH CI VALIDATION PENDING

