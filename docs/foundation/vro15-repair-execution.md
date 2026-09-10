# VRO-15 approved repair execution

## Scope and authorization

Alex approved full completion of F01–F18 in `vro15-gap-audit.md`, including native
Settings, TUI and ACP, retaining the accepted snapshot-reader ledger design.
Implementation is authorized. Release/tag/registry operations are not authorized.
Alex additionally requested a VesperLens feedback and provider-neutrality audit,
including native OpenAI API/subscription rather than assuming Z.ai-only behavior.

## Architecture and execution rules

- `vesper-swarm` remains provider-neutral coordination; execution composes the
  existing AgentLoop, permissions, sandbox, runtime sessions and credentials.
- Full-ledger coherent snapshot readers are required; no silent mutex downgrade.
- Runtime activation remains default-off until acceptance passes.
- Add desired-behavior regressions and observe failure before fixing each invariant.
- Existing audit probes assert historic defects, not desired behavior; keep them
  separate from acceptance. No live providers/user-state writes in verification.
- Follow the root-to-target DOX chain and update ownership/contract documentation.

## Ordered work packages

1. Correct acceptance status and establish requirement/test traceability.
2. Correct oracle scoring and hard eligibility (F05).
3. Connect actual turn cancellation and bound late outcomes (F06).
4. Repair pool lifecycle, reservations, capability admission and replacement (F07).
5. Introduce real independently owned worker instances (F01/F07).
6. Repair topology and partition election policy (F08).
7. Enforce pairwise sandbox path isolation (F09).
8. Quarantine failed teardown and verify real supervisor cleanup (F10).
9. Repair bus terminal states, wakeups and atomic broadcast (F11/F12).
10. Bound bus ACK/byte/TTL state and make coordination time explicit (F12/F18).
11. Harden HNSW snapshots, RNG state and continued insertion (F13/F14).
12. Repair pruning/math and measure scale (F15).
13. Implement coherent immutable ledger generation publication (F16).
14. Complete whole-ledger persistence, filters, eviction and atomic transfer (F16).
15. Implement validated bounded task decomposition/state machine (F01/F03).
16. Compose real parallel dispatch, leases, heartbeat and shutdown (F01/F03).
17. Replace text-only worker adapter with existing AgentLoop composition (F02).
18. Verify evidence-fed synthesis through the same composed engine (F01/F17).
19. Add persisted native Settings activation and shared command service (F04).
20. Wire both hosts with scoped protocol/interaction acceptance (F04).
21. Strengthen architecture guards, test contracts and DOX (F17/F18).
22. Run canonical/default/all-feature/MSRV/supply-chain/platform gates.
23. Trace and repair VesperLens interview notes/action delivery and provider-neutral
    TUI/ACP execution, including OpenAI; preserve choices AND overall feedback.

## Baseline and progress

- Starting checkout `f662519`; prior audit documentation changes preserved.
- Prior audit measured 1,893 all-feature / 1,869 default / 163 swarm passing tests.
- Implementation in progress; no finding is closed until desired-behavior tests pass.
- F05 scorer repair: `crates/vesper-swarm/src/hive/assignment.rs` now scales
  the entire adjusted base by health. `select_best` excludes incapable,
  zero-capacity, saturated, dead and invalid-metric candidates. Four desired-behavior
  regressions in `tests/assignment_regressions.rs` pass; the hive still must
  consume selection and dispatch independent workers before integration is closed.
- Local verification after scorer repair: `cargo test -p vesper-swarm` passes
  167 tests (one ignored doctest); crate all-target clippy with `-D warnings`,
  `cargo xtask architecture`, `cargo xtask naming-guard`, and `git diff --check`
  pass. Full workspace, MSRV, host/provider and platform acceptance are not
  established by these checks.
- VesperLens source inspection: trusted chrome submits notes and answers together;
  the TUI passes returned feedback through the shared context formatter. No
  provider-specific cause or missing-note reproduction has yet been established.
  Browser submission, server receipt and provider serialization still need
  end-to-end regression coverage; source inspection is not delivery acceptance.
- Read-only delegated investigation could not start: the harness worker returned
  OpenAI HTTP 400 / InvalidRequest. No retry/live provider experiment was performed.
  This is observed worker-path evidence, not proof of the VesperLens root cause;
  local source/fixture investigation continues without delegation.

## Verified foundation repair milestone

- `hive/timeout.rs`: boundary-owned signal passed into future construction and
  actual `WorkerPort`, cancellation on caller drop, deterministic deadline
  precedence, bounded grace and late-success refusal. Five tests pass. This is
  helper acceptance, not evidence that Hive dispatch uses it yet.
- `pool.rs`: three regressions failed before repair and pass afterward: repeated
  initialization (10 workers versus expected 2), replacement at maximum capacity
  (0 versus expected 2), and incapable growth. Independent worker factories,
  shutdown/reservation semantics and full Hive composition remain open.
- `sandbox.rs`: three regressions failed before repair and pass afterward:
  nested/ambiguous path refusal, checking every live shared member, and retaining
  capacity after failed release. Quarantine does not prove backend cleanup or
  provide retry/recovery; these remain outstanding composition requirements.
- `bus.rs`: three regressions failed before repair and pass afterward: parked
  close/unsubscribe receivers, atomic broadcast refusal, and expired/unsubscribed
  ACK cleanup. Broadcast recipients are sorted and admission preflight is locked.
  Immediate close drops queues/debts and wakes waiters; notification registration
  precedes queue checking. ACK lifetime/count and byte/subscriber/TTL limits remain
  open, so F12 is not closed.
- Verification: `cargo test -p vesper-swarm` passes 176 tests, one ignored doctest;
  crate all-target Clippy with `-D warnings`, `cargo check --workspace --all-features`,
  architecture, naming guard, and diff whitespace checks pass locally. No live
  provider calls or external user-state writes were used in these checks.
- The accepted full scope is unchanged. Topology, HNSW, immutable ledger,
  orchestration, native Settings, host parity and VesperLens delivery acceptance
  are not completed by this milestone. Root/parent crate DOX boundaries remain
  unchanged; the nearest swarm DOX records revised helper/pool/bus/lease contracts.

## HNSW loader hardening milestone

- `ledger/hnsw.rs` now checks allocation sizes against actual input bytes,
  checked vector-section arithmetic, unique IDs, self/duplicate edges, finite
  vector values, caller seed/capacity, entry/header maxima and neighbor layers.
  The legal level 16 is accepted. Four `hnsw_regressions.rs` tests pass; header
  level, caller policy and nonfinite-vector regressions failed before repair.
  The byte-budget test already returned an error before repair and does not
  alone prove the allocation bound; that bound is also source-checked.
- Existing 13 HNSW integration tests and crate all-target Clippy pass after
  hardening. Full snapshot acceptance remains open: exact RNG/raw/config
  persistence, normalization/pruning, corruption/property and scale evidence.

- Final local recheck for these foundation milestones: 180 swarm tests pass,
  one doctest ignored; all-feature workspace compilation, crate all-target
  Clippy, architecture, naming guard and diff whitespace checks pass. This is
  still partial repair evidence, not completion of the approved plan.

## Topology and snapshot publication milestone

- Three topology regressions reproduced and repaired: disabled failover surviving
  repeated rebalances, eligible-only successors, and mutation-free rejection of
  an inconsistent join ledger. Partition rebuild/status-change acceptance remains
  open; F08 is not fully closed.
- HNSW snapshot version 2 stores raw finite vectors, exact RNG state and semantic
  configuration. Version 1 is explicitly refused. Three regressions reproduced
  and repaired: continued-insertion byte equivalence, over-fetch configuration
  agreement and normalization at finite extremes. Raw signed-zero and mixed-zero
  seed round-trip also pass. Adjacency pruning now ranks against its owner.
- Ledger publishes coherent immutable full generations with pinned `arc-swap
  1.9.2`; readers do not take the writer mutex. Hybrid components use one
  generation; retained generation and transactional transfer rollback tests pass.
  Transfers reuse original embeddings. Generation cloning cost still needs scale
  acceptance; no mutex-based reader downgrade was made.
- `VSWLEDG1` whole-ledger snapshots carry HNSW v2 plus structured entries and
  sequence; identity/count/confidence validation and round-trip/truncation tests
  pass without embedding calls. Snapshot byte limit is 64 MiB; pre-serialization
  allocation budgets, eviction, extended filters and broader corruption tests
  remain open.
- Latest local verification: 190 swarm tests pass (one ignored doctest), crate
  all-target Clippy, Rust 1.88 locked crate compilation, all-feature workspace
  compilation, architecture, naming guard and diff checks pass. Pinned
  `cargo-deny 0.20.2` was installed into a temporary tools root; `cargo deny
  --all-features check` passes advisories, bans, licenses and sources, including
  the new dependency.
- Root/parent domain boundaries and child indices remain unchanged; closest
  swarm DOX records the changed format, dependency and publication contracts.

## VesperLens interview verdict clarification

- Confirmed ambiguity: browser `Send answers` used the artifact `modify` action,
  causing the shared formatter to say `NEEDS MODIFICATION`. It now uses typed
  `answer` and a planning-submission header; explicit notes/choices remain intact.
  This does not authorize tools or automatically interpret scope choices as
  implementation approval.
- JSON round-trip and authenticated loopback POST → feedback queue → model-context
  tests preserve both an approval note and its selected scope. 54 focused Lens
  tests and agent all-target Clippy pass; all-feature workspace compilation passed
  after the new action variant. Missing-note loss/provider coupling is still not
  reproduced, so this is not a claimed fix for an OpenAI-specific delivery bug.
- Both real-browser scripts pass in installed Chrome, including notes/answer
  preservation and console/network assertions. A temporary Node 22.23.2 runtime
  was downloaded from the official distribution and SHA-256 checked against its
  published manifest; Playwright 1.51.1 was installed under a temporary test root
  with install scripts disabled. No production activation/runtime changed.
  Adapter-wire acceptance remains open. Existing ACP browser-only exclusion
  has been evaluated and remains an explicit integration/design gap.
- Workspace verification now passes 1,922 tests (21 ignored), all-target/all-feature
  Clippy with warnings denied, Rust 1.88 locked all-feature workspace compilation,
  formatting, architecture, naming guard and whitespace checks. The naming guard
  required removing a pre-existing brand-style reference shifted by the DOX
  insertion; its baseline was not weakened. Five-target CI and remaining feature
  acceptance are not established by these local checks.

## Bus resource bounds milestone

- Hard payload, aggregate queued byte, identity, subscriber/filter, TTL and
  configured message-count ceilings are documented by swarm DOX. Byte pressure
  uses lower-priority eviction, with atomic count/byte broadcast preflight.
- Pending ACK count is capped at bus capacity. Saturation refuses delivery
  without popping it; debts lazily expire at original TTL. Four desired-behavior
  regressions and the existing 28 bus tests pass; payload/TTL and ACK-cap tests
  failed before repair. Explicit time injection and expanded concurrency/byte
  accounting acceptance remain open.
- Post-bounds recheck: 1,926 workspace tests pass (21 ignored); full Clippy,
  Rust 1.88 locked all-feature workspace compilation, architecture, naming and
  whitespace checks pass. Existing cross-platform and feature gaps remain open.

## Centralized topology lifecycle repair

- `TopologyManager::rebalance` now reconciles leadership before edge wiring;
  disabled-failover vacancy suppresses the fallback hub. Automatic node updates
  rebuild wiring instead of changing leadership alone.
- `tests/centralized_lifecycle_regressions.rs` first failed on the vacant-hub
  assertion, then passed across manual/automatic rebalance and enabled/disabled
  failover, including idempotence. Full swarm tests, crate all-target Clippy,
  workspace all-feature tests, architecture, naming guard and whitespace checks
  pass locally. Dependency-tree inspection shows only the documented crate deps.
- This closes the centralized edge-ordering defect, not partition lifecycle
  acceptance or F01–F18 as a whole. Partition eligibility/vacancy preservation,
  resource/scale work, real orchestration, Settings/host composition and Lens
  adapter-wire acceptance remain open. No release or registry action occurred.

## Partition, pool, snapshot and hive safety continuation

- Partition status/removal policy now selects only eligible leaders in admission
  order. Disabled-failover dissolution persists member suppression across rebuilds
  and serialization until explicit election. Two regressions first failed on
  initializing leadership/range-order selection, then passed across all flag and
  status/removal combinations. Placement tests now activate members explicitly.
- Pool caller-drop and health-cancelled-success regressions failed before repair.
  Close/drop now signal actual workers; late cancelled receipts cannot succeed.
  Independent worker creation and cleanup/replacement composition remain open.
- Whole-ledger export preflights HNSW encoded length and streams borrowed entries
  into a capped memory sink instead of cloning the log and allocating unbounded
  JSON. Exact-byte-limit and format-equivalence tests pass. Search uses sparse
  visited membership instead of full-index scratch per layer; existing 10k recall
  and runtime checks pass. Large-generation cloning, eviction and filter gaps
  are not closed by this change.
- Hive now bounds input admission, checks all three phases through the shared
  deadline/cancel helper, rejects unsuccessful receipts, and retains interrupted
  goals without automatic replay. All-phase hanging-worker, caller-drop and
  failure-receipt tests pass. Synthesis includes bounded completed-task evidence.
- Strict JSON decomposition replaces the scripted task-count fallback; actual
  task prompts, capability requirements and acyclic indexed dependencies drive
  execution. A two-class test proves capability-based target selection, dependency
  ordering/context and actual worker provenance. Independent worker overlap and
  lease/AgentLoop composition remain open, so synthetic test names now say
  "driver turns", not "three drivers".
- `xtask/src/swarm_gate.rs` enforces optional, explicit swarm activation and
  transitive default exclusion, including renamed dependency bypass regressions.
  Architecture and naming checks pass. Crate/root ownership docs no longer claim
  absent clock usage or that pooling/ledger modules are future work.
- Changed scopes remain swarm foundations, verification and evidence; native host
  activation and VesperLens adapter-wire work have not been completed here.

## Native execution adapter and caller-time continuation

- The stream-only harness adapter was replaced, not retained as a production
  fallback. `ProviderWorkerPort` now takes the existing `WorkerFactory`, real
  tool registry, mode/permission and approval port, then runs `AgentLoop` with
  native session creation and the same configured firewall/sandbox. Optional
  progress delivery and retained in-memory histories preserve interrupted text.
- `ToolRegistry::restricted_to` narrows executable registrations and drops prefix
  gateways; task requirements cannot widen the role's tool set. Six adapter tests
  use real confined executors with scripted provider traffic: two-turn read/result
  delivery, read-only write denial, unknown-tool rejection under Bypass,
  pre-dispatch bounds/cancellation, incomplete-terminal refusal and interrupted
  text/history retention without ambiguous replay. No live provider calls.
- Host DOX now removes the unsupported ACP progress exclusion, citing existing
  `AcpEngineProgressPort`. Settings and actual TUI/ACP swarm activation remain
  open; neither host is advertised as having a completed swarm surface.
- Bus queue and ACK expiry support `BusClock` injection; an exact-deadline test
  runs without sleeps and across cloned handles. Pool configuration now refuses
  more than 4,096 workers and timing budgets over 24 hours; oversized task budgets
  are refused before worker acquisition.
- Latest pre-final native-adapter workspace check: 1,945 tests pass, 21 ignored;
  all-feature/all-target workspace Clippy, Rust 1.88 locked workspace compilation,
  formatting, architecture and naming guards pass. Subsequent bus/pool additions
  passed their focused suites and require the next workspace recheck. Naming
  baseline was not weakened; two shifted stale branded comments were removed.
- Next execution work remains independent asynchronous worker creation, actual
  concurrent scored dispatch with lease/health lifecycle, ledger scale/filters/
  eviction, remaining lease bounds, native Settings/host composition, and Lens
  provider-wire/ACP acceptance. The accepted coherent reader design is retained.

## Verification checkpoint after native adapter and time bounds

- Full all-feature workspace recheck: **1,947 passed, 0 failed, 21 ignored**.
  Workspace all-target/all-feature Clippy, Rust 1.88 locked compilation,
  formatting, architecture, naming guard and whitespace checks pass locally.
- The preceding workspace run failed the existing
  `web_settings::tests::driver_detection_pins_only_valid_installed_ids` test:
  the bare valid SHA-256 fixture returned the generic container-engine unavailable
  error. Focused rerun and full workspace recheck passed. Underlying intermittent
  subprocess failure is not diagnosed; this is not uniformly clean-run evidence.
- Five-target CI, feature acceptance and all remaining plan items are still open.
  No commits, release tags, registry actions or live provider calls were performed.

## Concurrent dispatch and embedding boundary milestone

- Hive dispatch now runs ready tasks in concurrent waves across distinct execution
  ports, with one in-flight task per port even if several classes alias its Arc.
  Dependency outputs must be recorded before dependent dispatch. Evidence is
  collected in deterministic wave admission order. Failure drops sibling futures,
  signalling their real cancellation guards; interrupted goals cannot auto-replay.
- `hive_concurrency_regressions.rs` uses a three-party barrier (not sleeps or a
  synthetic count) to prove overlap. It also covers dependency context, failure
  cancellation and alias capacity. These are offline execution-port tests, **not**
  proof of independent pool factories, supervisor leases or host acceptance.
- Driver and synthesis ledger writes now bound the embedding await by the issuing
  role deadline. The hanging-embedding regression verifies no ledger publication
  and no replay after refusal. Detached embedding cleanup remains port-owned.
- Worker factory/lifecycle composition, resource acceptance, Settings, host wiring,
  VesperLens wire tests and final F01–F18 reconciliation remain open.

## Lease resource admission milestone

- `sandbox.rs` caps boundaries, members and queued waiters at 4,096 each, IDs at
  256 bytes, grants at 1,024 bytes, write paths at 4,096 bytes, wait budgets at
  24 hours, and retained diagnostics at 64 × 4,096 bytes. Counter exhaustion
  refuses before backend acquisition rather than wrapping an identity.
- `sandbox_tests.rs` verifies refusal without backend calls, exact queue capacity
  and cancellation reclamation, and shared joins respecting the global member cap.
- This does not make synchronous backend acquire/release preemptible. Real bounded
  supervisor cleanup and worker lifecycle composition remain acceptance gaps.

## Verification checkpoint after concurrent waves and lease bounds

- `cargo test --workspace --all-features`: **1,954 passed, 0 failed, 21 ignored**.
- Workspace all-target/all-feature Clippy with denied warnings and Rust 1.88
  locked all-target/all-feature compilation pass. Formatting, architecture,
  naming guard and whitespace verification pass.
- Swarm component tests include seven added regressions for concurrent waves,
  dependency context, sibling cancellation, alias capacity, hanging embeddings,
  and bounded lease admission/queues/members (some tests cover multiple assertions).
- DOX pass updated `crates/vesper-swarm/AGENTS.md` and this evidence domain.
  Root and parent contracts/indexes were intentionally left unchanged: ownership,
  dependency direction and host activation gates are unchanged.
- Full repair acceptance remains open; default-off activation is unchanged.
  Five-target CI and real supervisor/native-host acceptance were not executed.

## Independent worker factory milestone

- `pool_instances.rs` adds the provider-neutral `WorkerInstanceFactory` and
  `WorkerPool::with_factory`. Initialization, growth and replacement create real
  independent ports with cancellable bounded async boot, transactional publication,
  alias/zero-capacity refusal and monotonically consumed identities. Failed slots
  still held by a lease cannot be replaced. `scale_async` handles factory pools;
  synchronous scaling refuses them. Legacy shared-port construction is compatibility
  behavior, not independent-worker acceptance.
- `pool_instance_regressions.rs` has separate three-party boot and turn barriers,
  actual instance identity assertions, growth/replacement/shrink, failure rollback,
  boot timeout/close/caller-drop cancellation, alias refusal and leased-failure
  quarantine. All seven regressions pass locally.
- The native adapter exposes `into_instance_factory`, inheriting real registry,
  restricted tools, permission and progress services without sharing busy/history
  state. Its new pool fixture boots three ports and executes a real tool continuation
  through AgentLoop. This does not yet prove three concurrent native provider turns.
- Hive still uses its explicit class-port route; integrating the new pools with
  per-instance topology/provenance, leases, heartbeat and shutdown remains open.
  Synchronous backend destructors and detached factory cleanup remain port-owned.

## Factory verification checkpoint

- All-feature workspace tests: **1,962 passed, 0 failed, 21 ignored**.
- Workspace all-target/all-feature Clippy and Rust 1.88 locked compilation,
  formatting, architecture, naming guard and whitespace checks pass locally.
- Owning swarm/harness DOX contracts and foundation evidence were updated.
  Root/parent indexes are unchanged because dependency direction, ownership
  boundaries and default-off host activation contracts did not change.
- Replacement publication is transactional, but bounded real supervisor teardown
  is not proven by the factory tests. In particular, creating replacement ports
  while failed port resources remain owned must be resolved in lease composition;
  published-slot bounds are not evidence of physical-resource bounds.
- Five-target CI, host activation, ledger acceptance and VesperLens wire coverage
  remain open. No release, tag or registry action was performed.

## Exact selected-lease execution prerequisite

- `WorkerPool::run_leased_task` consumes the exact selected lease and verifies
  pool ownership, live Busy/leased state, capabilities and deadline before worker
  dispatch. The ordinary `run_task` path delegates to it, avoiding divergent
  cancellation/deadline implementations.
- Three `pool_selected_lease_regressions.rs` tests prove that selecting the second
  worker does not reacquire the first, foreign/failed leases execute nothing, and
  capability/oversized-deadline refusal releases the lease without executing.
- This is the prerequisite for actual Hive assignment/provenance routing, not
  completed Hive pool wiring. Remaining integration must use concrete worker IDs
  for bus inboxes and ledger provenance, not nominal class slot zero.

## Selected-lease verification checkpoint

- Workspace all-feature tests: **1,965 passed, 0 failed, 21 ignored**.
- Workspace all-target/all-feature Clippy and Rust 1.88 locked compilation,
  formatting, architecture, naming and whitespace checks pass locally.
- DOX pass updated the owning swarm contract and foundation evidence. Root and
  parent ownership/index contracts remain unchanged; no activation, dependency or
  host-surface contract changed. Cross-platform and full F01–F18 acceptance remain
  open; no release, tag, registry action or live provider verification was run.

## Concrete Hive pool routing milestone

- `Hive::with_factories` validates configuration before async role-pool boot and
  builds concrete worker routes from published pool IDs. Candidate scores are
  per-instance, with role/port capability intersection. Topology nodes, bus inboxes,
  exact selected leases, and ledger provenance use the same worker identity.
- `hive_pool_regressions.rs` proves one navigator plus three same-class drivers
  overlap at a barrier and asserts assignment/provenance against executing IDs.
  Other regressions cover cross-role alias refusal, actual pooled-turn cancellation,
  interrupted no-replay, notification ordering and topology admission rollback.
- Legacy `Hive::new` refuses multi-worker floors backed by a single class port.
  Its prior nominal-two-worker topology test now asserts the one supplied worker;
  the new factory test is the actual multi-worker acceptance evidence.
- Topology admission stages topology state and rolls back earlier subscriptions on
  failure; successful admission is idempotent. Close/drop signals pools and closes
  the bus. Closed Hive refuses submit/admit/run before dispatch.
- `CancellationSignal::cancelled` shares notification wakeups across clones and
  registers before testing the flag, without introducing timer polling.
- Still open: health-driven lifecycle synchronization/growth/replacement, verified
  supervisor lease cleanup, native provider/host 1+3 acceptance, ledger scale and
  Settings/Lens gates. Concrete routing is not full F01–F18 acceptance.

## Concrete Hive routing verification checkpoint

- Workspace all-feature tests: **1,971 passed, 0 failed, 21 ignored**.
- Workspace all-target/all-feature Clippy and Rust 1.88 locked compilation,
  formatting, architecture, naming guard and whitespace checks pass locally.
- DOX updated the owning swarm contract and foundation evidence. Root/parent
  indexes and host contracts remain unchanged: no dependency boundary, activation
  or host surface changed. Shared code compiles in both hosts, but native host
  activation and full swarm acceptance remain gated.
- No release, tag, registry action or live provider verification was performed.

## Native OpenAI rejection diagnostic checkpoint

- Verified the unfinished shared adapter rejection implementation in `http_error.rs`:
  bounded 16 KiB/two-second reads, cancellation, safe status fallback and exact
  code/parameter allowlists. Both authentication modes retain safe parameters when
  error codes are null/unknown; new adversarial tests reject nested paths, prefixes
  and non-string parameters without exposing private canaries or permitting replay.
- `cargo test -p vesper-provider-openai --all-features`: 36 passed.
  `cargo test -p agent-vesper-acp --test openai_native --all-features`: 4 passed;
  `--test openai_rejection --all-features`: 1 passed, exercising both auth modes
  through the real isolated host. Context rejection reaches JSON-RPC error data as
  `provider turn failed: ContextLimit`; provider prose reaches neither stdout nor
  stderr. An initial assertion requiring a content update failed because the host
  correctly returns a protocol error; the test now asserts that precise boundary.
  This does not establish client rendering or parameter-level host diagnostics.
- `cargo test -p vesper-agent vesper_lens --all-features`: 39 passed, including
  authenticated POST notes/answers delivery. TUI `--bins openai`: 2 passed.
  Adapter all-target Clippy and new ACP test-target Clippy pass with warnings denied;
  architecture, naming guard, formatting and whitespace checks pass locally.
- No live provider call was made. The original HTTP 400 cause and reported
  missing-note/provider coupling remain unproven. Browser-to-native-provider tool
  result acceptance, ACP Lens integration, full swarm acceptance and workspace/
  MSRV/platform rechecks remain open; these focused checks do not replace them.
- DOX updated the adapter and ACP verification contracts. Parent/root and foundation
  ownership documents were left unchanged because no ownership/index boundary or
  runtime activation changed. Prior unfinished repairs remain preserved.

## Ledger retrieval and explicit retention continuation

- Added `ledger/filter.rs` and bounded structured/snapshot selection with conjunctive
  worker/task/role, category, confidence and inclusive original-sequence predicates.
  Invalid ranges, nonfinite confidence and oversized/empty identities fail closed.
  Structured/filtered-semantic results cap at 100; retained generations stay coherent.
  Sequence predicates do not claim wall-clock timestamp support.
- `transfer_filtered` preserves the existing atomic batch and confidence floor;
  category/provenance exclusions are reported as dropped. Foreign/unknown explicit
  IDs fail even when excluded by the predicate. Four new tests cover structured and
  semantic selection, scope/provenance, snapshot retention, caps and rollback. These
  were added with the new API; no runtime red-before claim is made for these four.
- Two semantic regressions failed before repair: the documented 0.7 threshold was
  not applied and configured over-fetch was ignored. They now pass. Retrieval uses
  HNSW's configured post-traversal filtering; approximate scoped results may be
  fewer than requested, without silently expanding the candidate budget.
- `prune_scope` implements explicit per-scope retention: Swarm removes lowest
  confidence then oldest admission, Worker/Task use FIFO. Log/exact index and rebuilt
  graph publish together; vectors/provenance are reused without embeddings, surviving
  IDs remain stable, removed vector capacity is reclaimed, and old reader snapshots
  remain intact. Two tests initially failed compilation because this API was absent,
  then passed, including post-eviction snapshot load and monotonic continued inserts.
- This is **not** automatic cap admission or scale acceptance. Rebuild and generation
  cloning costs remain open, as do automatic per-scope retention policy, host-native
  activation, lifecycle/supervisor cleanup, provider-wire Lens and original HTTP 400
  diagnosis. Full F01–F18 acceptance remains open and default-off is unchanged.
- Local verification: **242 swarm tests passed, 0 failed, 1 ignored**;
  **1,988 all-feature workspace tests passed, 0 failed, 21 ignored**. Workspace
  all-target/all-feature Clippy with warnings denied, Rust 1.88 locked all-target/
  all-feature compilation, architecture, naming guard, formatting and diff whitespace
  checks pass. Five-target CI and fresh supply-chain checks were not run here.
- DOX updated the nearest swarm ownership contract, this execution record and the
  foundation evidence index. Parent/root documents and host/provider DOX were
  intentionally unchanged: this milestone adds no ownership/dependency boundary,
  host surface, activation or provider behavior. Existing repairs were preserved;
  no commit/release/registry action or live provider call was performed.

## Automatic retention admission checkpoint

- `ledger/retention.rs` adds explicit persisted Disabled/Limited policy and
  `Ledger::with_retention`. Positive receiving-scope caps cannot exceed the graph's
  global capacity. Existing constructors remain strict-capacity/Disabled.
- Record admission reserves only the receiving scope under the existing Swarm
  confidence/age and private FIFO rules. Rebuild, eviction, new identity and vector
  insertion stage under the writer boundary and publish once; malformed vectors
  cannot commit an eviction. Global pressure never evicts another scope.
- Transfers prevalidate all source IDs and reserve for the whole accepted batch;
  batches exceeding the destination cap fail unchanged. Successful calls return
  only copies present in the published generation, not copies evicted by later
  members of that batch. Existing low-confidence/category exclusions still apply.
- Whole-ledger `VSWLEDG1` version **2** requires and persists the retention policy;
  version 1 is explicitly refused. Load checks policy bounds and all per-scope
  counts without embeddings. HNSW format remains version 2. Four new regressions
  cover admission/reload/FIFO, failed-vector/global-pressure rollback, batch
  reservation and missing/invalid/violated/old snapshot policy. Tests were added
  alongside this new API; this checkpoint makes no red-before runtime claim.
- Local verification: **246 swarm tests passed, 0 failed, 1 ignored**;
  **1,992 all-feature workspace tests passed, 0 failed, 21 ignored**. Full workspace
  all-target/all-feature Clippy, Rust 1.88 locked compilation, formatting,
  architecture, naming guard and whitespace checks pass. Initial Clippy rejected a
  collapsible conditional; the implementation was corrected, without lint suppression.
- Swarm owning DOX and this evidence/index updated; parent/root/host docs left
  unchanged because ownership, dependency and activation boundaries did not change.
  Automatic retention is available to composition but host selection is not wired.
  Generation/rebuild scale, native lifecycle/supervisor cleanup, Settings/host and
  Lens provider-wire acceptance, original HTTP 400 diagnosis and external platform
  gates remain open. No live provider, release, registry or commit operation occurred.

## Interview adapter-boundary evidence

- Added `vesper-provider-openai/src/lens_wire_tests.rs`: a native decoded interview
  function call becomes a linked tool result in the next Responses request in both
  API-key and ChatGPT modes. Schema, tool name, action, Unicode approval note and all
  selected answers survive exactly. Added GLM serialization counterpart under
  `vesper-provider-glm/src/lens_wire_tests.rs`, with the same feedback payload and
  call/schema/name checks. Neither test adds production provider behavior.
- These fixtures construct feedback at the domain boundary. They do **not** claim
  browser submission, server-to-host registration, actual Lens executor composition,
  or end-to-end provider-neutral acceptance. Both adapter serializers preserve the
  supplied note/answers; loss before serialization and the original HTTP 400 cause
  remain unproven. The original GLM-only-wiring hypothesis is not established by
  the reported error, nor ruled out at untested host boundaries by these tests.
- OpenAI suite: **37 passed**. `cargo xtask provider glm verify`: **59 passed**, with
  authoritative offline loopback scenarios. Latest workspace all-feature suite:
  **1,994 passed, 0 failed, 21 ignored**. Workspace strict all-target/all-feature
  Clippy, Rust 1.88 locked compilation, architecture, naming, format and whitespace
  checks pass. No live providers or user credential/state writes were used.
- Both nearest provider DOX files now document the scope of these tests; stale GLM
  test-directory ownership was corrected. Root/parent/host documents are unchanged:
  no production wire, dependency, activation or host contract changed. Full repair,
  lifecycle/scale, native Settings/host and browser-to-provider gates remain open.

## Retirement ordering and measured generation-cost repair

- Four `pool_retirement_regressions.rs` tests failed before their respective fixes:
  replacement reached two live RAII instances in a one-slot pool; failed boot kept
  the retired physical instance alive; external strong and upgradeable weak owners
  did not block replacement. Factory replacement now drops exclusively owned failed
  unleased instances before boot, outside state/instance locks. Failed slots remain
  quarantined on boot refusal and can be retried without reviving old resources.
  External strong/weak references refuse new allocation. All four tests and the
  existing seven independent-factory regressions pass. This establishes in-process
  ownership order, **not** bounded external destructors or detached supervisor cleanup.
- Added two explicitly invoked release-mode measurements in
  `tests/ledger_scale_regressions.rs`. Baseline 10k/16D appends: **27.667 s**;
  snapshot/load/byte compare **26.444 ms**, encoded snapshot **4,639,542 bytes**.
  1k retained admissions at a 64-entry cap: **1.311 s**. These are synthetic offline
  vectors and local timings, not live-provider performance or universal thresholds.
- HNSW graphs now share immutable `Arc<Node>` values and use `Arc::make_mut` for
  adjacency changes. Structured generations share immutable `Arc<LedgerEntry>`
  payloads. No unsafe code, reader mutex, new dependency or snapshot-format change.
  New graph-clone regression proves original bytes remain unchanged after inserts,
  and continued insertion equals a separately restored graph byte-for-byte.
- Same release workload after node sharing: **10.249 s**. After entry sharing:
  **1.618 s** (about 17x faster than baseline), snapshot/load/compare **26.405 ms**,
  same encoded size. 1k retained admissions: **1.291 s**. Both workload correctness
  tests pass, including retained readers, fixed retention counts and lossless reload/
  continuation. Map/ordinal tables still clone and retention rebuilds the graph;
  high-dimensional/million-entry memory and native-host latency are not certified.
- Verification: **1,999 workspace all-feature tests passed, 0 failed, 23 ignored**.
  The two newly ignored scale tests were separately executed successfully with
  `cargo test -p vesper-swarm --release --test ledger_scale_regressions -- --ignored
  --nocapture`. Workspace all-target/all-feature strict Clippy, Rust 1.88 locked
  compilation, architecture, naming, formatting and whitespace checks pass.
- DOX updated the nearest swarm contracts/verification and foundation evidence index;
  root/parent/host/provider docs remain unchanged because there is no ownership,
  activation, dependency or host-surface change. Full lifecycle synchronization,
  supervisor cleanup, native Settings/host/Lens integration and external gates remain
  open. No live provider, user-state, release, registry or commit operation occurred.

## Hive membership reconciliation milestone

- Added `hive/lifecycle.rs`: admitted factory Hives expose caller-owned driver
  `scale_role` and timed `maintain_workers`. Actual growth/shrink/replacement now
  reconciles execution wrappers, topology membership, inboxes and assignment loads.
  Surviving workers retain load history; retired inboxes are unsubscribed. Cross-role
  Arc aliases are rechecked after new boots. Topology edits stage before publication;
  post-boot reconciliation failure closes the hive to prevent stale dispatch.
- Scaling preflights topology capacity and refuses navigator cardinality changes,
  unknown roles, unadmitted/legacy/closed or interrupted hives. Maintenance never
  invents heartbeats or replays goals. Disabled navigator failover closes on loss;
  enabled replacement follows topology policy without forcing an election. A guard
  closes all pools and bus on failed/dropped replacement, since old resources may
  already have retired. Native monitor/heartbeat scheduling remains host work.
- Five `hive_scaling_regressions.rs` tests pass: initial scaling tests failed to
  compile because the API was absent; later coverage checks actual three-worker
  post-growth provenance, execution after shrink, removed inbox refusal, capacity/
  cardinality checks, post-boot subscriber conflict, replacement identities,
  disabled navigator failover and manually polled caller cancellation. Maintenance
  tests were added with the API; no runtime red-before claim is made for them.
- Verification: **2,004 workspace all-feature tests passed, 0 failed, 23 ignored**.
  Workspace strict all-target/all-feature Clippy, Rust 1.88 locked compilation,
  architecture, naming, formatting and whitespace checks pass locally. Prior
  explicit scale measurements are unchanged; they were not rerun in this milestone.
- DOX updated the nearest swarm contracts and foundation execution/index evidence.
  Root/parent and host/provider docs remain unchanged: no dependency, ownership or
  activation surface changed. Native Settings/TUI/ACP, actual browser-to-provider
  Lens, verified sandbox leases/supervisor cleanup, remaining scale/platform gates
  and full F01–F18 reconciliation remain open. No live provider/user-state write,
  commit, release or registry operation occurred.

## Explicit quarantine recovery milestone

- `crates/vesper-swarm/src/sandbox.rs` adds caller-owned `retry_quarantined`:
  at most 4,096 attempts, once per selected empty boundary in admission order.
  Successful backend confirmation frees capacity and shared names, increments
  successful-release accounting and fulfils queued waiters. Failure retains
  quarantine and bounded diagnostics. Cleanup after close never reopens admission.
- `SandboxLeasePort::retry_release` defaults to explicit unsupported refusal;
  implementations must support idempotent recovery from partially completed
  teardown. No blind replay of `release`, automatic retry loop or new OS backend
  is introduced. Synchronous calls remain under the book lock and require bounded,
  non-reentrant implementations; attempt limits do not prove wall-clock cleanup.
- Three offline `lease_recovery_regressions.rs` tests cover failed/successful retry,
  unsupported backend refusal, queued handoff, successful-release accounting,
  shared-name quarantine/reuse, zero/oversized budgets and closed-book recovery.
  Initial test compilation used an incorrect enum variant, corrected to the existing
  `ProcessTree`; this is not red-before behavioral acceptance evidence.
- Verification: `cargo test --offline -p vesper-swarm`: **260 passed, 0 failed,
  3 ignored**. Package all-target strict Clippy, Rust 1.88 locked all-target check,
  architecture, naming guard and whitespace checks pass. Whole-workspace and
  release-scale suites were not rerun in this bounded milestone.
- DOX: updated nearest swarm contracts; foundation ownership and parent indexes
  are unchanged. Both hosts inherit this foundation API but no activation or
  supervisor composition is added. Native lease cleanup, Settings/TUI/ACP and
  full F01–F18 acceptance remain open; activation remains default-off.

## Native composition and shared sandbox outcome checkpoint

- `vesper-harness/src/sandbox_backend.rs` no longer discards explicit backend
  teardown errors. Unverified cleanup overrides success/cancellation, retains the
  run error or available stdout/stderr, and makes that port refuse subsequent
  provisioning. Cancellation after provisioning skips command dispatch but still
  tears down; cancellation after cleanup cannot return success. Both hosts already
  use this shared adapter (`agent-vesper-tui/src/main.rs`,
  `agent-vesper-acp/src/lib.rs`); no duplicate host-specific implementation is added.
  Already-admitted concurrent operations are not retroactively cancelled. Backend
  panic/hang handling and verified process-tree cleanup are not established.
- Six `sandbox_outcome_tests.rs` unit tests cover cleanup/run-error arbitration,
  output preservation, late cancellation, timeout preservation and sticky refusal.
  Initial test compilation failed because the extracted helper did not yet exist;
  no red-before runtime or real supervisor-failure reproduction is claimed.
- `vesper-harness/tests/swarm_native_hive.rs` now exercises real Hive -> native
  factory -> AgentLoop -> real read_file -> tool-result continuation -> synthesis
  under mesh/hierarchical/centralized/hybrid configurations. A three-party barrier
  inside separate provider sessions would deadlock serial/aliased execution.
  Each run asserts five independent runtime sessions (decomposition, three drivers,
  synthesis), eight provider requests, three distinct assignment IDs, four ledger
  entries, all three real file canaries in synthesis and no `.agent-vesper` writes.
  Synthetic provider/embedding fixtures are test-only. This does not prove sandbox
  enforcement, topology-edge routing constraints or native host activation.
- Verification on Linux: final `CARGO_NET_OFFLINE=true cargo xtask verify` passes,
  including workspace all-feature **2,016 passed, 0 failed, 23 ignored**; final
  `cargo xtask msrv` passes the same **2,016/0/23** on Rust 1.88. Default workspace
  tests pass **1,981/0/17**. Workspace strict all-target/all-feature Clippy,
  architecture, naming, fixture/contract/provider/runtime/ACP/session gates,
  formatting and whitespace checks pass. `cargo-deny 0.20.2 --all-features check`
  passes advisories, bans, licenses and sources. Five-target CI is not run here.
- The first full MSRV run failed `driver_detection_pins_only_valid_installed_ids`
  while spawning a fresh CLI fixture. The isolated test and two subsequent complete
  MSRV runs passed. The old error discarded the OS cause, so its root cause is
  unproven, not declared fixed. `web_settings.rs` now retains OS kind/code without
  exposing private executable paths; a missing-executable regression passes. No
  retry, sleep, assertion weakening or production success fallback was added.
- Real Chrome interview browser acceptance passes via the existing
  `vesper_lens_interview_browser.mjs`, using Node 22.23.2 and Playwright from isolated
  `/tmp` tool installations. `node` was absent from PATH; the existing explicit
  executable resolved it. Browser -> server feedback acceptance and the previously
  recorded adapter serialization tests pass independently; a single integrated
  browser -> native host -> provider execution trace remains open.
- Nearest harness DOX and foundation evidence updated. Root/crates/app DOX and
  child indexes intentionally unchanged: no new dependency, ownership boundary or
  host activation surface. No live provider, credential/user-state, release, registry
  or commit operation occurred. Foundation gates passing does not authorize activation.

## Explicit backend cleanup, identity and governance continuation

- Real backend source exposed two hidden F10 defects: Docker accepted every
  cleanup command exit (including daemon/permission failure), and namespace
  teardown returned success after Drop discarded poisoned-lock/kill/reap errors.
  Two regressions failed against those paths before repair. Explicit teardown now
  checks command success and local supervisor kill/reap results; missing/empty
  Docker commands fail closed and combined cleanup failures remain visible.
  Four local child/CLI tests pass. These use actual processes, not actual namespaces
  or Docker isolation. Drop recovers poisoned ownership for best-effort cleanup;
  explicit teardown still reports poison. CLI timeout reaping uses up to 500 ms
  polling grace rather than an unbounded `Child::wait` after the deadline.
- Three F09 identity regressions failed before repair: duplicate active identity
  with spare capacity, duplicate queued identity, and quarantined identity reuse.
  `WorkerReserved` now refuses before backend/queue mutation across live members,
  reserved origins and queued waiters. Queue drop frees its reservation; origins
  remain reserved until verified boundary cleanup. Shared-path tests use distinct
  identities so identity refusal does not mask the separate path-isolation check.
- F18 naming baseline is now strict versioned JSON with counted normalized
  path/content-digest identities; line numbers only identify diagnostics. Four
  self-tests prove stable line shifts, duplicate-occurrence refusal, edit/move
  refusal and malformed/version rejection. Migration was compared against HEAD:
  all **30 frozen occurrences** were preserved exactly, with no new exception and
  no regeneration from current source. Current scan has 27 permitted occurrences.
- ADR 0026 supersedes ADR 0025's single-stream adapter, TUI-only exclusion and
  completion claims under the approved full repair scope, preserving ADR 0025
  unchanged. Current PRD/migration status now says repairs in progress/default off;
  stale two-provider migration wording was corrected to the three registered
  adapters. Clock-injection, immutable-reader and dependency goals are not waived.
- Canonical verification reproduced the driver fixture failure with OS error 26,
  `ExecutableFileBusy` (ETXTBSY). Runtime-written executable fixtures were replaced
  by one immutable checked-in shell fixture through per-test symlinks; response/load
  state stays in isolated temporary roots. This removes the freshly written
  executable exposure; concurrent inherited writable descriptors are a plausible
  mechanism, not separately instrumented kernel evidence. No production retry or
  assertion weakening was added. All three CLI fixture cases use the same remedy;
  **20 repeated seven-test web-settings suites** and subsequent full suites pass.
- Final local gates: canonical `cargo xtask verify` and Rust 1.88 full workspace
  **2,027 passed / 0 failed / 23 ignored**; default workspace **1,990/0/17**.
  Canonical includes strict all-target/all-feature Clippy, formatting, architecture,
  naming and fixture/contract/provider/runtime/ACP/session gates. Sandbox no-capture
  output confirms six namespace tests skip their isolation bodies because this host
  cannot provision namespaces; three Docker integration tests remain ignored.
  Those counts are not supervisor/container isolation acceptance.
- DOX updated nearest sandbox, swarm, harness, xtask and ADR contracts plus parent
  documentation links and current evidence. No dependency, native host activation,
  supervisor protocol or unsafe-boundary change; root/app contracts stay unchanged.
  Async lease reservation/commit, backend panic/hang handling, real supervisor and
  both-host activation acceptance remain open. No live provider/user credential,
  registry, release or commit operation occurred.

## Topology dispatch, assignment correlation and oracle vectors

- Two desired-behavior regressions failed before repair: a disconnected driver
  still executed, and an unrelated urgent bus message with the identical prompt
  substituted for the intended assignment. Hive now requires an active elected
  navigator and directed reachability to each scored candidate. The pure routing
  helper ignores missing/failed intermediates and invalid weights and honors
  explicit reverse edges. Transport stays direct in-process; no relay agent turns
  are fabricated. No reachable eligible candidate interrupts the goal without replay.
- The exact issued bus message ID, concrete navigator/driver identities, kind and
  prompt must match before executing a turn. `routing_tests.rs` exercises disconnected
  refusal across all four topologies; routing unit coverage checks direction, live
  intermediates, absent nodes and invalid weights. Native three-session/tool/synthesis
  integration continues to pass under all four configured topologies.
- F05 now has captured source execution, not merely hand-authored arithmetic:
  `tests/assignment_oracle_vectors.json` stores 72 vectors from the original
  `scoreAgentForTask` method in `swarm/src/unified-coordinator.ts` (oracle package
  prefix elided), commit `e341ec8c4aba8ea616499180dee53035af7e295c`, method SHA-256
  `0cdbe2f756098d4c629d15c9345bf060af037928a671d730cca2bf326fa0b052`.
  Capture selects the method text from its declaration to the next method declaration,
  extracts the body, removes only `: Record<TaskType, AgentType[]>`, and invokes
  that body with Node 22.23.2. Cross-product inputs: type match false/true,
  workload 0/0.5/1, health 0/0.5/1, success rate 0/1, duration 0/60 seconds (converted
  to milliseconds for the oracle). A coding task and researcher/coder agent provide
  no-match/match. The Rust regression maps this Boolean to capability inclusion;
  the differing task type systems are not claimed identical. All 72 outputs match.
  Oracle HEAD and clean status were checked before and after; no oracle edits.
- Final checkpoint: canonical offline verification and full Rust 1.88 workspace
  **2,031 passed / 0 failed / 23 ignored**; default workspace **1,994/0/17**.
  Canonical strict Clippy, formatting, architecture, naming, fixture/contract,
  provider/runtime/ACP/session gates pass. External isolation/target gates remain
  unexecuted; skipped namespace and ignored Docker bodies are not upgraded to proof.
- Nearest swarm ownership updated; F01 still requires scoped sandbox/host acceptance.
  F05 arithmetic and hard eligibility now have source-captured/local regression
  evidence; composed selection remains covered separately. No host activation or
  dependency change is introduced.

## Shared sandbox panic containment

- `crates/vesper-harness/src/sandbox_backend.rs` catches unwinding panics during
  backend future construction and polling for provision, run and teardown.
  Every caught panic permanently quarantines the port; a run panic becomes an
  error before the existing explicit teardown step. No automatic replay or reset.
  Tool diagnostics name the phase but omit arbitrary panic payloads. The process
  panic hook is unchanged and may still print payloads to its configured sink.
- `sandbox_panic_tests.rs` adds three offline tests covering immediate and polled
  provision panics through the actual command port, subsequent admission refusal,
  run/teardown guard errors and ordinary provisioning refusal without quarantine.
  No fabricated production handle constructor was added to enable tests. Actual
  run-panic-to-supervisor cleanup remains a composition acceptance gap.
- Verification: all-feature harness suite **101 passed, 0 failed, 0 ignored**;
  strict all-target/all-feature package Clippy, Rust 1.88 locked all-target check,
  architecture, naming guard and whitespace checks pass. Initial test compilation
  incorrectly named a non-direct dependency; corrected to the existing agent
  capability re-export without changing dependencies. Whole-workspace suites and
  external supervisor/platform gates were not rerun for this milestone.
- Both native hosts use this shared command adapter. Owning harness DOX updated;
  root/parent/host documents and child indexes remain unchanged because ownership,
  activation and host composition did not change. Catching unwind does not contain
  abort/double-panic or interrupt blocking backend hangs, and does not implement
  async lease reservations outside locks. Those remain open under F10.

## Lease-port unwind containment

- `crates/vesper-swarm/src/sandbox.rs` catches acquire/release/retry backend unwind
  inside the book lock's lifetime, preventing backend unwind from poisoning it.
  Panic closes admission and wakes queued callers with `Closed`. Uncertain partial
  acquisition retains a boundary origin, shared group and capacity reservation;
  failed cleanup retains quarantine. Explicit verified recovery never reopens the
  book. Successful cleanup count can exceed successful acquisition count when a
  panicked acquisition is later cleaned up; counter documentation states this.
- Two `lease_panic_regressions.rs` tests cover partial-acquisition reservation,
  admission closure, queued wakeup, retry panic, inspectable/non-poisoned state,
  and a release panic while the holder is already unwinding. These are test-port
  assertions, not actual supervisor cleanup evidence. No red-before runtime claim:
  running the old double-panic path could abort the test process.
- Verification: swarm suite **269 passed, 0 failed, 3 ignored**; strict all-target
  package Clippy, Rust 1.88 locked check, architecture, naming, workspace formatting
  and whitespace checks pass. Owning swarm DOX updated; parent/host/index ownership
  contracts unchanged. Subsequent offline all-feature whole-workspace suite:
  **2,036 passed, 0 failed, 23 ignored**. Scale and external isolation gates were
  not rerun; ignored/skipped isolation bodies are not acceptance.
- This handles unwinding panics, not abort, a panicking panic payload destructor,
  blocking calls or asynchronous reservation/commit. Backend calls remain under
  the book lock and native lease composition is still unaccepted. Activation stays
  default-off; no live provider, user-state, release or registry operations.

## Asynchronous leases and real contained native worker composition

- Lease provisioning, release and explicit recovery now use reservation/dispatch/
  commit outside the book lock. The reentrant-capacity regression failed before
  repair. Seven async regressions pass for blocked calls, cancellation/drop,
  close/late success, partial provisioning, FIFO/shared admission and single retry
  ownership. Runtime tasks retain resources/capacity through caller timeout;
  `CleanupReport` distinguishes held, pending and quarantined boundaries. A
  deadline is not a claim that a blocking call stopped. Existing identity,
  recovery and panic tests now await explicit cleanup without weakening outcomes.
- `swarm_sandbox.rs` composes the native factory, fresh worker roots, permission
  ports, real backend and command route. Scoped leases survive all command
  continuations and detached command ownership. Single-run supervisors rotate
  only after verified cleanup. Five native refusal/alias/preparation tests pass.
- The real namespace Hive gate was attempted both sandboxed and escalated and
  failed at the capability gate. `target/debug/sandbox_init probe` reports
  `write /proc/self/uid_map: Operation not permitted (os error 1)`. No isolation
  body ran; this remains an external acceptance blocker.
- Real Podman execution exposed SELinux mount denial and cleanup timeouts before
  repair. Native dedicated worker roots now explicitly opt into private `:Z`
  labels; generic project mounts do not. Containers use zero stop grace so force
  cleanup fits the existing five-second CLI bound. Isolation was not disabled.
  Primary contracts: [Docker bind mounts](https://docs.docker.com/engine/storage/bind-mounts/),
  [Podman private labels](https://docs.podman.io/en/latest/markdown/podman-run.1.html),
  [Podman force-removal wait](https://docs.podman.io/en/v5.1.1/markdown/podman-rm.1.html).
  Assertions inspect typed `ToolResultStatus::Succeeded`; matching output text
  inside a failure diagnostic is explicitly insufficient.
- Passed the explicit real container test on all four topologies, using the
  already-installed Debian image pinned by digest, no image pull/provider call:
  `VESPER_DOCKER_BIN=podman VESPER_DOCKER_IMAGE=docker.io/library/debian@sha256:5ae3c39ebd15e229dcedd5cee596b2497182493d41ff162e824ba13fc1b2b867 cargo test -p vesper-harness --all-features --test swarm_native_hive native_container_hive --offline -- --ignored --nocapture`.
  The final run passed in 10.75s: three independent provider sessions, two real
  approved commands each, six distinct permission scopes across replacement,
  evidence-fed synthesis, scale-up/shrink, four actual replacements, a second goal
  after replacement, and clean final shutdown. This is native factory/Hive evidence,
  not TUI/ACP Settings activation, namespace acceptance or every cancellation case.
- New API/schema contracts are documented in the owning swarm, harness and sandbox
  DOX. Root/app/parent ownership and child indexes are unchanged. Required host
  product wiring, cognitive/compaction/Lens acceptance and remaining F13–F18 work
  are still outstanding; default activation remains off. No commit, release,
  registry, credential or live-provider operation occurred.

## Current F01–F18 reconciliation

This matrix supersedes milestone-local pending notes. All named ordinary suites
below passed in the current canonical and MSRV runs; historical counts are not
reused as current evidence. Paths under `tests/` are relative to
`crates/vesper-swarm/` unless a crate is named. **No full VRO-15 completion claim.**

| Finding / requirement | Current implementation and exact assertion evidence | Status / remaining code or acceptance |
|---|---|---|
| F01 — real orchestration | DAG, selected instances, bus correlation, routing; `hive_boundary_regressions`, `hive_concurrency_regressions`, harness `swarm_native_hive` including explicit real container gate | Partial: native host service/activation, project-input materialization and complete failover policy acceptance |
| F02 — native harness reuse | Existing AgentLoop, restricted registries, real scoped commands; harness `swarm_adapter_tests` and `swarm_native_hive` verify successful tool statuses, permission scopes and synthesis | Partial: cognition/semantic compaction/tool-context preservation through composed Hive and both hosts |
| F03 — bounded goal lifecycle | `hive_boundary_regressions` and `hive_concurrency_regressions`: drop/cancel/deadline and interrupted goals; `lease_async_regressions`: late cleanup without stale publication | Partial: native host cancellation/partial-output lifecycle, full stream no-replay acceptance |
| F04 — persisted native activation | No product activation implemented or advertised | **Unimplemented:** persisted Settings, shared command service/catalog and both host execution paths |
| F05 — scorer fidelity | `assignment_regressions`: hard eligibility, corrected arithmetic and all 72 pinned-oracle vectors; native fixture checks distinct selected workers | Local foundation verified; full product/platform acceptance separate |
| F06 — cancellation ownership | Timeout/pool cancellation suites plus `lease_async_regressions`: caller-drop/timeout retains backend ownership until cleanup | Partial: complete native detached AgentLoop/history shutdown acceptance |
| F07 — real pool lifecycle | Pool instance/retirement/selected-lease suites; real container Hive proves scale-up, shrink, four actual replacements and execution afterward | Partial: generic blocking/destructor/detached worker retirement acceptance |
| F08 — topology policy | Centralized/partition/topology regressions; real container Hive executes all four topologies and enabled replacement | Partial: full native disabled-failover and route-loss policy acceptance |
| F09 — sandbox scope/identity | `sandbox_tests`, `lease_identity_regressions`; harness `swarm_sandbox_tests` checks fresh-root alias refusal/canary invariance; container test proves independent permission roots | Partial: shared OS scopes and broader alias/platform isolation acceptance; native adapter intentionally supports isolated scopes only |
| F10 — owned verified cleanup | `lease_async_regressions` (7), lease panic/recovery suites, native refusal tests; real container Hive ends with clean report after real command/scale/replacement cleanup | Partial: namespace gate externally blocked; additional native cancellation/runtime-shutdown/destructor cases. Blocking calls retain capacity, never claimed preemptible |
| F11 — bus terminal states | `bus_tests`: close/unsubscribe wake parked readers and reject later operations | Local foundation verified; external target gates pending |
| F12 — bus resource bounds | `bus_bounds_regressions`, `bus_clock_regressions`: atomic broadcast, ACK/byte/subscriber/TTL bounds, injected expiry | Local foundation verified; external target gates pending |
| F13 — snapshot validation | `hnsw_regressions`, `hnsw_tests`: checked loader and malformed graph refusal | Partial: broader corruption/property and cross-target coverage |
| F14 — lossless continuation | `hnsw_regressions`: raw vectors/config/RNG and byte-identical continued insertion | Local foundation verified; cross-target snapshot acceptance pending |
| F15 — scale/math | Robust arithmetic/pruning tests; explicit release `ledger_scale_regressions` passed (10k/16D and 1k retention) | Partial: high-dimensional/default-capacity memory and adversarial clustering measurement |
| F16 — complete ledger contract | Immutable-reader, full-snapshot, atomic-transfer, filter/eviction/retention suites plus explicit scale assertions | Partial: timestamps/ranges still unimplemented; full retention/oracle reconciliation and large-scale acceptance |
| F17 — truthful acceptance | Current canonical/MSRV/default gates; typed tool-status assertion, real permission trace and contained lifecycle gate; this requirement-to-test matrix | Partial: product-host, complete supervisor/cancellation and integrated browser→native continuation→provider acceptance |
| F18 — governance/timing | Canonical architecture/naming tests pass; injected bus clock; exact-image contained Hive step added to `web-driver.yml` and YAML validated | Partial: remaining timing injection and external two-/five-target CI runs; default-off architecture preserved |

## Current verification and exact resume point

- `cargo xtask verify` exited 0. Its all-feature workspace pass contains
  **2,049 passed / 0 failed / 25 ignored**; later canonical package/doctype
  reruns are not added to that unique workspace count. Strict all-target/
  all-feature Clippy, formatting, architecture, naming and fixture/contract/
  provider/runtime/ACP/session gates passed. First sandboxed attempt was denied
  at ACP loopback fixture binds; the authorized outside-sandbox rerun passed.
- `cargo +1.88.0 test --workspace --all-features --locked --offline` exited 0,
  **2,049/0/25**. This is the underlying full-suite action of `cargo xtask msrv`.
- `cargo test --workspace --offline` exited 0, **2,006/0/17**.
- `/tmp/vesper-audit-tools/bin/cargo-deny --all-features check` passed advisories,
  bans, licenses and sources after authorized access to its advisory cache lock.
- The explicit container Hive gate above also passed against
  `localhost/vesper-web-driver:acceptance` in 9.83s after the fix. The same-image
  gate is now in `.github/workflows/web-driver.yml` for x86_64 and ARM64;
  workflow YAML parses locally. CI execution is pending, not local evidence.
- `VESPER_DOCKER_BIN=podman VESPER_WEB_TEST_IMAGE=localhost/vesper-web-driver:acceptance cargo test -p vesper-web-fetch --all-features real_pipe_browser_actions -- --ignored --nocapture`
  passed the real pipe-browser actions/redaction/stale-index assertion.
- `cargo test -p vesper-swarm --release --test ledger_scale_regressions --offline -- --ignored --nocapture`
  passed both bodies: 10k/16D append 1.616s, snapshot/load/compare 26.34ms,
  4,639,542 encoded bytes; 1k retention at cap 64 took 1.296s. These are bounded
  workload observations, not high-dimensional/million-entry certification.
- Existing real Chrome interview gate passed:
  `PLAYWRIGHT_MODULE=/tmp/vesper-lens-browser-test/node_modules/playwright /tmp/vesper-node-test/node-v22.23.2-linux-x64/bin/node crates/vesper-agent/tests/vesper_lens_interview_browser.mjs`.
  This remains browser→server evidence; a complete native continuation→provider
  trace was not implemented or claimed by this pass.
- Swarm oracle is clean at `e341ec8c4aba8ea616499180dee53035af7e295c`.
  The root contract's lowercase frozen Python path is absent on this host; it
  was not modified or substituted with an unpinned source.
- Logs: `/tmp/vro15-native-{verify,msrv,default,deny,scale,lens-browser}.log`.
  Namespace retry command:
  `cargo build -p vesper-sandbox --bin sandbox_init --offline`, then
  `VESPER_SANDBOX_INIT=/home/Alex/Projects/agent-vesper/target/debug/sandbox_init cargo test -p vesper-harness --features swarm --test swarm_native_hive native_scoped_hive --offline -- --ignored --nocapture`
  on a host permitting the actual namespace probe. No skip counts as a pass.
- Resume with **F04 and F01/F02 product composition**, not another foundation
  audit: implement the shared service/command/settings path in both hosts, wire
  a real configured embedding adapter without a synthetic fallback, and provide
  bounded project inputs to each fresh worker scope. Then test host cancellation,
  partial state, cognition/compaction and the complete Lens continuation path.
  F13/F15/F16/F18 remaining work is enumerated above. These are outstanding
  implementation tasks, not merely external verification blockers.
- DOX updated nearest swarm/harness/sandbox/CI owners and current foundation
  evidence/status. Root, parent crate/app contracts and child indexes remain
  unchanged because no ownership boundary or native host surface changed. No
  commit, push, tag, release or registry action was performed.

## Completion requirements

Every F01–F18 requires source/test/command evidence. Require real 1+3-worker
overlap, permissioned tools, grounded synthesis, bounded cancellation/teardown,
lossless whole-ledger snapshots and coherent readers, Settings/TUI/ACP parity,
default-off/no-user-state regressions, and verified provider-neutral feedback.
Unexecuted or unavailable gates remain explicit blockers. Never infer quota/model
capabilities or substitute a fake production embedding backend.
