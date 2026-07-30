# Stage 2 Contract Completion Report

Status: COMPLETE

## Outcome

Agent Vesper now has stable, executable provider-neutral contracts for the
future GLM adapter and ACP/core boundaries. No concrete provider, network
transport, ACP server, agent loop, tool executor, persistent store, or frontend
was implemented.

## Files created

- Domain: `crates/vesper-domain/src/command.rs` and
  `crates/vesper-domain/src/compatibility.rs`.
- Testkit: `crates/vesper-testkit/src/conformance.rs`.
- Fixtures: `fixtures/coverage-stage2-plan.json`,
  `fixtures/coverage-stage2.json`, and 11 directories under
  `fixtures/contracts/`.
- Oracle: `tools/python-oracle/generate_contract_fixtures.py`.
- Stage 2 reports: this report, `evidence-index.md`, `fixture-coverage.md`,
  `oracle-expansion-report.md`, `acp-contract-map.md`,
  `provider-contract-map.md`, `session-v1-compatibility.md`, and
  `stage3-readiness.md`.

## Files modified

- Domain modules/exports: IDs, versions, extensions, content, messages, tools,
  usage, finish/errors, plans/goals, sessions, events, and crate instructions.
- Provider modules/exports: capabilities, request/continuation/fallback,
  catalog/factory/session/auxiliary ports, owned cancellation, stream state, and
  provider errors.
- Testkit fixture/fake/normalization exports and tests.
- `xtask` commands and architecture/coverage/contract gates.
- Fixture schemas/index/instructions and Python coordinator.
- CI workflows for pull-request, MSRV, and five-target foundation checks.
- `README.md`, architecture, workspace map, dependency register, migration
  status, and applicable DOX instructions.

No dependency manifest or direct dependency changed.

## Final contract surface

### Domain

- 18 distinct opaque identity classes plus provider-qualified model identity and
  optimistic revisions.
- Validated schema versions and bounded namespaced extension envelopes.
- Ordered text/image/audio/tool/reasoning/context/provider-opaque content.
- Debug-redacted bounded opaque provider continuation data.
- Structural tool definitions, distinct harness/provider names, fragmented call
  identity, completed arguments, result status/location/diff/linkage.
- Usage provenance, delta/cumulative modes, checked arithmetic, and inconsistent
  total visibility.
- Distinct finish/cancellation/partial-output/protocol/error outcomes.
- Safe shared error taxonomy and no-plain-replay classification.
- Versioned, correlated `HarnessCommand` covering initialization, all session
  operations, prompt/slash/cancel/config/mode/permission/shutdown.
- Versioned `HarnessEvent` covering runtime/session/message/reasoning/content/
  tools/permissions/plan/goal/usage/provider/context/warning/error/terminal/
  shutdown.
- Per-runtime/session/turn monotonic sequencing, unique turn terminal, and no
  post-terminal events.

### Provider

- Typed support algebra and capability descriptors for limits, reasoning,
  preservation, media, tool dialect/choice/parallel/streaming, caching,
  structured output, discovery, authentication, quota, continuation, process,
  and external-runtime ownership.
- Complete request intent with qualified model, ordered input, controls,
  continuation, fallbacks, and versioned extensions.
- Pre-dispatch validation of ownership, explicit capability intents, unsupported
  requirements, sampling, output, and continuation bounds.
- Small catalog/factory/session/auxiliary ports and owned cancellation views.
- Normalized stream, quota/rate/warning, terminal, visible-output, and retry
  contracts with poll-driven bounded-backpressure requirement.

## Session-v1 result

All seven authoritative outcomes are covered. Five complete/defaulted records
decode and re-encode with exact known-field equality; omitted fields receive
documented defaults; unknown fields round-trip; invalid bounds/corrupt JSON are
typed errors; lineage and reasoning behavior are preserved; GLM settings stay
inside explicit compatibility types. The codec performs no filesystem I/O.

## ACP-neutral result

Every ACP fixture maps to shared commands/events for initialization,
capabilities/authentication, session lifecycle, roots, prompt/message identity,
slash commands, cancellation, permissions, replay/order, plan/tools/usage, and
terminal outcomes. ACP SDK and wire types remain absent. The known
`userMessageId` placement remains an explicit future adapter obligation.

## Fixtures and coverage

| Measure | Result |
| --- | ---: |
| Original source scenarios | 65 |
| Added synthetic contract vectors | 11 |
| Final scenarios | 76 |
| Indexed payloads | 154 |
| Implemented Stage 2 contract representations | 76 |
| Scenarios with deferred runtime behavior | 53 |
| Scenarios with no remaining Stage 2 runtime behavior | 23 |

Final fixture-index SHA-256:
`d09edfe2169df49e0cfef9a66083a7df046651f441deb0e78bc0c855dec6db7a`.
Two independent synthetic generations were byte-identical.

## Verification

| Gate | Result |
| --- | --- |
| `cargo check --workspace --all-targets --all-features` | PASS |
| `cargo fmt --all --check` | PASS |
| strict all-target/all-feature Clippy | PASS |
| Rust 1.95 workspace tests | 63 passed, 0 failed/ignored |
| Rust 1.88 MSRV tests | 63 passed, 0 failed/ignored |
| doc tests | PASS |
| fixture schema/index | 76 scenarios / 154 payloads PASS |
| Stage 2 coverage/contracts | PASS |
| architecture boundaries | 7 packages PASS |
| Cargo Audit | PASS, no vulnerability |
| Cargo Deny | PASS; reviewed transitive `syn` duplicate warning |
| workflow YAML | 4 files parsed |
| placeholder/forbidden scans | empty |

## Source invariance and target status

Frozen source HEAD remains
`bf4d4287e2e3320aa3f09015f678e6169d520045`; tracked diff is empty; only the
pre-existing untracked roadmap document remains.

Target remains on `main` with no remote and no initial commit. Consequently all
project content is still reported as untracked. Existing reconnaissance,
foundation, Stage 1, fixtures, oracle, and spike work was preserved. No commit
or remote was created.

## Platform/CI status

Linux x86-64 is locally validated. Linux ARM64, macOS Intel, macOS Apple
Silicon, and Windows x86-64 remain CI-validation pending; no pending platform is
described as passing.

## Precise Stage 3 inputs

Stage 3 receives the request/capability/ports/stream/error/continuation contracts,
21 source GLM fixtures, applicable synthetic vectors, the SSE spike evidence,
and the test matrix in `stage3-readiness.md`. It owns exact GLM wire
serialization, local SSE transport, retries, cancellation, continuation,
authentication, and model discovery—nothing else.

## Remaining risks

- Remote platform jobs have not run.
- Real transport timing/backpressure/cancellation is not yet production code.
- Exact GLM configuration/wire translation may expose a neutral contract gap;
  any change requires an evidence-backed vector rather than adapter leakage.
- Opaque reasoning must stay outside generic sinks in every later stage.
- All target files are uncommitted because the repository intentionally still
  has no initial commit.

READY FOR STAGE 3 — GLM PROVIDER ADAPTER WITH CI VALIDATION PENDING
