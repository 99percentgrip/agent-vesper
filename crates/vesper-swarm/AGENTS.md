# vesper-swarm — pure swarm-coordination foundations (VRO-15; VRO-16 governance)

## Purpose

Own the provider-neutral, pure-logic swarm coordination foundations
extracted from *the swarm oracle* (an authorized upstream orchestration
repository): topology, worker pooling, priority messaging, task assignment,
shared memory ledger, sandbox lease coordination, hive orchestration and
task-level governance. It delegates turns through
execution ports, performs no network/filesystem I/O and names no provider.
Bus TTL accepts an injected clock; pool/turn deadlines use Tokio's monotonic
clock and are tested with virtual time. Governance countdowns derive from
caller-supplied wall-clock milliseconds via `GovernanceClock` — the crate
reads no clock itself. Physical blocking-destructor observation
uses a bounded wall-clock wait; it cannot preempt external destructors.

## Ownership

- `src/lib.rs` — crate root, module index, and the zero-I/O contract.
- `src/topology.rs` — `NodeId`, `TopologyKind` (mesh, hierarchical,
  centralized, hybrid), `TopologyConfig` (+ `validate` and fail-closed
  defaults), `TopologyRole`, `NodeStatus`, `TopologyNode`, `TopologyEdge`,
  `TopologyPartition`, `TopologyState` (with the `join_order` admission
  ledger).
- `src/manager.rs` — `TopologyManager`: `add_node`/`update_node`/
  `remove_node`, deterministic `elect_leader` (oldest eligible queen, then
  oldest eligible member, by join order), fail-closed leader failover
  (promote requires `failover_enabled`; otherwise leadership goes vacant and
  the led partition dissolves), strategy-driven partition assignment
  (`hash` via inline FNV-1a, `range`, `round-robin`; new partitions open
  when membership exceeds `nodes_per_partition`), and per-kind
  `rebalance` edge construction (bounded-degree symmetric mesh, fanout
  tree, hub/spoke, hybrid backbone + intra-partition mesh). Rebalance is
  idempotent; `NodeUpdate`/`MetadataPatch` patch role/status/metadata.
  `TopologyState::automatic_election_blocked` persists disabled-failover vacancy
  across rebalances until explicit election. Removal chooses eligible successors
  and refuses inconsistent join ledgers before mutation. Partition leaders must
  be eligible; initial/failover selection follows admission order, not placement
  sort order. Status/removal reconciles partition leadership even when automatic
  edge rewiring is off. Disabled-failover dissolution persists affected members in
  `partition_election_blocked` across serialization/repartitioning; explicit
  election clears suppression. Surviving partitions clamp replica counts.
- `tests/partition_lifecycle_regressions.rs` — initializing/failed exclusion,
  admission-order successors, all auto/failover flag combinations, removal/status
  loss, serialized suppression and explicit recovery.
- Centralized rebalancing reconciles leadership before wiring; a disabled-
  failover vacancy never falls back to an implicit hub. Automatic node updates
  rebuild wiring to reflect leadership changes.
- `tests/centralized_lifecycle_regressions.rs` — failed-hub successor wiring,
  disabled-failover edge vacancy and repeated-rebalance idempotence.
- `tests/topology_regressions.rs` — disabled-failover vacancy, failed successor
  exclusion and mutation-free inconsistent-ledger refusal.
- `src/worker.rs` — `WorkerPort` (the execution seam: `run_turn` returning
  a `BoxFuture` of `TurnReceipt` under a `CancellationSignal`),
  `CancelFlag`/`CancellationSignal` cooperative cancellation pair,
  `WorkerCapabilities`, `WorkerTask`/`TaskKind`/`TaskPriority`,
  `TurnReceipt`, `WorkerError`. `CancellationSignal::cancelled` parks on shared
  notifications without timer polling; registration precedes checking the flag,
  so cancellation before/after registration and across clones cannot be missed.
- `src/pool.rs` — `WorkerPool` over `Arc<dyn WorkerPort>`:
  `PoolConfig` (validated bounds/timings), parallel `initialize` (one
  `join_all` wave to `min_workers`), lease-guarded `acquire`/`release`
  (drop releases; release never resurrects a `Failed` worker),
  `run_task` (deadline race, cancelled/timed-out tasks never report
  success), `scale` within bounds (busy workers never shed; floor
  `min_workers`), `health_tick` (fails silent workers and overdue
  in-flight, cancelling the task first) + caller-owned
  `heartbeat_interval`, `replace_failed`, bounded event log. Repeated
  initialization adds only missing floor slots, replacement reuses failed slots
  even at maximum capacity, and acquisition never grows incapable workers.
  Pool configuration caps workers at 4,096 and timing budgets at 24 hours;
  oversized task deadlines are refused before acquisition.
  Close and dropped in-flight leases signal actual workers; health-cancelled
  late receipts are refused, and deadline wins simultaneous readiness.
  `with_factory` uses `src/pool_instances.rs` and `WorkerInstanceFactory` for
  independent async creation. Boot waves publish transactionally, reject aliased
  or zero-capacity ports, and cancel on failure/deadline/close/caller drop.
  Factory-backed growth/replacement uses real instances and never reuses IDs;
  last-owner pool drop closes and dispatches instance retirement. Successful
  unpublished boot members retain retirement ownership even when publication
  fails. Retirement waits up to ten seconds for exclusive ownership and pending
  work; a remaining external owner or panic keeps cleanup unverified.
  failed-but-still-leased slots cannot be replaced. `scale_async` is required
  for factory pools; legacy `new` retains shared-port compatibility only.
  `run_leased_task` executes the exact selected lease instead of reacquiring a
  worker. It refuses foreign, released, failed, incapable or over-budget leases
  before dispatch and consumes/releases the lease on every outcome. `run_task`
  delegates to this same execution path. Ports own detached cleanup; Hive
  lease/health composition remains outstanding.
- Factory replacement retires failed unleased instance Arcs before booting new
  instances, outside state/instance locks. Failed boot leaves the slot quarantined
  without restoring the retired instance. External strong or upgradeable weak
  owners refuse replacement before new allocation; a raw slot count is not proof
  of physical-resource bounds. Detached cleanup and blocking external destructors
  are covered by owned retirement tasks and explicit settlement. Boot rollback and
  runtime shutdown remain separately audited.
- `tests/pool_retirement_regressions.rs` proves peak live RAII instance count,
  failed-boot quarantine/retry and strong/weak external-owner refusal before boot.
- `tests/pool_selected_lease_regressions.rs` — selected identity preservation,
  cross-pool/failed lease refusal and mutation-free capability/deadline rejection.
- `tests/pool_instance_regressions.rs` — separate boot/turn barriers proving
  three independent instances, actual growth/replacement/shrink, alias refusal,
  transactional boot failure, timeout/close/drop cancellation, and leased-failure
  quarantine.
- `tests/pool_cancellation_regressions.rs` — manually polled real boundary signals
  for close, health cancellation with a success-ignoring port, and caller drop.
- `tests/pool_tests.rs` — integration battery with in-crate fakes
  (`FakeWorkerPort` succeed/cancel-aware/hang, `WaveTracker`), covering
  bounds state machine, parallel boot wave, deadline enforcement,
  heartbeat fail/cancel/replace, monitor-loop pattern, scaling rules.
- `src/error.rs` — the crate-wide `SwarmError` enum (`InvalidCapacity`,
  `BusFull`, `BusClosed`, `DuplicateSubscriber`, `UnknownSubscriber`,
  `UnknownAck`).
- `src/bus.rs` — `MessageBus`: four-tier strict-priority inboxes
  (`MessagePriority` Low/Normal/High/Urgent, O(1) dequeue from the highest
  non-empty `VecDeque` tier), per-worker `subscribe` with `MessageKind`
  filters, directed `send` + `broadcast` fanout (sorted recipients and atomic
  capacity/eviction preflight; filtered deliveries consume no capacity), TTL
  expiry before ACK creation, acknowledgment tracking cleaned on unsubscribe
  and immediate close,
  deterministic bounded eviction (oldest message by global admission
  sequence in the lowest tier below the incoming priority — Urgent is
  never evicted; a lowest-priority arrival with nothing lower to evict is
  refused loudly via `SwarmError::BusFull`), bounded diagnostic event log,
  non-blocking `try_recv`, and task-parking `recv` woken by per-inbox
  `Arc<Notify>` registered before rechecking state. Close drops queued traffic
  and wakes every receiver with `BusClosed`; unsubscribe wakes removed readers.
  `MessageBus` is `Clone` (shared state handle). ACK debt is capped at message
  capacity and lazily expires at the original message deadline; saturation
  refuses delivery without popping it. Resource ceilings: 1 MiB payload,
  64 MiB aggregate queued payload/identity bytes, 4,096 subscribers, 256-byte
  identities, 32 filters, 24-hour TTL and 1,000,000 configured message slots.
  Byte pressure uses the same lower-priority eviction policy; broadcasts
  preflight both count and bytes. Limits and identity overflow fail loudly.
  `MessageBus::with_clock` accepts a cheap nonblocking monotonic `BusClock`;
  clones share it. Default construction uses the system monotonic clock.
- `tests/bus_clock_regressions.rs` — queue/ACK expiry at exact injected time,
  without sleeps and across cloned handles.
- `tests/bus_bounds_regressions.rs` — payload/TTL refusal, ACK cap preserving
  queued delivery, aggregate byte-budget broadcast refusal and ACK expiry.
- `tests/bus_tests.rs` — integration battery: strict priority
  ordering (urgent-last-dequeued-first), FIFO within tiers, the eviction
  matrix (low dropped first, oldest-by-sequence wins, urgent never,
  normal-before-high, new-message self-refusal), loud backpressure, TTL
  discard semantics, filters/broadcast fanout, ack lifecycle, subscriber
  lifecycle, close semantics, parked-recv wakeup, and a 50k-message
  volume check.
- `src/hive/` — hive orchestration semantics (PR-5): `assignment.rs` owns
  the pure capability-scoring function (the upstream oracle's formula
  over Vesper capability vectors: `(100 + 50*type_match -
  20*workload)*health + 10*success_rate - 5*(avg_turn_secs/60)`), stable
  `select_best` (ties to the earliest eligible candidate; missing required
  capabilities, zero capacity, saturation, dead workers, and invalid metrics
  are refused), and the task→bus
  priority mapping (`Critical→Urgent, High→High, Normal→Normal,
  Low/Background→Low`); `timeout.rs` owns `execute_bounded` — the
  timeout/cancel/grace boundary for dispatched turns. `execute_bounded` takes
  a signal-consuming future factory; `execute_bounded_port` takes a worker port,
  not an already-constructed future. Expiry and caller drop cancel the exact
  signal passed to the worker; deadline wins simultaneous readiness. A bounded
  grace window lets the worker unwind (signal-ignorers are abandoned with `DeadlineExceeded`, and a success
  observed after cancellation is rewritten to `Cancelled` — a cancelled
  or timed-out task never yields a successful receipt).
- `src/ledger/` — the swarm's ephemeral shared-memory ledger. PR-6 owns
  `hnsw.rs`: the pure-Rust HNSW index (oracle defaults `m=16`,
  `ef_construction=200`, `max_elements=1_000_000`, cosine similarity,
  runtime-configured dimensions; deterministic xorshift64* geometric
  level draws at `p=0.5` capped at 16 layers; insert with bidirectional
  links and cap pruning `2m` at layer 0 / `m` above; greedy top-down
  descent then best-first `ef` layer search; filtered search via
  `over_fetch_factor` so predicates never guide traversal; versioned
  little-endian binary snapshot `VSWHNSW1` with strict header/length/
  ordinal validation, input-byte allocation budgets, unique identities,
  finite vectors, caller seed/capacity agreement, entry/header level consistency
  and neighbor layer/self/duplicate-edge checks). Snapshot version 2 preserves
  raw vectors, exact RNG state and all semantic configuration; version 1 is
  refused because it lacks those fields. Finite vectors normalize via f64;
  nonfinite input is refused; adjacency pruning ranks against its owner.
  Cloned graphs share immutable node Arcs; insertion uses copy-on-write before
  changing any adjacency, preserving earlier graph generations and snapshot bytes.
  Seeded byte mutations and every truncation are tested for refusal or valid,
  deterministic continued insertion. High-dimensional scale remains bounded by
  the explicit workload evidence.
  **Anti-duplication audit (PR-6):** the workspace's public cosine and
  embedding ports live in `vesper-cognition` (`score.rs::cosine`,
  `ports.rs::EmbeddingPort`); the architecture allowlist keeps
  `vesper-swarm` independent of that crate, so `hnsw.rs` keeps its math
  **private** (no second public cosine is exported), takes raw `&[f32]`
  vectors, and defines no embedding port in this PR (the ledger's async
  port is PR-7 scope). The ledger is ephemeral and swarm-scoped: it
  never touches `vesper-memory` durable state.
- `src/ledger/store.rs` (PR-7) — the hybrid composition over `hnsw.rs`:
  `EmbeddingPort` (async `embed(Vec<BoundedText>) -> Vec<Vec<f32>>`
  seam, composition-boundary; deterministic fake in tests), `BoundedText`
  = reused `vesper_domain::ContentText` (not a duplicate type),
  `MemoryScope::{Swarm, Worker(id), Task(id)}` with strict isolation,
  `EntryDraft`/`LedgerEntry`/`Provenance`/`EntryKind`,
  dual-write `record` (vector side first; any embedding/dimension/
  capacity rejection leaves **no trace** on either side),
  auto-routing `query` (`Exact`/`Filtered` → structured log; `Semantic`
  → HNSW; `Hybrid` → merge with `SEMANTIC_THRESHOLD = 0.7`,
  `HYBRID_MAX_RESULTS = 100`, **exact matches win ties and rank first**),
  and `transfer` (copy, never move; `TRANSFER_CONFIDENCE_FLOOR = 0.8`,
  `TRANSFER_CAP = 20`; low-confidence entries are dropped-and-reported;
  provenance preserved verbatim — worker, role, task, sequence and original timestamp never
  rewritten; fresh id per copy). Transfers reuse original embeddings and stage
  the complete batch before publication. The ledger is `Clone`: `ArcSwap`
  publishes one coherent immutable log/index generation; a writer-only mutex
  serializes staged changes. `LedgerSnapshot` retains prior generations without
  locking writers; hybrid query components use the same generation. Structured
  entry payloads use immutable Arcs and graph nodes use copy-on-write; generation
  maps/ordinal tables still clone. Bounded 10k/16D measurement is recorded in the
  repair evidence, not a million-entry/host-latency claim. `VSWLEDG1` version 3
  whole-ledger snapshots combine HNSW v2 with the structured log, next identity
  and required retention policy; version 1 is refused rather than guessing policy;
  load validates identities/counts/confidence without invoking embeddings. Input
  and output snapshots are capped at 64 MiB. Output checks HNSW encoded size
  before allocation and streams borrowed log entries into a bounded memory sink
  (`src/ledger/snapshot_writer.rs`) instead of cloning/serializing an unbounded log.
  Sparse visited membership replaces full-index per-layer scratch allocation.
  Broader corruption/scale and generation-clone acceptance remain open.
  Identity exhaustion fails transactionally rather than wrapping.
- `src/ledger/filter.rs` owns validated conjunctive worker/task/role, category,
  original-sequence range and confidence predicates. `Ledger::select` and retained
  `LedgerSnapshot::select` return newest admissions first, capped at 100.
  `semantic_filtered` applies predicates after graph traversal and before top-k;
  semantic routes honor configured HNSW over-fetch and the 0.7 threshold.
  Approximate filtering may return fewer results than requested; it never expands
  the configured candidate budget implicitly. Optional Unix-millisecond timestamp
  ranges exclude unknown times; `record_at` accepts explicit original timestamps
  and `with_timestamp_source` injects a cheap host clock. Transfers/snapshots retain
  original times; v3 writes prevent old readers from silently losing timestamps.
  Legacy v2 logs load with unknown time and migrate to v3 on export, never sequence-derived
  values. Native Hive composition supplies wall time; pure defaults remain unknown.
  `transfer_filtered` validates all explicit source IDs even if filtered out,
  reports category/confidence exclusions and cannot lower the 0.8 floor.
- `Ledger::prune_scope` is explicit caller-owned retention, not automatic cap
  enforcement. Swarm evicts lowest confidence then oldest admission; Worker/Task
  use FIFO. A single publication replaces log, exact index and rebuilt HNSW;
  original vectors/identities and retained snapshots survive without embeddings.
  Rebuild reclaims capacity without tombstones; rebuild/clone scale remains an
  acceptance gap.
- `src/ledger/retention.rs` defines persisted `LedgerRetention`; `with_retention`
  opts into positive per-Swarm/per-Worker/per-Task caps bounded by global capacity.
  Legacy constructors use explicit Disabled policy. Record admission and complete
  transfer batches reserve receiving-scope capacity transactionally, evicting only
  existing entries in that scope under the same confidence/FIFO rules as pruning.
  Batch size above the destination cap fails without publication; successful
  transfers never return IDs evicted within the same batch. Other scopes are never
  evicted to satisfy global pressure. Failed vector admission rolls eviction back.
  Snapshot load validates required policy, cap ranges and per-scope entry counts.
- `tests/ledger_scale_regressions.rs` contains explicit release-mode measurements:
  10k appends at 16 dimensions with retained-reader, snapshot and exact continued
  insertion checks; 1k automatic evictions at a 64-entry cap with FIFO/count checks.
  These are ignored in ordinary runs and must be explicitly invoked for scale
  evidence. The 2048-entry/1536D gate uses the default million-entry capacity,
  immutable readers and exact reload/continuation. A million 1536D raw+normalized
  vectors alone need 12.288 GB; configured capacity is not a memory certification.
  Wall-clock measurements are observations, not portable timing thresholds.
- `tests/ledger_retention_regressions.rs` covers persisted automatic admission,
  private FIFO/isolation, immutable readers, nonfinite-vector rollback, global
  pressure refusal, full-batch reservation and malformed/missing/old policy refusal.
- `tests/ledger_filter_regressions.rs`, `tests/ledger_eviction_regressions.rs`,
  `tests/ledger_semantic_regressions.rs` cover conjunction/isolation, retained
  generations, transactional selective transfers, invalid bounds, result caps,
  eviction order, capacity reclamation, snapshot round-trip after eviction,
  documented semantic threshold and configured over-fetch.
- `tests/ledger_tests.rs` — 15-test integration battery: dual-write
  admission + fail-closed (confidence bounds, dimension mismatch,
  embedding-port failure — no partial writes), strict Worker/Task/Swarm
  isolation across exact/semantic/filtered queries, hybrid exact-wins
  tie-breaking with identical embeddings, router dispatch by query
  shape, transfer bounds (0.79 dropped & reported, cap 21 refused, cap
  20 allowed, same-scope and foreign-entry refusal, unknown id loud),
  provenance-verbatim copies, oracle defaults, and concurrency: 4
  writers × 4 readers interleaved across all query shapes with
  entry-integrity invariants at every observation point, plus
  concurrent bounded transfers with exact final-count accounting.
- `src/sandbox.rs` (PR-8) — sandbox lease coordination, the policy layer
  over `vesper-security` capability types (`IsolationRequirement`,
  `SandboxCapabilities` — the crate's second foundational dependency,
  allowlisted since PR-1): `SandboxLeasePort` (composition-boundary
  acquire/release seam), `LeaseSpec`/`LeaseMode`/`NetworkGrant` with
  strict `can_share` (same group, same requirement, same grant
  provenance, component-disjoint absolute logical paths checked against every
  current member; traversal and ambiguous forms refused, case-folded for
  conservative collision detection), `Lease` RAII guard, and `LeaseBook`
  — **boundary-based accounting** (a shared group is ONE boundary
  however many members; capacity counts boundaries, never members),
  pre-spawn fail-closed capability gate (`unmet_axis` diagnostic with
  `Unknown`/`Unavailable` = denial; zero-capacity refused), FIFO queue
  on exhaustion (never over-provision; shared joins are exempt from the
  capacity count), cancellation-safe parked waiters (a dropped acquire
  removes its queue entry; dispatched work retains ownership), and `close()`
  failing queued/in-flight callers with `Closed`. Backend calls run on the captured
  Tokio blocking pool outside the book lock. Admission reserves identity, group,
  member and boundary capacity before dispatch; commit rejects cancelled/closed
  publication and schedules late-success teardown. At most one operation per
  boundary is in flight. A hung call keeps capacity and its owning task; dropping
  a caller or join observer does not cancel it or certify physical cleanup.
  Member Drop schedules release. `settle`/`shutdown` report held, pending,
  quarantined and queued state; deadlines bound observation only. Only verified
  teardown frees capacity or increments releases. Runtime-abandoned operations
  quarantine and close admission. `ProvisioningUncertain` explicitly preserves
  partial provisioning; ordinary acquisition refusal must mean no resources exist.
  Async `retry_quarantined` reserves at most 4,096 quarantined boundaries once
  each and observes per-operation completion, preventing concurrent duplicate
  cleanup. The port defaults to retry refusal; implemented recovery must be safe
  and idempotent after partial cleanup. Backend unwind closes admission without
  poisoning the book. Abort/double-panic and arbitrary backend destructors remain
  outside this containment guarantee.
  `tests/lease_async_regressions.rs` covers reentrant inspection (failed before
  repair), blocked acquisition/release, timeout, dropped observers, close/late
  cleanup, partial provisioning, single retry ownership and queued shared joins.
  Existing recovery/panic/identity regressions observe explicit cleanup completion.
  Admission caps boundaries, reserved members and waiters at 4,096 each;
  worker/group identities at 256 bytes, grants at 1,024 bytes, paths at 4,096
  bytes, wait budgets at 24 hours, and diagnostics at 64 × 4,096 bytes.
  Origins remain reserved through provisioning and quarantine. Shared joins
  check every live member and never join an unverified boundary. Filesystem alias
  enforcement and real supervisor execution belong to the native composition.
- `tests/sandbox_tests.rs` — integration battery (including nested-path,
  pairwise-member and failed-teardown regressions):
  `FakeSandboxLeasePort` with exact acquire/release pair accounting and
  live-boundary tracking, the capability denial matrix (full backend ✓
  ×5, process-only denied naming the axis ×3, Unknown = denial),
  zero-capacity refusal, exhaustion-queues-never-over-provisions with
  live-count proof, strictly-FIFO handoff, shared-group joins without a
  new boundary slot (port sees one acquisition), all three
  shared-refusal rules named exactly, group dissolution freeing the
  name, scope-drop releasing everything with exact acquire/release
  pairing, panic-path teardown, close-fails-queued-waiters, port
  refusal reserving nothing, and bounded waits.
- `src/hive/orchestrator.rs` (PR-9) — `HiveOrchestrator` (`Hive`):
  composes topology (navigator joins as Queen, driver classes as
  Workers, class-namespaced worker ids with bus inboxes subscribed at
  admission), per-role `RoleProfile`s (`navigator()`: decomposition +
  scored assignment + ledger reads + synthesis; `driver(tools)`: bounded
  turns with explicit toolsets), the assignment pipeline (task →
  `bus_priority` mapping → bus send at the mapped tier → driver drain →
  `run_turn` → trajectory `record` into the Swarm ledger scope with
  provenance → load-snapshot updates for scoring), a bounded
  `HiveEvent` log, and the caller-driven `run_tick`/`run_to_completion`
  loop (the host's `/swarm` task owns the loop; pool retirement uses owned
  blocking tasks and never touches a render thread). All three turn phases use the shared
  deadline/cancellation boundary and reject unsuccessful, mismatched or oversized
  receipts. Admission caps: 128 queued goals, 1,024 lifetime unique identities,
  256-byte IDs, 64 KiB prompts, 64 decomposed tasks, 512 KiB synthesis evidence.
  `submit` returns a refusal on overflow/duplicate identity. Role bounds are
  validated; interrupted goals remain inspectable and block automatic replay
  after caller drop/error. Synthesis receives completed task outputs explicitly.
  `src/hive/decomposition.rs` accepts strict bounded JSON task prompts, required
  capabilities and indexed dependencies (acyclic, deterministic ordering); it
  refuses count-only output and preserves prerequisite outputs in task context.
  Selection consumes per-instance scores over the intersection of role tools and
  execution-port capabilities. `src/hive/routing.rs` gates candidates by directed
  reachability from the active elected navigator; missing/failed intermediates and
  invalid edge weights cannot grant routes, while declared bidirectional edges do.
  Dispatch transport remains direct in-process delivery, not simulated relay turns.
  Each task consumes exactly its issued bus message ID plus actual sender/recipient,
  kind and payload; an unrelated identical-prompt message interrupts without worker
  execution or automatic replay. `routing_tests.rs` proves disconnected refusal in
  all four topologies; routing unit tests cover direction/liveness/weight gates and
  `tests/hive_tests.rs` covers identical-prompt assignment substitution.
  Ready independent tasks execute in bounded concurrent waves, at most one per
  distinct execution-port Arc; aliasing a port under class names never multiplies
  capacity. Prerequisites must finish before dependent dispatch. Evidence remains
  in admission order within each wave; failed siblings or caller drop cancel all
  polled turns and leave the goal interrupted. Ledger embedding writes use the
  issuing role's deadline and do not publish when the embedding future is dropped.
  `src/hive/pools.rs` owns `Hive::with_factories`: validated role pools boot
  concurrently and concrete pool IDs drive topology, bus, scoring and provenance.
  Instance wrappers atomically acquire that exact worker and propagate outer
  cancellation through the shared pool execution boundary. Cross-role Arc aliasing
  is rejected. `Hive::new` accepts only one supplied instance per class and refuses
  inflated worker floors. Topology admission stages mutations, rolls back new
  subscriptions on failure and is idempotent after success. Close/drop closes pools
  and bus; closed Hive rejects submission, admission and execution.
  `src/hive/lifecycle.rs` adds caller-driven `scale_role` and `maintain_workers`.
  Scaling fixes navigator cardinality, refuses interrupted/unadmitted/legacy/closed
  hives and preflights topology capacity. Reconciliation stages topology, adjusts
  inbox membership, rechecks cross-pool Arc aliasing and preserves surviving load
  history before replacing concrete routes. Post-boot reconciliation failure closes
  the hive rather than allowing stale dispatch. Health passes take caller monotonic
  time and never invent heartbeats. Navigator failure with disabled failover closes
  the hive; otherwise failed pools replace and reconcile. Dropped/failed maintenance
  closes all pools and bus, since retired IDs cannot safely remain dispatchable.
  Native monitor/heartbeat integration, verified sandbox leases/teardown and native
  host overlap acceptance remain gaps; close does not prove detached cleanup.
- `tests/hive_scaling_regressions.rs` covers real grow/shrink dispatch/provenance,
  retired inbox removal, capacity/cardinality refusal, post-boot inbox-conflict
  closure, health replacement IDs, disabled navigator failover and manually polled
  cancellation during replacement with stale-route refusal.
- `tests/hive_pool_regressions.rs` — actual 1+3 pool overlap, exact assignment and
  ledger provenance, cross-role alias refusal, cancellation/no replay, notification
  ordering, nominal-slot refusal and transactional/idempotent topology admission.
- `tests/hive_concurrency_regressions.rs` — barrier-proven three-port overlap,
  dependent output ordering, sibling cancellation/no replay and alias capacity.
  This is execution-port evidence, not native host/pool lifecycle acceptance.
- `tests/hive_boundary_regressions.rs` — actual cancellation signals and retained
  goals at all three hanging phases, unsuccessful receipt refusal, caller drop,
  and hanging embedding refusal without publication or replay.
- `tests/assignment_regressions.rs` — desired-behavior audit regressions for
  whole-base health scaling and fail-closed candidate eligibility. The adjacent
  `assignment_oracle_vectors.json` contains 72 outputs captured by executing the
  pinned oracle scoring method (commit and method SHA-256 recorded), with only
  TypeScript annotation removal and milliseconds/seconds input conversion. The
  test maps oracle type matching to Vesper capability inclusion; it does not claim
  equivalence of their type systems. Hive routing has separate integration tests.
- `tests/hive_tests.rs` — synthetic pipeline battery: one navigator port and
  one driver port across mesh and hierarchical topologies (decomposition →
  3 bus assignments → 3 sequential driver turns → 3 ledger trajectories →
  synthesis; exact turn counts, event pipeline order, ledger contents,
  exact-key trajectory queries), sequential multi-goal draining, failed
  driver turns surfacing as worker errors, empty-queue no-op, assembly
  refusals (missing ports, navigator-not-first), and Queen/Worker
  topology role structure.
- `tests/hnsw_regressions.rs` — malformed header level, caller capacity/seed,
  nonfinite vector and count/input-budget refusal regressions.
- `tests/hnsw_tests.rs` — 13-test integration battery: the directive's
  Recall@10 ≥ 0.95 bar on 10k seeded vectors vs brute-force cosine
  (measured 0.997 @ ef=16, 1.000 @ ef≥64), ef scaling, byte-identical
  determinism per seed, seed divergence, lossless snapshot round-trip
  preserving search results, fail-closed corruption matrix (bad magic,
  bad version, truncation at six offsets, trailing bytes, header
  disagreement), filtered over-fetch semantics, capacity enforcement,
  post-reload insert continuity, and a 10k build/query runtime guard.

- `src/hive/governance.rs` (VRO-16 PR-1 + audit fixes) — the pure
  task-level governance state machine: `GovernanceConfig`/`GovernanceProfile`
  (Auto/Gated/Custom) with bounded countdowns (60 s–1 h; `Governor::new`
  refuses outside), `HostCommand` (Resume/Redirect/Fail/Cancel; the one
  shared verb surface both hosts delegate to), `GateRecord` with
  `opened_at_ms` and **derived** remaining time (never a ticking timer),
  D1 `expire_due(now_ms)` firing the pre-configured `FailTask` fallback,
  the `AuditEvent` audit stream (gates, decisions, verification verdicts,
  budget thresholds; append-only, bounded; `goal_id()` derives the owning
  goal from the event so persistence never depends on live run state),
  the `ReceiptSignals` SmartPause evaluator (observable-only), and
  `Governor::replay` reconstruction. Audit fixes: `resolved_gate_ids()`
  gives boundary gates once-per-task-id semantics; `goal_of_gate` cuts at
  the last `-task-` boundary. Final-audit proofs live in
  `tests/hive_governance_audit_fixes.rs` (G2 real bus inbox, G3 Cancel
  persistence + ledger round-trip, G4 boundary enforcement, G5
  directive-to-prompt, G1 panel composition, G9 budget-fed SmartPause).
- `tests/hive_governance_regressions.rs` (VRO-16 PR-1 proofs) — D1
  injected-clock expiry with `GateExpired` audit and no-open-gate-after
  (silent host never hangs the hive); D2 two-party barrier proving
  sibling tasks execute while one task sits suspended at a gate;
  restart reconstruction with identical derived countdown; verb-surface
  stability; bounded-timeout refusal; and governance-off hives keeping
  exact VRO-15 semantics (no gates, no audit events, failed receipts
  error the tick).
- `src/hive/mod.rs` re-exports the governance surface; `MessageKind` gains
  `Governance` (gate publication rides the Urgent tier — never evicted,
  dequeued before ordinary work); `EntryKind` gains `Audit` for ledger
  persistence of gate events; `RoleProfile` gains `model_binding`
  (D3 preparation: author/judge role routing hint, prompt-separated by
  construction) and `RoleProfile::judge()` with structurally distinct
  evaluation-only instructions. The orchestrator's in-flight goal became a
  re-entrant `ActiveRun` (assignments/results/evidence/failed-tasks/
  pending-directive/suspended): with governance enabled, a failed receipt
  validated by `run_receipt_for_governance` (identity/size still strict,
  `success: false` passed through) re-dispatches once, then opens a gate
  and suspends **only that task** — siblings keep dispatching, dependents
  stop at the dependency wall, the tick returns `Ok(false)`, and
  `resolve_host_command` (Resume/Redirect/Fail/Cancel) or expiry resumes
  the run. An errored tick (no open gate) keeps the stricter VRO-15
  `Interrupted` no-replay contract. Without `with_governance` the hive is
  byte-for-byte VRO-15 behavior.
- `crates/vesper-harness/src/swarm_gate_surface.rs` — ONE shared
  gate-command parser (`gate <task> resume|redirect <directive>|fail
  <reason>|cancel`), countdown renderer (`3m 25s remaining`) and audit
  renderer; both hosts delegate (parity by construction).
  `crates/vesper-harness/tests/swarm_gate_parity.rs` proves the shared
  surface, `/swarm gate` routing through `SwarmControls`, and the
  settings round-trip (draft edits never touch the filesystem; Save
  persists once; legacy settings files default to Auto).
- `crates/vesper-harness/src/swarm_service.rs` — `GateChannel`
  (queued host commands in, derived gate snapshots out) shared between
  hosts and the running hive's tick loop (drained between ticks, never
  spun); `SwarmRunReport.gate_events` carries the audit trail to both
  hosts; the run loop polls at a bounded 250 ms while a gate is open.
  `swarm_settings.rs` persists `governance: auto|gated` (serde default
  keeps legacy files valid) surfaced in the TUI Settings › Swarm hub and
  the ACP `/swarm settings` verbs — activation is native-Settings-only.

- `src/hive/decision.rs` (VRO-16 PR-2) — the Navigator decision node:
  `DecisionConfig` (default 3 refines / 2 pivots; caps above the defaults
  are refused so configuration cannot manufacture an unbounded loop),
  strict-JSON verdict parsing (`Proceed`/`Refine{amended,iteration}`/
  `Pivot{tasks}`/`ProceedWithFailure`), and `DecisionEngine::issue` —
  which rewrites an over-cap refine/pivot to the audited
  `ProceedWithFailure` verdict (cap exhaustion is always an explicit
  audit event, never silent; malformed decisions and dangling
  `rationale_refs` fail closed and consume no iteration). Amended task
  sets are validated against the decomposition bounds (1–64 tasks,
  bounded prompts/capabilities/dependencies). `VersionRegistry`
  preserves one `LedgerSnapshot` per decision (`push_version`,
  `version(goal,index)`, `resolves_any` as the rationale resolver) so
  any iteration's evidence stays retrievable; `LedgerSnapshot::has_entry`
  backs fail-closed reference checks. `decision_tests.rs` proves the
  caps, the audited exhaustion path, dangling-ref refusal, malformed
  refusal, cap-inflation refusal and cross-version retrieval against a
  real ledger.
- `src/hive/panel.rs` (VRO-16 PR-2) — the Driver-role review panel:
  frozen-snapshot rounds (`RoundInput::frozen` builds each reviewer's
  input from positions recorded **before** the round — same-round peers
  are structurally invisible, not conventionally hidden), bounded rounds
  (`MAX_ROUNDS = 2`, ≤ 8 reviewers), drop-on-double-failure (single
  failures tolerated; round-≥1 empty output keeps the prior position;
  round-0 empty counts as a failure attempt), and `PanelOutcome::ZeroPanel`
  — a panel reduced to zero members is a verification failure, never a
  silent acceptance. `panel_tests.rs` proves same-round isolation
  (observed inputs), double-failure drops, zero-panel failure, empty-
  rebuttal retention and the round bound.
- Orchestrator PR-2 wiring — `RoleProfile::template_identity()` hashes
  the **authored surface** (instructions + tools; the name is excluded
  so relabeling an author as "judge" cannot dodge D3), `with_decision`
  refuses any two roles with identical surfaces at assembly, and
  `drive_run` runs a decision turn after synthesis when decisions are
  enabled: it records the pre-decision ledger version, issues the
  verdict with rationale refs resolved against all recorded versions
  (fail-closed), audits it into the unified stream (`gate_events()`
  unions Governor gate events and decision events), and applies
  Refine/Pivot by rebuilding the run's assignments (goal re-dispatch,
  never a silent loop) or completes on Proceed/ProceedWithFailure.
  `run_to_completion` counts only completed goals and keeps driving
  through `Ok(false)` continuation ticks. The harness composes
  `with_decision` with default caps on every governance-enabled run.
  `tests/hive_governance_regressions.rs` PR-2 proofs: refine loop
  bounded through the real orchestrator with the exhaustion verdict
  audited and versions retained; judge/author hash-distinctness plus the
  relabeled-impostor refusal and real-judge acceptance; evidence
  versions retrievable after decisions.

- `src/hive/verify.rs` (VRO-16 PR-3) — deterministic verification gates:
  `artifact_digest` (FNV-1a 64 over task-id/output with a separator and
  length suffix — concatenation cannot forge collisions) records each
  artifact's content hash at admission; `EvidenceBook` admits once per
  task (double admission refused), binds the published ledger entry id as
  the trace target, and re-verifies digests before synthesis admission
  (`DigestMismatch`/`UnknownTask` failures carry the recorded/recomputed
  pair). `extract_citations`/`verify_traces` enforce the `[evidence:<id>]`
  citation contract — every cited id must resolve to an admitted
  artifact's ledger entry; dangling citations fail closed.
  `BudgetWatchdog` evaluates caller-supplied `BudgetReading`s against a
  `BudgetCeiling` (binding axis = the axis closest to its ceiling):
  50%/80% emit `BudgetThreshold` audit events without stopping, 100%
  returns `Exhausted` and each level fires exactly once (skipped levels
  emit retroactively in ascending order). `verify_tests.rs` proves digest
  determinism/tamper-sensitivity, double-admission refusal, dangling-
  citation fail-closed, and the complete threshold matrix including the
  time-axis-only stop.
- Orchestrator PR-3 wiring — `with_verification(ceiling)` (nonzero on at
  least one axis) enables the gates; evidence admission records digests
  and binds ledger entry ids after trajectory publication; synthesis runs
  both gates before acceptance (digest failures and dangling citations
  each fail the goal with an audited `VerificationVerdict`); per-turn
  budget accounting (output bytes proxy tokens, reported duration
  proxies elapsed) hard-stops the goal with truthful partial state and a
  `TurnCompleted(<goal>-budget-stop, false)` event when 100% crosses.
  `budget_exhausted()` surfaces termination; the harness composes
  verification on every governance-enabled run and reports
  `SwarmRunReport.budget_exhausted`. `render_audit` covers
  `VerificationVerdict` and `BudgetThreshold`.
  `tests/hive_governance_regressions.rs` PR-3 proofs: trace gate accepts
  real citations and fails closed on hallucinated ones (with audited
  failure verdicts), digest rejection of tampered artifacts, watchdog
  50/80/100 emissions with no duplicate levels, orchestrator hard-stop
  with the audited 100% event, and the complete exact-match-filterable
  audit chain. `tests/swarm_pipeline_e2e.rs` (harness) drives the full
  Driver → panel → Navigator → gates pipeline over the real orchestrator
  composed exactly as the service composes it, pins the audit-lifecycle
  anchor text both hosts render through the ONE shared `render_audit`,
  and the ACP process test (`swarm_controls.rs`) proves `/swarm audit`
  reaches that renderer through the real JSON-RPC process.

## Local Contracts

- Shared members arriving during dispatched origin provisioning queue until its
  commit; teardown/quarantine and grant mismatches refuse.

- Factory instance retirement runs in owned blocking tasks outside bookkeeping
  locks. `settle_retirements` and Hive `settle_workers` observe completion;
  caller cancellation/timeout does not free unfinished retirement admission.
  Destructor panic closes admission and retains uncertainty. The captured native
  runtime must remain available through settlement; observation is not preemption.
- Runtime deadlines, grace and pool heartbeat time use Tokio's injectable clock;
  deterministic paused-time tests cover deadline/grace and silence boundaries.
  Bus TTL uses its separate injected monotonic `BusClock`. Native record timestamps
  come from the host wall-clock port and never drive deadline or ordering policy.

- `WorkerPort::pending_work` is a nonblocking lifecycle observation. Ports with
  owned work after caller drop must keep it true until native work settles. Pool
  shrink/replacement observes it outside bookkeeping locks and refuses retirement
  while work remains; ordinary pure caller-owned ports use the false default.

- Utility dependencies include `serde`, `serde_json`, `thiserror`, `futures-util`, `tokio`
  and pinned `arc-swap` for coherent lock-free snapshot publication. New
  dependencies require license/advisory and MSRV acceptance.
  Architecturally permitted: `vesper-domain` and `vesper-security`; nothing
  else ever.
- No network/filesystem I/O or process spawning; no provider crates,
  `vesper-testkit` or frontend crates. Timing policy is stated above; only the
  physical retirement observer uses a wall-clock bound.
- `#![forbid(unsafe_code)]`; MSRV 1.88; workspace lints apply.
- The upstream is referenced ONLY as *the swarm oracle*. The upstream-brand
  embargo is mechanically enforced by `cargo xtask naming-guard` against
  `xtask/naming-guard-baseline.json` (fail-closed ratchet; new hits block
  merge).
- Divergences from the upstream model are deliberate and documented in
  source doc comments: `auto_rebalance` and `failover_enabled` default to
  `false`, and topology state carries no wall-clock timestamps.
- Integration is default-off: `vesper-harness` uses an optional dependency
  enabled by its `swarm` feature. Native host activation remains acceptance-gated.

## Work Guidance

- Source of requirements: `docs/swarm-oracle-extraction-prd.md` (§1.3 for the
  data model, §1.4 for lifecycle semantics). PR-1 delivered the topology
  model; PR-2 delivered the manager. Determinism rules: admission order is
  the single ordering authority; hash placement uses the inline FNV-1a in
  `manager.rs` (never std hasher internals); tests use the in-file xorshift
  PRNG (never wall-clock or unseeded randomness).

## Verification

- `cargo test -p vesper-swarm`
- `cargo test -p vesper-swarm --release --test ledger_scale_regressions -- --ignored --nocapture`
- `cargo clippy -p vesper-swarm --all-targets -- -D warnings`
- `cargo tree -p vesper-swarm` (purity: only permitted deps)
- `cargo xtask naming-guard`
- `cargo xtask architecture`

## Child DOX Index

No children.
