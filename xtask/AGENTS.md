# Repository maintenance task

## Purpose

Own non-runtime commands for verification, fixtures, contract conformance,
architecture, MSRV, and source-oracle checks.

## Local Contracts

- ADR 0028 `acceptance` runs fixed named policy/runtime/native-host cases and rejects
  missing, ignored or zero-match selections. Automatic enrollment with real evidence
  and invalid enrollment without state writes are named gate cases. `verify` includes it.
  `acceptance-mutations` copies source to a temporary workspace and requires two
  deliberate evaluator defects to fail their named assertion tests; a compile failure is
  not a mutation kill. Build artifacts stay under `target/acceptance-mutations`.
  The fixed acceptance set also executes the RRC partial-matrix, verified-repair,
  pause/resume, immutable-publication, natural-language stale-epoch recovery,
  obsolete-epoch preservation, explicit canonical supersession of a historical
  local-only objective, unrelated-objective clarification, irreversible-state
  retention, registered-controller/local-subprocess lifecycle, truthful TUI task
  projection, production-orchestrator, native host cancellation/restart
  process-lifecycle, Host Resource Governor cgroup discovery, constrained RAM/swap and
  disk admission, resumable resource deferral, ACP process-level controller-status and
  Cargo-policy inheritance, and TUI-controller routing cases. The lifecycle and renderer
  cases also require live gate elapsed/activity updates, child turnover, completed-gate
  advancement, bounded output retention and exclusion of
  contradictory idle labels. The registered-controller lifecycle case injects its
  test-only resource policy directly; live pressure behavior remains owned by the
  separate governor acceptance cases.
  The Unix real-PTY case runs the actual TUI and a noisy registered child, rejecting
  inherited stdout/stderr, raw terminal controls, writes outside RUN, premature Ready,
  and stale telemetry after settlement. Deleting or renaming any case fails the gate.

- `xtask` may depend on `vesper-testkit`; production crates may not depend on it.
- Commands must not call providers or mutate source/user state.
- Verification failures return nonzero and never fabricate success.
- Platform status distinguishes local execution from CI-pending evidence.
- The architecture allowlist is explicit: composition applications may depend
  on `vesper-agent`, the TUI may use bounded session search and observability,
  and only `vesper-mcp` may use its bounded HTTP client; runtime/domain/provider
  foundations retain their HTTP and frontend bans.
- `vesper-provider-openai` is a concrete HTTP adapter boundary with no
  process-runtime or frontend dependencies; both hosts may compose it.
- `vesper-provider-xai` is a concrete HTTP adapter boundary with no
  process-runtime or frontend dependencies; both hosts may compose it after
  the VRO-18 host-parity gate established the shared registry/AgentLoop route.
- `vesper-harness` may depend on `vesper-web-fetch` to compose the shared
  sandbox-only helper transport; `vesper-web` remains pure.

- `src/swarm_gate.rs` validates Cargo metadata: production swarm dependencies
  must be optional, activated through `swarm`, and excluded from transitive
  default features (including renamed dependencies and host forwarding).
  Its unit tests exercise unconditional, indirect and renamed bypass attempts.
- `src/naming_baseline.rs` owns versioned strict JSON naming exceptions, keyed by
  normalized relative file path plus SHA-256 content digest and occurrence count.
  Line numbers are diagnostics only: unrelated line shifts do not add violations,
  but duplicated occurrences, edited content and moved paths do. Malformed,
  duplicate-entry and unknown-version baselines fail closed. Format migrations
  preserve existing frozen identities/counts; never regenerate from current hits
  merely to make the gate pass. Unit fixtures enforce these distinctions.

## Verification

- Run `cargo xtask architecture`.
- Run `cargo xtask fixtures validate`.
- Run `cargo xtask fixtures verify-index`.
- Run `cargo xtask fixtures coverage --stage 2`.
- Run `cargo xtask contracts verify`.
- Run `cargo xtask fixtures coverage --stage 3`.
- Run `cargo xtask provider glm verify`.
- Run `cargo xtask runtime verify`.
- Run `cargo xtask acp verify`.
- `acp verify` must include both the baseline transcript suite and Stage 4.1
  blocker process suite.
- Run `cargo xtask fixtures coverage --stage 4`.
- Run `cargo xtask fixtures coverage --stage 5`.
- Run `cargo xtask sessions verify`.
- Run `cargo xtask naming-guard` (VRO-15 PR-1: enforce the upstream-brand
  naming embargo against `xtask/naming-guard-baseline.json`; pass
  `--regenerate` to re-freeze after a deliberate baseline change).
- Run `cargo xtask verify`.

## Child DOX Index

No children.
