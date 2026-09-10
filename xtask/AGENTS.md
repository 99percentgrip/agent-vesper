# Repository maintenance task

## Purpose

Own non-runtime commands for verification, fixtures, contract conformance,
architecture, MSRV, and source-oracle checks.

## Local Contracts

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
