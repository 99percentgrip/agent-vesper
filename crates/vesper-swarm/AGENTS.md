# vesper-swarm — pure swarm-coordination foundations (VRO-15)

## Purpose

Own the provider-neutral, pure-logic swarm coordination foundations
extracted from *the swarm oracle* (an authorized upstream orchestration
repository): the topology data model now, and in later VRO-15 PRs the worker
pool, priority message bus, task assignment, shared memory ledger, and
sandbox lease coordination. The crate is coordination logic only — it never
executes turns, never performs I/O, and never names a provider.

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
- `src/worker.rs` — `WorkerPort` (the execution seam: `run_turn` returning
  a `BoxFuture` of `TurnReceipt` under a `CancellationSignal`),
  `CancelFlag`/`CancellationSignal` cooperative cancellation pair,
  `WorkerCapabilities`, `WorkerTask`/`TaskKind`/`TaskPriority`,
  `TurnReceipt`, `WorkerError`.
- `src/pool.rs` — `WorkerPool` over `Arc<dyn WorkerPort>`:
  `PoolConfig` (validated bounds/timings), parallel `initialize` (one
  `join_all` wave to `min_workers`), lease-guarded `acquire`/`release`
  (drop releases; release never resurrects a `Failed` worker),
  `run_task` (deadline race, cancelled/timed-out tasks never report
  success), `scale` within bounds (busy workers never shed; floor
  `min_workers`), `health_tick` (fails silent workers and overdue
  in-flight, cancelling the task first) + caller-owned
  `heartbeat_interval`, `replace_failed`, bounded event log.
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
  filters, directed `send` + `broadcast` fanout (filtered deliveries
  consume no capacity), TTL expiry at dequeue, acknowledgment tracking,
  deterministic bounded eviction (oldest message by global admission
  sequence in the lowest tier below the incoming priority — Urgent is
  never evicted; a lowest-priority arrival with nothing lower to evict is
  refused loudly via `SwarmError::BusFull`), bounded diagnostic event log,
  non-blocking `try_recv`, and task-parking `recv` woken by per-inbox
  `Arc<Notify>`. `MessageBus` is `Clone` (shared state handle).
- `tests/bus_tests.rs` — 25-test integration battery: strict priority
  ordering (urgent-last-dequeued-first), FIFO within tiers, the eviction
  matrix (low dropped first, oldest-by-sequence wins, urgent never,
  normal-before-high, new-message self-refusal), loud backpressure, TTL
  discard semantics, filters/broadcast fanout, ack lifecycle, subscriber
  lifecycle, close semantics, parked-recv wakeup, and a 50k-message
  volume check.
- `src/hive/` — hive orchestration semantics (PR-5): `assignment.rs` owns
  the pure capability-scoring function (the upstream oracle's formula
  ported exactly over Vesper capability vectors: `100 + 50*type_match -
  20*workload*health + 10*success_rate - 5*(avg_turn_secs/60)`), stable
  `select_best` (ties to the earliest candidate), and the task→bus
  priority mapping (`Critical→Urgent, High→High, Normal→Normal,
  Low/Background→Low`); `timeout.rs` owns `execute_bounded` — the
  timeout/cancel/grace boundary for dispatched turns (budget expiry
  fires the `CancelFlag`, a bounded grace window lets the worker unwind,
  signal-ignorers are abandoned with `DeadlineExceeded`, and a success
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
  ordinal validation that fails closed on any mismatch).
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
  provenance preserved verbatim — worker, role, task, sequence never
  rewritten; fresh id per copy). The ledger is `Clone` (`Arc` shared
  state, single interior mutex; no clock, no randomness).
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
  provenance, disjoint write paths), `Lease` RAII guard, and `LeaseBook`
  — **boundary-based accounting** (a shared group is ONE boundary
  however many members; capacity counts boundaries, never members),
  pre-spawn fail-closed capability gate (`unmet_axis` diagnostic with
  `Unknown`/`Unavailable` = denial; zero-capacity refused), FIFO queue
  on exhaustion (never over-provision; shared joins are exempt from the
  capacity count), cancellation-safe parked waiters (a dropped acquire
  removes its queue entry via an armed-guard `Drop`; no `mem::forget`
  leaks), `close()` failing queued waiters with `Closed`, and guaranteed
  teardown: member `Drop` → last-member boundary release through the
  port → waiter fulfilment; panic unwinds run the same path.
- `tests/sandbox_tests.rs` — 13-test integration battery:
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
  loop (the host's `/swarm` task owns the loop; the hive spawns nothing
  and never touches a render thread). Fail-loud: a failing driver turn
  aborts the tick with the worker error — no silent retry.
- `tests/hive_tests.rs` — 8-test synthetic e2e battery: 1 Navigator +
  3 Drivers across **mesh and hierarchical** topologies (decomposition →
  3 bus assignments → 3 scored driver turns → 3 ledger trajectories →
  synthesis; exact turn counts, event pipeline order, ledger contents,
  exact-key trajectory queries), sequential multi-goal draining, failed
  driver turns surfacing as worker errors, empty-queue no-op, assembly
  refusals (missing ports, navigator-not-first), and Queen/Worker
  topology role structure.
- `tests/hnsw_tests.rs` — 13-test integration battery: the directive's
  Recall@10 ≥ 0.95 bar on 10k seeded vectors vs brute-force cosine
  (measured 0.997 @ ef=16, 1.000 @ ef≥64), ef scaling, byte-identical
  determinism per seed, seed divergence, lossless snapshot round-trip
  preserving search results, fail-closed corruption matrix (bad magic,
  bad version, truncation at six offsets, trailing bytes, header
  disagreement), filtered over-fetch semantics, capacity enforcement,
  post-reload insert continuity, and a 10k build/query runtime guard.

## Local Contracts

- Depends only on standard utility crates (`serde`, `thiserror`).
  Architecturally permitted: `vesper-domain` and `vesper-security`; nothing
  else ever.
- Zero I/O by construction: no network, no filesystem, no clock, no process
  spawning, no provider crates, no `vesper-testkit`, no frontend crates.
- `#![forbid(unsafe_code)]`; MSRV 1.88; workspace lints apply.
- The upstream is referenced ONLY as *the swarm oracle*. The upstream-brand
  embargo is mechanically enforced by `cargo xtask naming-guard` against
  `xtask/naming-guard-baseline.json` (fail-closed ratchet; new hits block
  merge).
- Divergences from the upstream model are deliberate and documented in
  source doc comments: `auto_rebalance` and `failover_enabled` default to
  `false`, and topology state carries no wall-clock timestamps.
- Integration is default-off: no production crate or application may depend
  on `vesper-swarm` until the VRO-15 composition PR wires a default-off
  `swarm` feature at the host boundary.

## Work Guidance

- Source of requirements: `docs/swarm-oracle-extraction-prd.md` (§1.3 for the
  data model, §1.4 for lifecycle semantics). PR-1 delivered the topology
  model; PR-2 delivered the manager. Determinism rules: admission order is
  the single ordering authority; hash placement uses the inline FNV-1a in
  `manager.rs` (never std hasher internals); tests use the in-file xorshift
  PRNG (never wall-clock or unseeded randomness).

## Verification

- `cargo test -p vesper-swarm`
- `cargo clippy -p vesper-swarm --all-targets -- -D warnings`
- `cargo tree -p vesper-swarm` (purity: only permitted deps)
- `cargo xtask naming-guard`
- `cargo xtask architecture`

## Child DOX Index

No children.
