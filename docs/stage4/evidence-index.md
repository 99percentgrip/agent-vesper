# Stage 4 evidence index

Status: COMPLETE

## Scope

Production ACP protocol-v1 adapter, minimal provider-neutral runtime, thin ACP
composition binary, deterministic process transcripts, and Stage 5 readiness.

## Baseline

- Target branch: `main`; repository has no commit and no remote.
- Frozen source commit: `bf4d4287e2e3320aa3f09015f678e6169d520045`.
- Fixture corpus: 76 scenarios, 154 indexed payloads,
  `d09edfe2169df49e0cfef9a66083a7df046651f441deb0e78bc0c855dec6db7a`.
- Stage 3 baseline: 88 workspace tests on Rust 1.95 and 1.88.

## ACP SDK gate

- Confirmed locally from `agent-client-protocol` 2.0.0 and
  `agent-client-protocol-schema` 1.5.0.
- Protocol wire version is v1.
- `PromptResponse` lacks the frozen source's top-level `userMessageId`.
- The SDK permits handler request enums whose response type is
  `serde_json::Value`; the compatibility wrapper can therefore preserve SDK
  parsing/dispatch and emit the required response shape.
- SDK request handlers execute on the inbound event loop; Stage 4 callbacks
  must enqueue bounded work and return promptly.
- Context7 lookup was attempted first and was unavailable because the configured
  service quota was exhausted; the pinned successful spike and local published
  crate source are the primary evidence.

## Commands

- `git status --short`, branch/remote/workspace/fixture/source preflight.
- targeted reads of Stage 2/3 reports, ADRs, DOX files, and ACP spike evidence.
- local Cargo registry searches and reads for ACP 2.0.0 request dispatch,
  response serialization, prompt schema, and `ByteStreams`.
- `cargo check --workspace --all-targets --all-features`.
- `cargo fmt --all --check`.
- strict workspace Clippy and tests, workspace doc tests, and targeted runtime,
  ACP, GLM, and process transcript suites.
- all fixture/index/coverage/contract/provider/runtime/ACP/architecture xtasks.
- `cargo xtask verify` and `cargo xtask msrv`.
- `cargo audit` and `cargo deny --all-features check`.
- workflow YAML parse, forbidden-code/dependency/I/O scans, fixture-index hash,
  source Git invariance, and target Git status.

## Files created

- `docs/stage4/AGENTS.md`
- `docs/stage4/evidence-index.md`
- `crates/vesper-runtime/AGENTS.md`
- `crates/vesper-acp/AGENTS.md`
- `apps/AGENTS.md`
- `apps/agent-vesper-acp/AGENTS.md`
- Production code and manifests under `crates/vesper-runtime/`,
  `crates/vesper-acp/`, and `apps/agent-vesper-acp/`.
- `fixtures/coverage-stage4.json`.
- Nine Stage 4 reports plus this ledger.

## Files modified

- Workspace manifest/lockfile and dependency policy.
- `xtask` Stage 4 coverage, runtime, ACP, and architecture gates.
- README, architecture/workspace/dependency/migration records.
- root/scoped DOX indexes and CI workflows.

## Results

- 105 workspace tests pass on Rust 1.95 and Rust 1.88; no ignored tests.
- Twelve process transcript suites pass, including all seven Stage 4.1
  blockers and cancellation under backpressure.
- 76 scenarios and 154 indexed payloads validate; the index remains
  `d09edfe2169df49e0cfef9a66083a7df046651f441deb0e78bc0c855dec6db7a`.
- Formatting, strict Clippy, architecture, Audit, and Deny pass. Cargo Deny
  reports reviewed transitive duplicate warnings only.
- Linux x86-64 is locally exercised; four target families remain CI pending.
- Source HEAD and tracked state are invariant; only
  `docs/codex-tui-roadmap-prompt.md` remains untracked.

## Stage 4.1 closure

- Retry, continuation, post-output interruption, cancel-before-dispatch,
  cross-session concurrency, same-session serialization, and slow-consumer
  backpressure all pass through the real process path.
- Stability: 25/25 for cancellation and both concurrency vectors; 10/10 for
  each pressure vector.
- Stage 5 is locally unblocked; remote target-family CI remains pending.
