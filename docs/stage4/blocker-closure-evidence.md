# Stage 4.1 blocker-closure evidence

Status: COMPLETE

## Scope

Close only the seven missing real-process transcript vectors. Sessions remain
ephemeral; no persistence, agent/tool loop, or frontend work is authorized.

## Preflight

- Target: `main`, no initial commit, no remote, 21 top-level untracked paths.
- Workspace: 11 production/maintenance packages, 97 tests before Stage 4.1.
- Runtime bounds: event channel 64; session command channel 16.
- ACP bounds: request and notification queues 32.
- Fixture corpus: 76 scenarios, 154 indexed payloads,
  `d09edfe2169df49e0cfef9a66083a7df046651f441deb0e78bc0c855dec6db7a`.
- Source: frozen HEAD `bf4d4287e2e3320aa3f09015f678e6169d520045`;
  only `docs/codex-tui-roadmap-prompt.md` is untracked.
- Context7 was attempted for Tokio synchronization behavior; its configured
  service reported monthly quota exhaustion. Pinned local crate sources and
  existing tested contracts remain the implementation evidence.

## Blocker ledger

| Vector | Existing unit/integration proof | Missing process proof | Final result |
| --- | --- | --- | --- |
| Retry before visible output | Stage 3 retry/`Retry-After` loopback tests | Real ACP process request count/order | PASS — two requests |
| Continuation | Stage 3 exact continuation and usage tests | Real ACP continuous output and terminal | PASS — two requests, cumulative usage 10 |
| Post-output interruption | Stage 3 no-replay stream state tests | Real ACP partial output and reusable session | PASS — one request, later prompt succeeds |
| Cancel before dispatch | Provider cancellation tests | Deterministic zero-request process barrier | PASS — zero requests |
| Cross-session concurrency | Actor-per-session architecture | Overlapping real HTTP requests | PASS — maximum concurrency 2 |
| Same-session serialization | One actor per session | Real provider maximum concurrency one | PASS — maximum concurrency 1 |
| Slow-consumer backpressure | Bounded channel contracts | Slow stdout reader, RSS, recovery/cancel | PASS — 6,000 ordered deltas; cancellation responsive |

## Commands

- Read all scoped DOX contracts and Stage 3/4 evidence required by the mission.
- Target/source Git preflight, Cargo metadata, test enumeration, fixture hash,
  and bounded-channel/task searches.
- `cargo check -p vesper-acp --all-targets` during the bounded writer change.
- `cargo test -p agent-vesper-acp --all-features --test process_blockers
  --no-run`.
- Each blocker process test individually during diagnosis; then the eight-test
  suite with `--test-threads=1`.
- Existing `process_transcript` suite after ACP flow-control changes.
- Stability loops: cancel 25, cross-session 25, same-session 25, slow-reader
  10, and cancellation-under-pressure 10.
- `cargo check --workspace --all-targets --all-features`.
- `cargo fmt --all --check`, `cargo fmt --all`, then final check.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- `cargo test --workspace --all-features` and
  `cargo test --workspace --doc`.
- `cargo xtask fixtures validate`, `fixtures verify-index`,
  `fixtures coverage --stage 4`, `contracts verify`, `provider glm verify`,
  `runtime verify`, `acp verify`, `architecture`, `msrv`, and `verify`.
- `cargo audit` and `cargo deny --all-features check`.
- Python YAML parse for all four workflow files.
- Production placeholder/ignored/scenario-ID/persistence/unbounded-channel
  scans; process-leak scan with `ps`.
- Final source HEAD/diff/status invariance and target branch/HEAD/remote/status
  inspection.

## Changes and results

## Changes and results

- Added one shared RAII process harness and eight process tests for the seven
  blockers plus cancellation under pressure.
- Added physical-writer flow control to `vesper-acp`; outstanding visible
  updates no longer accumulate in the SDK queue when stdout stalls.
- Added a non-default, required-feature integration driver for generic
  pre-dispatch coordination. Architecture verification rejects making this
  feature default.
- Corrected the shared response collector to retain out-of-order JSON-RPC
  responses discovered during cross-session repetition.
- Extended Stage 4 coverage, ACP verification, architecture checks, and the
  five-target workflow.
- Workspace tests: 105/105 on Rust 1.95 and Rust 1.88; no ignored tests.
- Process transcripts: 12/12.
- Formatting, strict Clippy, architecture, fixtures, Cargo Audit, and Cargo Deny
  pass. Cargo Deny reports only the already reviewed transitive duplicate
  warnings.
- No surviving process or local server was found.
- Fixture corpus is unchanged: 76 scenarios, 154 payloads, index
  `d09edfe2169df49e0cfef9a66083a7df046651f441deb0e78bc0c855dec6db7a`.
- Source invariance confirmed exactly. Target remains `main`, has no initial
  commit or remote, and reports the same 21 top-level untracked paths.
- Linux x86-64 is locally validated; four target families remain CI pending.
