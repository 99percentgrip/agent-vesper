# Repository maintenance task

## Purpose

Own non-runtime commands for verification, fixtures, contract conformance,
architecture, MSRV, and source-oracle checks.

## Local Contracts

- Resolve repository and fixture roots from the invoking directory at runtime,
  including nested paths and shared-cache worktrees. Print the actual verification
  workspace. Never fall back to `CARGO_MANIFEST_DIR` embedded in a cached binary;
  invocations outside a recognized workspace fail before checks. The two real
  cached-binary regressions and shared-testkit invocation-root regression are
  mandatory acceptance cases. Native release admission during pending account
  discovery is mandatory as a shared pure routing case on every target and a real
  TUI process case on Unix. Native capacity and owned descendants/settlement,
  inventory validation, Windows constraints and malformed-job accounting cases
  are also mandatory on every native platform.

- ADR 0028 `acceptance` runs fixed named policy/runtime/native-host cases and rejects
  missing, ignored or zero-match selections. Automatic enrollment with real evidence
  and invalid enrollment without state writes are named gate cases. `verify` includes it.
  `acceptance-mutations` copies source to a temporary workspace and requires two
  deliberate evaluator defects to fail their named assertion tests; a compile failure is
  not a mutation kill. Build artifacts stay under `target/acceptance-mutations`.
  The fixed acceptance set also executes the RRC partial-matrix, verified-repair,
  pause/resume, immutable-publication, production-orchestrator, native host
  cancellation/restart process-lifecycle, and TUI-controller routing cases;
  deleting or renaming any case fails the gate. Mandatory RRC cases also pin
  literal 20–120 second polling, long-running platform settlement, semantic repair
  stagnation, same-objective published admission, shared closeout journaling,
  idempotent report delivery and the TUI/ACP final receipt routes. RRC cases additionally pin
  continuous polling, owner exclusivity, cancellation propagation, stale-writer
  rejection, mutation journals, exact-attempt routing, reserved retry floors,
  immutable epoch history, the complete publication inventory, secret redaction and
  nonzero native focused proof. Native patch fixtures preserve version seeds
  and new regression files; policy denial remains authoritative. Combined
  isolated AgentLoop/native proof/promotion and missing last-green platform
  evidence, missing-runner-log failure annotations and the composed
  published/docs-red/repair/different-platform-red scenario are mandatory cases.
  The fixed acceptance set also executes the RRC partial/missing-matrix active watch,
  responsive remote-poll cancellation, complete-matrix multi-family verified repair,
  pause/resume, immutable-publication, natural-language stale-epoch recovery,
  obsolete-epoch preservation, explicit canonical supersession of a historical
  local-only objective, unrelated-objective clarification, conflicting-target
  candidate/CI clarification, registered-controller/local-subprocess lifecycle, truthful TUI task
  projection, production-orchestrator, native host cancellation/restart
  process-lifecycle, Linux Host Resource Governor cgroup discovery and constrained
  RAM/swap and disk admission, resumable resource deferral, Linux ACP process-level
  controller-status and Cargo-policy inheritance through explicit and natural-language
  session-policy routes, and TUI-controller routing cases.
  The terminal cancelled/timed-out runner case requires repository-owned failure
  annotations, preserves the causal acquisition message, and rejects missing
  annotations without inventing an outage; it runs on every native target.
  The local failure receipt case requires late stdout assertions to survive bounded
  storage with secrets redacted. Numeric-thread-ID panic parsing, repair Cargo
  governor/override refusal and watcher unwind settlement are mandatory on every
  target. Linux additionally runs actual managed-cache/bounded-environment repair
  Cargo and portable controlled-metadata JSON cases.
  Host-neutral swap-window cases reject subsecond rate amplification and retain
  sustained pressure/critical detection on every target; the threshold case also
  requires displayed RAM requirements to include actual swap/owned-tree margins.
  Linux-only governor cases are enrolled only on Linux; host-neutral controller cases
  remain mandatory on every supported platform. The lifecycle and renderer
  cases also require live gate elapsed/activity updates, child turnover, completed-gate
  advancement, bounded output retention and exclusion of
  contradictory idle labels. The registered-controller lifecycle case injects its
  test-only resource policy directly; live pressure behavior remains owned by the
  separate governor acceptance cases.
  The Unix real-PTY case runs the actual TUI and a noisy registered child, rejecting
  inherited stdout/stderr, raw terminal controls, writes outside RUN, premature Ready,
  and stale telemetry after settlement. Deleting or renaming any case fails the gate.
  The probe owns its controlling terminal independently of the invoking host.
  Every platform also requires exact repair-error liveness settlement and TUI/ACP
  effective-session repair configuration cases, including pending catalog routing.
  The exact terminal header case rejects RUNNING/READY/DEFERRED for an ownerless
  epoch in ordinary and screen-reader modes; the live deferred-controller case
  remains mandatory and must not be weakened to make that negative case pass.
  Every target requires deterministic target-cache disappearance and non-missing
  I/O failure cases. Cache retirement must not stop resource observation; genuine
  observation failures still refuse admission. Fresh symlink metadata must observe
  retired Windows entries instead of cached enumeration sizes. Every target also
  requires the two-provider real-command repair continuation case: documentation
  after an earlier check triggers bounded fresh proof in the same admission.
  Shared-cache proof-executable isolation and rejection of late worker failures
  after authoritative cancellation are mandatory on every target.
  Failed isolated source-repair journal settlement and refusal for dirty or changed
  candidates, infrastructure diagnosis, cancellation and external-write state are
  mandatory on every target; settlement preserves all evidence and admissions.
  Registered-worker restart must reconcile Failed liveness before its heartbeat
  while preserving the next permission refusal; OwnerExited, legacy and uncertain
  mutation journals remain fenced through repeated requests. This case also runs
  on every native target.

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

- Publication recovery acceptance pins persisted two-retry read bounds, ledger
  reload, two-provider timeout-to-Published progression without another tag,
  cancellation/denial/invalid-evidence/uncertain-write refusal and unchanged-epoch
  matching tagged-request admission. The cumulative active publication watch survives
  reload/read recovery and stops at its deadline. Missing or renamed cases fail every target.

- RRC acceptance includes shared worker cancellation before GitHub/status/publication dispatch
  and exact admission scope for failed-job-only reruns.
  Causal selection/fingerprint regressions cover passing error-module tests, long
  linker wrappers, concrete dependency errors, unknown OS-labelled messages and
  remote cancellation without a local user-cancel claim, and account execution
  restrictions requiring owner action without source repair or outage retry.
  Typed Python fixture runtime exceptions and preserved owner-action/unknown
  classification are mandatory source-diagnosis cases on every native target.
  Settled Windows CLI assertions, earlier APT download errors and same-step
  dependency-index timeout context are mandatory cases; metadata, command echoes,
  previous steps and unproven timeouts must not authorize repair.
  The named native-worker case executes both direct and continuous routes and pins
  owner-action/uncertainty settlement before permission, repair or mutation journals.
  The named health-routing case proves read-only diagnosis without source permission,
  authoritative rerun refusal and firewall checks for the actual GitHub write scope.
  The named repair-authority case uses real native tools and a temporary Git repo
  to prove that model commands cannot create a tag outside controller admission.
  The named repair-budget case executes distinct real file observations with host
  caps zero, five and one hundred, and proves bounded unsuccessful exhaustion
  without changing the host configuration. Mandatory cases cover inherited lock
  descriptions, initial adapter-admitted retry, both-auth-mode HTTP rejection policy
  and numeric/date server delays; action/fragment replay and long-delay retries refuse.
  Mandatory preparation coverage executes a compiler failure, exact rollback,
  causal journal settlement, admitted patch preparation and tamper refusal. The
  two-provider composition case exercises both remote and preparation promotion.
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
