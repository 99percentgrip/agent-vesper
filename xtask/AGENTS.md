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
  pause/resume, immutable-publication, production-orchestrator, native host
  cancellation/restart process-lifecycle, and TUI-controller routing cases;
  deleting or renaming any case fails the gate. RRC cases additionally pin
  continuous polling, owner exclusivity, cancellation propagation, stale-writer
  rejection, mutation journals, exact-attempt routing, reserved retry floors,
  immutable epoch history, fourteen publication assets, secret redaction and
  nonzero native focused proof. Native patch fixtures preserve version seeds
  and new regression files; policy denial remains authoritative. Combined
  isolated AgentLoop/native proof/promotion and missing last-green platform
  evidence, missing-runner-log failure annotations and the composed
  published/docs-red/repair/different-platform-red scenario are mandatory cases.

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
  sandbox-only helper transport and on pure `vesper-policy` for native RRC
  firewall enforcement; `vesper-web` remains pure.

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

- RRC acceptance includes shared worker cancellation before GitHub/status/publication dispatch
  and exact admission scope for failed-job-only reruns.
  Causal selection/fingerprint regressions cover passing error-module tests, long
  linker wrappers, concrete dependency errors, unknown OS-labelled messages and
  remote cancellation without a local user-cancel claim, and account execution
  restrictions requiring owner action without source repair or outage retry.
  The named native-worker case executes both direct and continuous routes and pins
  owner-action/uncertainty settlement before permission, repair or mutation journals.
  The named health-routing case proves read-only diagnosis without source permission,
  authoritative rerun refusal and firewall checks for the actual GitHub write scope.
  The named repair-authority case uses real native tools and a temporary Git repo
  to prove that model commands cannot create a tag outside controller admission.
  The named repair-budget case executes distinct real Cargo commands with host
  caps zero, five and one hundred, and proves bounded unsuccessful exhaustion
  without changing the host configuration.
  Provider-owned server tool selections are distinct from client gateways; the
  named fixture proves their request/configuration retention without measuring
  live hosted execution.

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
