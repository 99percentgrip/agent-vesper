# vesper-harness — shared hosted agent services

## Purpose

Own the composition-neutral hosted tool service used by production frontends.
It supplies the Python-oracle memory, skills, awareness, deliberation,
failure-corpus, cron, session-search, delegation, source-inspection,
transactional patch, workflow, signed-plugin, worktree, and MCP gateway tools.
It also exposes the Python-compatible `web_search`, `web_reader`,
`vision_analyze`, and permission-gated `browser_ui` presets through protected
Z.ai and Playwright MCP server descriptors.

## Ownership

- `release_recovery` owns the provider-neutral RRC state machine, transactional
  reducer, retry/focused-proof budgets, exact-SHA settled workflow evidence,
  first-causal extraction, secret redaction and immutable last-green context.
  GitHub inventories paginate with bounds and use attempt-specific job lists;
  canonical workflow names come from workflow-ID metadata rather than evaluated
  run titles; only the four required main-push workflow identities advance gates.
  Missing, running, skipped or stale evidence cannot authorize publication.
  Failed-job-only reruns use the shared evidence port with a consumed full or
  infrastructure admission scoped to the exact run; targeted job tokens cannot
  authorize a run-wide request. Unadmitted write methods remain blocked.
  Last-green comparisons require the matching platform/job; missing/skipped
  historical lanes preserve unknown context. Host Git metadata queries use
  bounded native process execution.
- `release_executor` owns continuous bounded polling, local verification,
  version/commit/exact-SHA push/tag/publication, official-status admission and
  bounded isolated AgentLoop repair. Host mode, permission port and command
  firewall apply to native mutations and repairs. Nonrepeatable operations are
  journaled before execution; uncertain restart never replays them.
  Promotion requires an observed mutation, explicit hypothesis, native focused
  re-verification after the final edit, a nonempty patch and full local gates.
  Cargo test proof must execute tests and include an identifiable failing test,
  including explicit rustup toolchain selections. Rust panic parsing accepts numeric
  thread IDs as well as older messages, preserving exact failing-test binding.
  Production repair Cargo commands use the controller governor, exclusive resource
  lease, managed cache, bounded jobs/test threads and native owned-child progress/
  cancellation watchdog. Its real output/process transitions refresh the repair
  heartbeat and registered-worker telemetry; quiet time alone never does.
  Model flags cannot override that policy or workspace.
  Missing governors and selected sandboxes without a governed Cargo route refuse
  before host dispatch; the current sandbox port cannot bind the managed cache and
  environment. Unwinding or dropping a repair task settles its cancellation watcher.
  Legacy ungoverned commands exist only in explicit unit-test repair fixtures. The repair role advertises and
  dispatches only core source tools with supported verification commands; direct
  Git/GitHub lifecycle commands, compound shells and explicit Git metadata writes
  refuse before execution. The client registry has no hosted gateways; previously
  selected provider-owned server tools remain in provider requests under their
  existing adapter policy. Existing permission/firewall/sandbox enforcement still
  applies; verification subprocesses
  are not an additional OS sandbox. Controller-owned release operations use typed
  admissions outside the model tool registry.
  The repair factory uses a 24-iteration segment limit even when the invoking host
  disables its ordinary cap with zero; smaller positive host limits stay smaller.
  The existing native plan continuation and progress heartbeat watchdog still apply.
  This copied repair configuration never changes the ordinary host setting.
  Local repair preserves the version seed and includes new files; candidate
  commits include only admitted version/repair paths. Native version preparation
  resolves the complete Cargo workspace inventory before writing, updates all
  member path-dependency pins, and records those manifests in candidate admission.
  Git repair fixtures declare LF bytes explicitly across native targets.
  The RRC coding factory fixture runs real file/command tools and native failing
  then passing Rust tests with two registered fixture provider IDs; fixture
  streams never establish real model effectiveness or production transport.
  The combined composition fixture verifies native isolated-worktree proof and
  one-patch promotion for both fixture IDs; only its fixture-wide gate port is
  substituted. Production always executes the complete native gate set.
  Measured CI waits/model repair time and denied autonomous retries persist
  separately from pure status queries and idle time across host restarts.
- RRC ledger writers redact all strings, reject stale/cancelled writers and
  hold one cross-process owner lock during progression. Explicit resume retains
  identity/counters and refreshes remote evidence first. A watcher propagates
  persisted cancellation to another host's local process group or Job Object.
  The parallel-runner watcher fixture has a ten-second scheduling bound;
  production polling remains 100 ms and cancellation still requires observation.
  `NativeReleaseExecutor::with_resource_governor` attaches the real controller-owned
  governor for standalone native verification; absent governors still refuse Cargo gates.
  Background GitHub inventory/log/admitted-retry, official status, last-green and
  publication reads share the worker cancellation token; standalone evidence
  adapters retain bounded read-only use outside an active worker.
  Passive CI polls do not consume the active stagnation budget. Infrastructure
  recovery reserves budget/attempt floors before its scoped rerun.
- Published assets require a settled exact-SHA tag workflow, an annotated tag
  targeting that SHA, nondraft/nonprerelease metadata and the complete expected
  inventory of nonempty uploaded archive/checksum assets. Downloaded checksum contents must match the
  archive and checksum asset's server SHA-256 digests and exact filename.
  Credential/normalization regexes compile once with thread-safe LazyLock;
  ledger string redaction must not repeatedly compile patterns per field.
  CI logs and fingerprints share a cached causal matcher: passing `error::`
  test names, generic linker/compiler exit wrappers and long preceding linker
  argument lines cannot hide the actual diagnostic. Unknown text stays Unknown
  despite an OS label; concrete missing-package/version messages are dependency
  failures. Remote operation cancellation remains failed/uncertain CI evidence,
  not a local user-cancelled epoch. GitHub account payment/spending restrictions
  require owner action: classify as credential/permission failures and escalate
  without model source repair, outage claims or automatic retry.
  Direct executor and continuous worker honor this directive before source repair,
  host mutation permission or mutation journaling; uncertain causes stop for evidence
  before those side effects as well.
  Read-only infrastructure health checks likewise precede mutation permission and
  journaling. An admitted infrastructure rerun still requires owner permission and
  scans the actual scoped `gh api --method POST .../rerun` commands; unrelated Git
  patch rules cannot substitute for GitHub write policy.
  Local command failures select a causal excerpt from both captured stdout and
  stderr before receipt truncation, preserving late Rust assertions and redaction.
  CI logs strip terminal controls and exclude runner command echoes before causal
  extraction; fingerprints ignore unrelated interleaved output/exit wrappers.
  A 404 log response may use bounded failure annotations only from the completed
  failure, cancelled or timed-out job's repository-owned check URL. A concrete
  runner-acquisition failure annotation selects runner infrastructure evidence;
  cancellation alone never proves an outage. Missing annotations remain unknown;
  permission/transport failures cannot use this fallback.
  A later main SHA uses a separate archived
  epoch; published version/tag/assets remain immutable. Current acceptance and
  unexecuted platform/live/publication gates belong to the PRD-linked report;
  historical five-target runs do not validate changed source. The combined
  Published/docs-red/focused-repair/different-platform-red fixture pins
  immutable publication, new diagnosis and exhausted full-retry refusal.
- `release_recovery` owns the provider-neutral Release Recovery Controller:
  conservative natural-language release admission shared by both hosts;
  `admit_natural_release_with_launcher` is the shared composition port for canonical
  source reconciliation; its caller owns worker lifetime and permission enforcement.
  repository-wide objective-provenance resolution; ambiguity refusal before
  mutation; isolated clean release-worktree creation; persisted objective identity,
  human labels/evidence reports, source workspace, base/source/final SHAs, ordered
  intended commits, binary-diff hash and release workspace; and the same typed
  `/release` lifecycle. A clean candidate is eligible only when its committed
  `docs/foundation/release-objective-provenance.json` binds a validated completed
  objective and existing committed evidence reports. The newest completed objective
  excludes unrelated historical worktrees before candidate comparison. A committed
  canonical-source marker excludes retained historical variants; explicit supersession
  may remove an ancestor variant. Equal commit trees and equal implementation diffs
  collapse after bounded provenance-only paths are removed. Only genuinely different
  maximal implementations for that same objective may clarify, using unique human
  labels or label-plus-subject descriptions rather than paths or SHAs; raw Git
  provenance is exposed by `/release evidence`. Questions, planning prose,
  negation, deferred intent and release-process discussion never admit a release.
  Admitted objectives carry one typed release-version selector: `patch`, `minor`,
  `major`, or an exact stable `X.Y.Z` target normalized from optional `vX.Y.Z`.
  Exact targets are visible in status before version mutation. A lower target
  fails closed; an equal already-versioned target validates without rewriting
  manifests, `Cargo.lock`, or Registry metadata and may reuse clean canonical
  HEAD; a higher target flows through the transactional version graph update.
  A dirty active checkout is
  never cleaned, reset, stashed or used as the release mutation workspace.
  The typed lifecycle, controller directives/settled-evidence reducer and immutable
  failure evidence; the repository's four exact-SHA pre-release gates; first-causal
  log extraction/redaction, stable fingerprints, last-green comparison,
  retry/focused-proof/diagnostic budgets, outage admission, stagnation limits,
  bounded atomic user-state checkpoints, structured GitHub Actions access, and
  the shared `/release`/`/ci` projection. Natural-language admission reconciles an
  active checkpoint before creating another: matching recoverable local stages
  resume automatically; stale same-objective source/workspace identities and a
  canonical integrated source that explicitly supersedes a historical local-only
  objective are atomically replaced only after the full old record is archived
  under the user-owned release-state root. Candidate push, remote CI and failure
  receipts remain historical evidence: for the same resolved target, a newer
  canonical strict descendant may replace the active candidate after the full old
  record is archived. A missing, dirty, or provenance-mismatched release workspace
  cannot replace a remote-evidence epoch from the unchanged source because it lacks
  that strict-descendant proof. Selector syntax is not target identity (`Patch` from
  `0.24.4` and exact `0.24.5` are equal). Conflicting targets with candidate/CI
  evidence, non-descendant sources, pushed target tags, and actually published target
  releases require bounded clarification. The active and replacement records retain
  stable objective/variant identities, canonical-source status and supersession
  links. Unrelated objectives and the fail-closed cases above produce one bounded
  human clarification without mutation; that prompt names both human
  objectives, includes known version transitions and describes the irreversible
  state without paths, raw SHAs or internal state names. Primary workspaces are never
  changed by reconciliation. Resume refreshes exact-SHA GitHub state before progression.
  `RemoteGateRunning`, `WaitingForMatrix`, and `ClassifyingFailure` remain active
  controller-owned worker states: missing/partial runs poll at 20/40/80/120-second
  bounded intervals, passive refreshes do not spend the six-action repair watchdog,
  cancellation interrupts each wait promptly, and restart reloads the same epoch before
  continuing the exact-SHA watch. Missing runs retain a 20-minute evidence window;
  known queued/running jobs use a two-hour passive watch so 60-minute platform jobs
  can settle without consuming active repair limits. Later-main recovery clears
  the prior completion receipt and writes a distinct full-SHA main report. GitHub write operations require
  an unforgeable controller admission token; adapters, providers and hosts do not
  decide retry policy. `release_executor` runs the repository's existing local gates, version/commit/
  push/tag/publication path, bounded official GitHub-status check, persisted
  background progression and cancellable local subprocesses. Every controller-owned
  process group is polled for cancellation and bounded by state-specific meaningful-
  progress inactivity. Local gates use a 30-minute inactivity window and local
  mutation commands use five minutes, with no elapsed-runtime ceiling while output or
  reliable process-tree activity advances. Process-tree heartbeat advances only on
  owned process-count or `rustc`-count transitions; RSS remains diagnostic telemetry
  and never resets inactivity. Individual external operations retain
  protocol/safety deadlines: remote Git mutations use 5/30 minutes, GitHub evidence
  requests use 2/5 minutes, publication requests use 2/10 minutes and official health
  requests use 15 seconds. The repair AgentLoop uses its progress port as a heartbeat
  with a 10-minute inactivity bound. A separate semantic watchdog hashes actual
  successful tool observations, excludes call IDs and command timing, and stops
  after six repeated/failed actions or 20 minutes without new evidence. Streamed
  text/status never resets semantic progress. Governed focused verification retains
  its native subprocess watchdog; a progressing compiler may outlast 20 minutes.
  Observation identities are bounded to 4096 hashes per repair. Every causal-family
  repair admission is counted and persisted before model dispatch, including turns
  that fail before focused proof; restarts cannot reset that limit. Disproven prior
  hypotheses/proof status enter the next repair as untrusted evidence. New later-main
  epochs start their own admission counters while preserving the published record. Exact-SHA dispatch
  starts a fresh remote-matrix window under the separate 20-minute no-evidence bound.
  Watchdog outcomes are persisted as typed
  liveness failures, never source/CI evidence, and both hosts render the shared
  progress headline after worker exit or restart. `Active` stores the owner pid.
  A missing or dead owner becomes `OwnerExited` and stays recoverable; it is not
  a source failure and must not be rendered as a live run. Version preparation
  handles governor rejection through the same persisted `ResourceDeferred` path as
  local gates: restore version bytes before returning, settle the preparation journal,
  retain the pending gate/reason and watch for three safe observations before retry.
  Resource errors escaping a worker retain redacted diagnostics without source
  failure evidence or clearing an uncertain mutation journal. Version preparation
  persists the resolved release version and a visible `version-preparation` gate
  before the expensive workspace check. Scheduling checkpoints (worker heartbeats
  and gate intent saved before a cargo child is spawned) rename into place and
  do not flush the ledger file or its directory on any thread. A background
  directory flush stalled the next governed Cargo spawn on saturated CI disks.
  Authority transitions still fsync before return. An exited local child is
  reaped before the next host-resource sample so a fast Cargo command cannot
  sit behind procfs or cgroup discovery. Worker admission is
  registered before the launcher acknowledges startup; the registry retains the
  owned join handle, cancellation token, epoch identity and live activity until
  settlement. Its bounded telemetry snapshot is updated from persisted gate changes,
  command/child lifecycle and streamed stdout/stderr activity; it carries stage, gate,
  command, current test executable, elapsed/last-activity time, gate counts, version/SHA,
  retry/fingerprint state and the newest eight redacted output lines. The ledger also
  persists a bounded, sequence-numbered `ReleaseProgress` projection: typed phase,
  headline, local/remote completion counts and concise milestones emitted at state
  transitions, local-gate start/settlement, resource deferral and remote-matrix
  changes. It is a display/read-resume projection only; RRC state and immutable
  evidence remain the sole progression authority. TUI chat/RUN and ACP status render
  this same projection and must not infer progress from elapsed time. The RRC-owned
  `host_resources` governor samples Linux physical/cgroup capacity, effective
  `MemAvailable`, reserve plus required gate headroom, memory PSI, swap growth,
  swap growth uses a minimum five-second observation window and retains the last
  measured trend between windows; startup warming is explicit and default pressure
  thresholds remain unchanged. Reported normal RAM requirements include the same
  near-full-swap and owned-tree burst margins enforced by admission. It observes
  zram logical/compressed/physical values, the owned process tree and managed-target
  filesystem; logical zram occupancy is never treated as physical RAM consumption.
  It reserves desktop headroom, exports one inherited
  `CARGO_BUILD_JOBS`/`RUST_TEST_THREADS`/`CARGO_TARGET_DIR` policy, serializes
  expensive compiler gates (including Windows `ERROR_LOCK_VIOLATION` contention),
  and refuses unsafe new work. Admission pressure or a
  critical owned-tree stop persists typed `ResourceDeferred` with the pending gate
  and telemetry, creates no source failure and spends no retry budget. The worker
  watches passively with bounded backoff, does not respawn the gate while pressure is
  unchanged, and returns automatically to `LocalVerification` only after three safe
  `Normal` observations. Defer and recovery each emit one milestone; polling emits
  none. Non-Linux production resource discovery is explicit unavailable: expensive
  local gates fail safely until a truthful platform backend exists. The explicit
  test-support permissive policy always uses a bounded deterministic synthetic snapshot
  so shared-runner pressure cannot control lifecycle acceptance; dedicated tests retain
  live Linux discovery and constrained-observation coverage, and the production policy
  is unchanged. TUI/ACP status
  queries this ownership rather than inferring work from admission prose or polling
  the filesystem. Local release children always use null
  stdin plus explicitly piped stdout and stderr; both streams are drained concurrently,
  terminal controls are removed, and
  only bounded/redacted records enter worker telemetry. No release child may inherit an
  interactive host terminal. Version preparation inventories every declared workspace member,
  requires one matching inherited
  package version and matching exact pins for all internal path dependencies across
  normal/dev/build/target tables, validates Registry release URLs, then applies the
  complete manifest/Registry mutation transactionally. It regenerates `Cargo.lock`,
  runs a workspace all-target check and locked metadata validation, and restores all
  original bytes if writing or validation fails; no later release gate may run on a
  partial or inconsistent version graph. Missing or failed
  local-gate evidence cannot support external-outage classification. A complete red
  matrix supplies every proven or strongly supported deterministic failure to one
  permission-aware AgentLoop turn in a uniquely named sibling user-state worktree.
  Promotion requires observed mutation and focused-command receipts, a non-empty diff
  and a clean controller workspace; the reducer transactionally records a repair for
  every admitted failure family against one changed commit before one fresh-SHA
  full-gate retry. A crashed repair may restart in another sibling without colliding
  with the epoch workspace. The native lifecycle regression
  owns a real child/descendant group or Job Object, proves cancellation reaps it,
  and starts a fresh process that reloads the same epoch/run/job identity; it runs
  on all five target families. Deterministic executor tests and controlled public
  GitHub fixtures do not substitute for production release publication.

- `dependency_setup` owns explicit native dependency consent, local engine health,
  fixed Podman package plans, bounded credential-free progress, setup serialization
  with `fs2`, user-wide runtime preferences and managed VM intent/recovery. It is
  never a model tool. Package-manager/official installer execution is an explicit
  setup-only exception to workspace I/O and helper-only networking; it cannot
  enable web tools or change permission grants. Reads create no preferences.
  Setup uses OS authorization; no provider or arbitrary shell input enters a plan.
  Cancellation stops between OS transactions and always awaits probe cleanup.
  A managed VM can auto-start on opted-in runtime use only after verified setup;
  normal tool execution never installs packages or initializes/downloads a VM.
  Browser readiness runs a bounded network-disabled, read-only container with no
  host mounts and verifies removal. Missing platform acceptance remains in the PRD.

- `lens_tools` owns shared native artifact-review and planning-interview tools:
  bounded question validation, workspace confinement, real Lens invocation and
  provider-visible feedback serialization. Both hosts use the same executor;
  hosts own URL presentation and the live question limit. ACP emits a review URL
  through its event sink; TUI also attempts desktop-browser launch.
  `tests/swarm_lens_browser.mjs` is driven by the explicit native worker browser
  gate in `swarm_adapter_tests`: actual Chrome submission, returned tool status,
  captured provider continuation and retained history, with isolated state.

- `swarm_settings` owns bounded workspace preferences and session-local explicit
  Save/Cancel drafts; reads and cancellation create no state. `swarm_service`
  owns one admitted native goal, real configured embedding/backend checks,
  existing tool/permission/progress composition, retained partial results and
  verified cleanup. Caller drop signals cancellation while the owned task settles.
  Cleanup uncertainty closes further service admission; preference alone never
  grants tool/network access. Explicit sandbox disable aliases and backend
  selection are honored; missing compiled backends and unknown selections refuse.
  Namespace probing runs off the async executor. Both hosts compose this behind
  default-off `swarm`.
- `swarm_inputs` captures bounded project files (4 MiB each, 64 MiB aggregate,
  4096 files, 64 levels) and materializes separate worker copies. Known project
  dotfiles (`.github`, `.cargo`, `.gitignore`, `.gitattributes`, `.editorconfig`,
  `.dockerignore`) are included; other hidden state and generated dependency/build
  directories are excluded, symlinks/special files and
  excessive inventories refuse. Source project files are never worker write roots.
  Explicit-run directories remain as output artifacts.
- `swarm_embedding` bridges a host's real configured embedding implementation on
  bounded blocking tasks; unfinished requests retain permits. Setup/recall share a
  process-wide four-request bound, including abandoned observers. No hash fallback.
  `swarm_journal` retains native worker/task identities and full returned histories
  in memory through observer cancellation, with explicit settlement observation.


- `src/swarm_adapter.rs` (feature `swarm`, default-off) adapts `WorkerPort`
  through the existing `AgentLoop`, not a second provider-stream loop. Hosts
  inject `WorkerFactory` (registry/config/credentials), real `ToolRegistry`,
  mode/permission, approval and optional progress ports. Role/task restrictions
  remove executable registrations and prefix gateways, not only schemas.
  Each instance refuses concurrent reuse and reports pending owned work until
  its native loop settles, even after observer drop; each task creates its own native
  runtime session. `into_instance_factory` produces the shared native pool
  factory: registry/config/permission/progress services are inherited, but busy
  flags and histories are independent. Session startup remains lazy inside the
  existing AgentLoop, not a second boot-time provider call.
  Completed/interrupted histories remain in memory, never
  implicitly persisted. Incomplete/cancelled outcomes cannot become success.
  `tests/swarm_adapter_tests.rs` uses a scripted provider and real read/write
  executors to check continuation, denial, restriction, cancellation, bounds
  and interrupted-history preservation with temporary workspace roots.
  Native Settings and TUI/ACP orchestration composition are wired behind the
  opt-in build; full cross-host acceptance remains in the repair matrix. ACP has
  `AcpEngineProgressPort`, so lack of progress support is not an exclusion.

- `src/swarm_sandbox.rs` (optional `swarm`) binds native worker factories to
  one shared `LeaseBook` and real `SandboxBackend`. Each role/instance receives a
  fresh canonical child directory, the same permission port, and a scoped native
  command route. Worker roots remain output artifacts; cleanup never deletes
  results. Existing/symlinked roots refuse before provisioning. Filesystem
  capability is mandatory; network access also requires full capability and
  explicit grant provenance. Explicit shared mode uses one existing Docker
  supervisor and disjoint `w/<worker-id>` roots. `swarm_shared_scope` invokes
  the bundled checksum-pinned setpriv utility with Landlock, dropped capabilities,
  no-new-privileges and unique non-root worker credentials. A real confinement
  probe is mandatory; the namespace backend retains its isolated one-run protocol.
  Each command verifies all processes with its worker UID have exited and restores
  the original bind-mount owner before native tool continuation (rootful Docker
  and rootless Podman mappings differ). Ownership records stay supervisor-private. The bundled init reaps
  orphans. Cleanup uncertainty quarantines the scope even if the supervisor is
  subsequently removed. No new Rust syscall or sandbox backend is introduced.
  Scope preparation owns cancellation cleanup even when its blocking observer
  drops. Native backend work runs outside book/metadata locks; uncertain provision
  or teardown retains quarantine. Detached command ports keep their leases.
  One-run backends rotate supervisors only after verified teardown while retaining
  the worker reservation across continuations. Dedicated roots explicitly permit
  private SELinux labels; ordinary project roots are never relabeled by this path.
  `settle`/`shutdown` and bounded error diagnostics expose actual cleanup outcomes;
  consuming backend teardown offers no assumed safe retry. The factory's
  `with_sandbox_leases` applies to startup, growth and replacement. Host commands
  and Settings remain acceptance-gated; the adapter alone is not product activation.
  `src/swarm_sandbox_tests.rs` verifies no-state refusals, unused preparation,
  uncertainty and symlink canary preservation. `tests/swarm_native_hive.rs` has
  explicit namespace/container gates for real 1+3-worker execution, two approved
  commands per worker, scoped permission traces, grounded synthesis, scale,
  retirement, replacement and verified shutdown. Unavailable capabilities fail
  these explicit gates instead of skipping their bodies.

- `src/sandbox_backend.rs` owns the shared native command adapter used by TUI
  and ACP. Current backends execute Linux payloads using absolute `/bin/sh`,
  including containers hosted on Windows/macOS. Nonzero command exit preserves
  stdout/stderr as failure; verified cleanup never converts it to success. Teardown failure overrides run success/cancellation, preserves available
  command output or the run error in diagnostics, and permanently quarantines that
  port against subsequent provisioning. Cancellation is rechecked after provision
  and after cleanup. Already-admitted concurrent operations are not retroactively
  cancelled. Unwinding backend panics during provision/run/teardown construction
  or polling become errors and permanently quarantine the route. A run panic still
  reaches explicit teardown. Panic payloads are excluded from tool diagnostics;
  the process panic hook is unchanged. Abort/double-panic, blocking hangs and
  process-tree verification remain acceptance gaps. No automatic cleanup retry or
  quarantine reset is exposed. `src/sandbox_outcome_tests.rs` checks outcome
  arbitration and quarantined refusal; `src/sandbox_panic_tests.rs` covers provision
  construction/poll panics, phase guards and ordinary refusal without quarantine.
- `tests/swarm_native_hive.rs` runs real Hive/native-factory/AgentLoop/read_file
  composition under all four topology configurations. Three independent provider
  sessions must overlap at a barrier; tool results feed continuation and synthesis,
  and temporary roots must remain free of implicit durable state. Provider and
  embedding fixtures are test-only. This is not supervisor or native host activation
  acceptance, nor proof that topology edges constrain all dispatch.

- `src/web_settings.rs` owns explicit workspace web-setting saves and shared
  `/web` controls. Atomic private JSON snapshots preserve existing TOML and
  its initial allowlist/budget values. Read/status/cancel never create state.
  Docker/Podman discovery honors the operator override, runs only in explicit
  settings/setup flows, has a five-second timeout, and normalizes bare Podman
  IDs to `sha256:`. Inspection spawn failures retain OS error kind/code without
  leaking executable paths or falsely diagnosing daemon availability.
  `setup_driver` reads only the executable-adjacent bundle
  (or explicit `AGENT_VESPER_BUNDLE_DIR`), verifies SHA-256 before importing,
  caps import at 180 seconds, and verifies the exact bundled image ID after
  import. This explicit installer/settings operation is the exception to
  workspace-root I/O confinement; it is never a model-facing tool. Import-only
  setup launches no container. Guided `dependency_setup` adds separate confirmed
  runtime installation and contained readiness probes. Both hosts expose `--setup-web-driver`
  before provider boot and `/web setup` for workspace selection. Web runtime
  CLI selection shares the saved runtime and explicit connection; health-aware
  discovery checks Podman when installed Docker is unavailable. No sandbox or
  private-address protection may be switched off by these controls.
  An enabled runtime without an explicit image override uses the bundled
  immutable image ID; activation never requires copying a digest by hand.

- `src/skill_model_selector.rs` owns the explicitly enabled configured-provider
  selection call: empty tool registry, one iteration, 20-second deadline, cancellation,
  1024 output-token request and 4096-byte streamed text/reasoning cap. It sends only
  bounded task/metadata, rechecks settings/catalog before and after dispatch, and
  reports elapsed time and provider-reported usage. Provider/parse/budget failure
  may use observable lexical fallback; cancellation or stale decisions withhold.
  No selector prompt, body or reasoning is persisted.
- `src/skill_routing_settings.rs` owns workspace Skills preview preferences,
  explicit atomic saves, no-write reads and the shared host routing bridge.
  Per-skill disables apply in both modes; corrupt preferences withhold activation
  rather than resurrect disabled skills. `/skills settings` uses the domain parser;
  only `save` mutates preferences. The bounded transition helper permits one
  refinement with new task information and never executes tools.

- `src/lib.rs` owns the shared service, bounded durable-store wiring, and
  provider-worker delegation boundary.
- Frontends own provider selection, approval UI, ACP/TUI protocol mapping, and
  session lifecycle; they inject a `WorkerFactory` when nested work is allowed.

## Local Contracts

- Shared live release snapshots project same-epoch persisted milestones and gate
  settlement immediately, retaining trusted live acceptance-case counts. A blocking
  gate must not freeze TUI/ACP progress at its preceding milestone.

- `release_closeout` owns existing-registry-PR delivery and local release reports.
  Publication is an immutable checkpoint, not a worker termination condition.
  The native worker continues through current-main matrix settlement, expected-blob
  registry update/readback and a durable completion receipt. Matching published
  requests resume that checkpoint without a new epoch/version/tag or retry reset.
  Uncertain registry writes are reobserved, never replayed; concurrent edits refuse.
  Reports retain failures, focused attempts, reserved admission counters, retry
  budgets, transitions, metrics and changed paths; later-main links name the actual
  distinct report. Final summary delivery requires the receipt. Post-publication evidence links stay
  local until the next authorized code push; closeout never installs an update.

- ADR 0029 adds empty-path opt-in as pending automatic PRD enrollment. Both hosts
  capture the original user request with `activate_for_prompt`; `acceptance_enroll`
  independently reviews the candidate scope, freezes it once and remembers its
  workspace path under normal mutation permissions. Pending enrollment cannot
  complete. No model tool can disable or replace an enrolled objective.
  Enrollment is bounded by one wall-clock window (300 s production) shared by
  every nested phase — scope review and the contract ladder (max 2 proposals);
  window expiry, ladder exhaustion and scope refusal each fail loudly with
  do-not-retry guidance, save nothing and leave the gate unenrolled. Nested
  reviewers stream `AgentProgressEvent::Status` stage lines so enrollment is
  never silent; `acceptance_tests` pins the window, the ladder cap and the
  `#[cfg(test)]` ceiling override defaults to the production value.
- ADR 0028: `acceptance`, `acceptance_snapshot`, `acceptance_runner` and
  `acceptance_settings` own original PRD/project-rule capture, independent read-only
  review, exact Rust test execution, private in-memory receipts and native controls.
  Snapshot bounds are 128 MiB/16384 files/64 levels; symlinks, special files and private
  .env inputs refuse. Default reads write nothing; settings saves/audit export are
  explicit. Resume imports history only, never verification. `acceptance_tests` covers
  real defective/repair execution and adversarial failures.
  Saved opt-in discovery treats unresolved or absent workspaces as inactive, so
  ordinary ACP sessions remain usable. Explicit enrollment/saves require an existing
  absolute workspace; unreadable or malformed existing settings still refuse.
  Verification forwards only the explicit toolchain environment allowlist, including
  Windows SDK/MSVC discovery roots; those same values bind the receipt environment.

- Every filesystem path is confined to the caller's primary workspace root
  before access.
- `src/slash_commands.rs` owns store-backed slash-command execution for the
  ACP and TUI compositions: `execute_slash_command` resolves catalog commands
  against a host-supplied `SlashCommandContext` (durable `MemoryStores`,
  model/plan labels, mode), `/help` renders the oracle fixture text
  byte-exactly, `/curator` runs deterministic curation against the memory
  store, provider-facing switches validate into a `SessionOverrides` payload
  the host applies at its own provider boundary. The payload may retain
  bounded provider-namespaced configuration for descriptor-driven ACP
  controls; it never interprets concrete provider semantics. Commands only a
  frontend can serve (conversation state, workflow turns, live provider
  quota) return `SlashCommandOutcome::Host` passthrough. The catalog and
  parser delegate to `vesper-domain::slash_commands`.
  `/max-iterations` accepts `enable`, `disable`, or `1-1000`; the optional
  user cap is disabled by default and never removes the agent loop's ultimate
  safety ceiling.
- `MemoryStores::parity_report` owns the shared read-only repository,
  meta-learning, observability, and journey renderings used by ACP and
  available to the TUI composition.
- `src/host_commands.rs` executes the store-backed host commands
  (`/checkpoint`, `/rollback`, `/undo`, `/export`, `/sessions`, `/lineage`,
  `/ci`, `/plugins`, `/mcp`) on `HarnessToolService` against the same
  durable checkpoint/MCP roots the TUI drains through, with byte-identical
  response formats, so the ACP composition reaches TUI parity without
  duplicating drain logic. `/checkpoint` and `/lineage` seed a session
  lineage record named for the host session id on first use.
- Checkpoints/lineage are OPT-IN per composition:
  `HarnessToolService::new_with_checkpoint_gate(..., checkpoints_enabled)`
  (the plain `new` keeps the historical enabled default for the TUI). A
  gated service never creates the checkpoint or lineage directories at
  construction, never spawns the cron scheduler, and answers the five
  checkpoint-family commands with the truthful
  `AGENT_VESPER_ENABLE_CHECKPOINTS` opt-in notice. The ACP host builds
  gated-by-default (root AGENTS.md contract); `/ci` and `/export` remain
  available either way.
- `MemoryStores::open_default` opens the project root
  (`AGENT_VESPER_MEMORY_ROOT` → `.agent-vesper/memory/`) plus the
  cross-project global skill layer (`AGENT_VESPER_GLOBAL_MEMORY_ROOT` →
  `~/.agent-vesper/memory/`); `open_at` is the explicit-root constructor
  compositions with their own root resolution share so the TUI and ACP can
  never drift on store-open semantics again.
- Durable roots are supplied by the composition boundary and default to the
  `.agent-vesper/` layout; no credentials are persisted by this crate.
- `src/scope_holder.rs` resolves the shared VRO workspace scope once at host
  boot. The TUI uses its default writing stamp policy; ACP must pass
  `StampPolicy::ReadOnly` unless `AGENT_VESPER_ENABLE_SCOPE_STAMP=1`, so an
  editor-spawned process never creates `.vesper-scope-id` in a project by
  default. The policy changes persistence only, never the resolved id.
- The service exposes the same hosted tool definitions and behavior to ACP and
  TUI, avoiding frontend-specific parity drift.
- `HarnessToolService::orchestrate_skills` is the shared ADR 0024 host bridge.
  It supplies current tool capabilities and platform to `vesper-memory`, owns
  bounded outcome feedback, and shares that tracker with read-only workers.
  It never turns selection into permission.
- Provider-backed worker outcome rendering preserves partial content and the
  typed interruption diagnostic from the shared agent loop; workers never
  silently report an interrupted generation as complete.
- Provider-backed workers render ultimate iteration-cap outcomes explicitly;
  unfinished native-plan work is never reported as completed.
- First-party MCP presets are not persisted and cannot be shadowed by custom
  registry entries. Z.ai Search, Reader, and Vision definitions inherit the
  protected preset's `zai` provider scope; both discovery and direct/gateway
  execution repeat that eligibility check. Search uses the documented remote
  name `webSearchPrime`. Web and reader calls resolve Z.ai credentials on
  demand through the host's adapter-owned credential bridge, then retain the
  legacy environment fallback; vision paths are workspace-confined; browser
  actions are an explicit allowlist and never arbitrary JavaScript evaluation.
- All output, source scans, batches, workflow depth, and worker actions remain
  bounded. When a provider-backed worker is supplied, the host starts a
  one-second polling scheduler that claims due cron jobs, executes them, and
  persists bounded status/output; dropping the service aborts that scheduler.
- Daemon locks and PID watchers share one bounded native liveness probe:
  `/proc/<pid>` on Linux, `kill -0` on other Unix hosts, and `tasklist` on
  Windows. If the probe cannot run it conservatively treats the PID as live,
  preventing accidental lock takeover or false watcher fires.
- No protocol, provider-wire, UI, SQLite, or live-provider dependency is
  allowed here.
- **Phase 3 deferred loading + MCP gateway.** `mcp_list_tools` now translates
  discovered MCP tool descriptors into `ToolDefinition`s named
  `mcp__<server>__<tool>` (with `defer_loading = false`) and returns them via
  `ToolResult::with_injected_tools` instead of a stringified text payload.
  `McpGatewayExecutor` (registered under the `mcp__` prefix by
  `HarnessToolService::build_default_registry`) parses the call name back into
  `(server, tool)` and dispatches through the service's shared `McpSession`.
  `build_hosted_registry` preserves this gateway through host wrappers, including
  TUI direct/VRO/ReAct. Discovery, browser presets and explicit calls share one
  owner. `fork_mcp_session` and read-only workers receive fresh MCP owners;
  permission restrictions still remove gateways. Unscoped discovery reports
  per-server failures without hiding healthy servers; `isError` is failure.
  `src/mcp_session_tests.rs` verifies real fixture-process continuity/isolation.

## Work Guidance

- Add provider-neutral core tools to `vesper-agent`; add durable or host-bound
  tools here and inject them through `ToolRegistry::with_service`.
- Preserve the fail-closed permission gate in the agent loop; this service
  never bypasses it.

- `src/web_service.rs` (VRO-14 PR-5) hosts the five opt-in web tools
  (`web_fetch`, `web_scrape`, `web_map`, `web_crawl`, `web_interact`) as
  `ToolExecutionClass::Network` with `defer_loading = true`. Both hosts
  explicitly attach the boot scope through `with_web_scope`. The hosted
  service itself advertises and dispatches web tools so direct TUI wrappers,
  ACP registries, discovery, and worker services all reach the same executor.
  With no effective web scope (or
  `enabled = false`) zero web tools register and the registry path is
  byte-identical to the pre-web build. The process-global web holder
  (`web_service::holder`) mirrors the firewall/sandbox holders.
- Passive web tools execute through the shared service's `FetchTransport`;
  production uses the sandbox helper on a blocking worker, tests use offline
  pages. Registry builds reuse the service Arc. `search_tools` searches eligible
  web definitions and injects their schemas for subsequent model requests.
- `src/web_runtime.rs` lazily owns a single sandbox route used by fetch,
  ephemeral rendering, and persistent browser interaction. Initialization
  requires a digest-pinned driver image; no host HTTP/browser fallback exists.
  `AGENT_VESPER_SANDBOX=off` refuses initialization, including web execution.
  Browser interaction has a separate opt-in. A failed pipe invalidates the
  session, and render escalation never modifies the interactive session.
  Four shared runtime permits cap concurrent sandbox operations. Fetch and
  render share one deadline; crawl includes seed work in its wall-clock budget.
  Argument validation precedes external execution; closing an unopened browser
  is an idempotent no-op without a daemon/image probe.
- Map merges bounded sitemap/index discovery with page links. Current
  acceptance status remains in `docs/foundation/vro14-gap-audit.md`; a passing
  component test alone is not permission to advertise full PRD completion.

## Verification

- Shared selector composition fixtures use the actual host OS for preparation
  and resolution; a mismatched platform must still invalidate a pending selection.

- `tests/watcher_latency_gate.rs` serializes sibling tests around process-wide
  descriptor measurements; sweep/input concurrency remains inside the measured test.

- Driver CLI fixtures use the immutable executable `tests/container_cli_fixture.sh`
  through per-test symlinks with response/load state in temporary roots. Do not
  execute freshly written test scripts: concurrent fork/exec can retain a writable
  reference and trigger Linux ETXTBSY even when the writing thread closed its file.
  Never rewrite a just-executed inode or add production retries to mask this fixture
  race.
- The registered-controller lifecycle fixture injects a test-only permissive
  resource policy directly so live workstation pressure cannot replace its
  worker-ownership assertions with a governor deferral. In `cfg(test)` only, that
  explicit policy may use bounded synthetic capacity when the host has no production
  discovery backend; default/production policy still fails closed. Dedicated
  constrained-host and resumable-deferral tests retain production resource-policy
  coverage.
- Run `cargo test -p vesper-harness`.
- Run `cargo test -p vesper-harness --test vro13_e2e` (VRO-13 PR-8
  cross-feature fixture: watcher fire → bounded turn → composed
  firewall → opt-in sandbox route → scope-keyed slot ledger; run with
  `--features docker` to include the feature-gated cold-start arm).
- Run `cargo xtask architecture` and `cargo xtask verify`.

- `src/bridge_service.rs` (behind the default-off `bridge` feature)
  composes the pure `vesper-bridge` core (VB-PRD-001 Phase 2): the
  8-tool Bridge surface, single-session ownership and denial text. No
  adapter/transport/I/O lives here; `with_bridge(false)` or the feature
  off leaves zero bridge tools and no bridge state (BR-30/NF-01).
  Evidence: `src/bridge_service_tests.rs` (10 integration tests,
  feature-gated).

## Child DOX Index

No children.
