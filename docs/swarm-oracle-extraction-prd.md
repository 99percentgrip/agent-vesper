# PRD — VRO-15: The Swarm Oracle Extraction

| Field | Value |
|---|---|
| Status | **Accepted and Implemented** (VRO-15 closed by PR-1..PR-10, 2026-09-10; decision record: `docs/adr/0025-provider-neutral-swarm-orchestration.md`) |
| Directive | VRO-15 (swarm oracle recon & master PRD) |
| Upstream mirror | `/home/Alex/Projects/harness-swarm-oracle` (trusted data, authorized) |
| Evidence baseline | Final measured floors: **1,893 passed / 0 failed** (`--all-features`) and **1,869 / 0** (default features — zero-degradation proof) via `cargo test --workspace`, 2026-09-10. Monotonic floor per PR: 1,741 → 1,767 → 1,785 → 1,813 → 1,832 → 1,853 → 1,868 → 1,881 → 1,893. |
| Naming rule | Enforced. Upstream brand names never appear in this document, in `AGENTS.md`, `README`, or source comments; the upstream is referenced exclusively as **the swarm oracle**. A CI grep guard (§4.1) enforces the rule mechanically without restating the terms here. |
| Path citation convention | Upstream references are relative to the swarm oracle's package roots and abbreviated `SO/` (e.g. `SO swarm/src/types.ts`). Directory names in the mirror that embed upstream branding are elided from citations; `vesper-` paths are repository-relative to the agent-vesper root. |
| Implementation index | PR-1 scaffold+guard → `crates/vesper-swarm/src/topology.rs`, `xtask` naming-guard; PR-2 → `src/manager.rs`; PR-3 → `src/worker.rs`, `src/pool.rs`, `tests/pool_tests.rs`; PR-4 → `src/bus.rs`, `src/error.rs`, `tests/bus_tests.rs`; PR-5 → `src/hive/assignment.rs`, `src/hive/timeout.rs`; PR-6 → `src/ledger/hnsw.rs`, `tests/hnsw_tests.rs`; PR-7 → `src/ledger/store.rs`, `tests/ledger_tests.rs`; PR-8 → `src/sandbox.rs`, `tests/sandbox_tests.rs`; PR-9 → `src/hive/orchestrator.rs`, `tests/hive_tests.rs`, `crates/vesper-harness/src/swarm_adapter.rs` (feature `swarm`, default-off) + its tests. |

---

## 0. Executive Summary

### 0.1 What the swarm oracle is worth taking

The swarm oracle is an orchestration layer whose core insight is worth extracting:
a multi-agent system is a **coordination problem, not a provider problem**. Its
swarm package (SO `swarm/src/`, ~6k lines across coordination, topology, pooling,
and dispatch) implements topology management, capability-scored task assignment,
a priority message bus, and pooled worker lifecycle — and, critically, does so
with **zero LLM-vendor imports** (verified: `grep` over `swarm/src` finds no
vendor SDK references; all vendor binding lives in a separate `providers/src/`
adapter package behind `base-provider.ts` + `provider-manager.ts`). That
separation is exactly the seam agent-vesper already formalizes as
`ProviderSession` (`crates/vesper-provider/src/ports.rs:217`).

VRO-15 extracts three subsystems into a **new, opt-in, pure-logic Rust crate
`vesper-swarm`**, dependent only on `vesper-domain` and `vesper-security`:

1. **The Hive Orchestrator** — topology-aware deployment of parallel worker
   agents (Navigator/Driver roles) over generic provider sessions.
2. **The Shared Memory Ledger** — HNSW-indexed, scope-filtered sharing of
   reasoning trajectories across concurrently running agents.
3. **Sandbox Concurrency** — safe sharing or isolation of
   `IsolationRequirement::Network` boundaries across parallel workers.

### 0.2 Explicit non-goals (excluded from all phases)

Per the directive, the following upstream surfaces are **ignored**:

- **API servers** — every HTTP/MCP gateway, appliance builder, and server mode.
- **Node/WASM bridges** — the browser/WASM packages and JS interop layers.
- **Cloud billing** — cost trackers, metering dashboards, cloud deployment.
- **Vendor-locked SDKs** — `providers/src/` per-vendor adapters are *not*
  ported; Vesper's provider-neutral ports replace the entire package.
- **Distributed federation** — the swarm oracle's raft/gossip/byzantine
  consensus transports, federation hubs, and cross-machine peer discovery.
  Vesper swarms are single-process; consensus is deferred (§1.9).
- **The 100+ prompt-agent library and its skill/command marketplace.**
- **Neural RL training** (PPO/DQN/A2C/etc. in the oracle's neural package).
  Not taken: no training loops, no weight persistence.
- **Browser orchestration** — owned by VRO-14.
- **The oracle's embedded vector-store database file format and its
  persistent-memory bridge** — Vesper defines its own on-disk ledger (§2.5).

### 0.3 Non-negotiable: zero harness degradation

1. The measured workspace floor (1,706 as of 2026-09-10; the directive's 1,644
   historical floor is superseded by the measured one) **must not drop** in any
   PR. No test may be deleted or weakened to admit a feature.
2. `vesper-swarm` is behind a default-off cargo feature (`swarm`). The
   single-agent ReAct loop, TUI, and ACP hosts compile and pass identically
   with the feature disabled.
3. Swarm orchestration runs on its own async task; it never blocks the render
   thread, and the TUI transcript path is untouched.
4. Swarm execution is **opt-in at runtime** (explicit `/swarm`-style
   activation). No default behavior of either host changes.
5. Host parity: per the root `AGENTS.md` contract, any swarm capability wired
   into the TUI is evaluated for ACP in the same change, or receives a
   documented, justified host-specific exclusion in the affected app's
   `AGENTS.md`.

### 0.4 Source-of-truth map (upstream → Vesper seam)

| Upstream (SO, verified) | Concern | Vesper seam |
|---|---|---|
| `swarm/src/types.ts:23-77` | `TopologyType = mesh\|hierarchical\|centralized\|hybrid`; `TopologyConfig/State/Node/Edge/Partition`; roles `queen\|worker\|coordinator\|peer` | `vesper-swarm::topology` data model |
| `swarm/src/topology-manager.ts:17-438` | add/remove/update node, leader election, per-topology rebalance, partition assignment | `vesper-swarm::topology::TopologyManager` |
| `swarm/src/agent-pool.ts:26-474` | pooled worker lifecycle: parallel creation (`Promise.all`), acquire/release, scale, health replacement | `vesper-swarm::pool::WorkerPool` over `WorkerPort` |
| `swarm/src/unified-coordinator.ts:136-933` | per-type and per-domain pools, domain task queues, heartbeat/health intervals, task timeout/cancel, event forwarding | `vesper-swarm::hive::HiveOrchestrator` |
| `swarm/src/unified-coordinator.ts:742-833` | capability-scored assignment + priority mapping (`critical→urgent … background→low`) | `vesper-swarm::hive::assignment` |
| `swarm/src/message-bus.ts:30-425` | 4-tier priority queues, O(1) dequeue, per-agent inbox + type filters, TTL, ack-required, bounded eviction | `vesper-swarm::bus::MessageBus` |
| `swarm/src/queen-coordinator.ts:500-1104` | strategic task analysis, delegation plans, consensus timeouts | Navigator role semantics (subset; §1.7) |
| `swarm/src/workers/worker-dispatch.ts:406-626` | trigger-typed worker execution, 50 ms spawn budget | Driver turn dispatch budget |
| `memory/src/hnsw-index.ts:31-758` | HNSW: M=16, efConstruction=200, cosine, default dim 1536, 1M cap, quantization optional, p=0.5 level draw (cap 16), binary persistence | `vesper-swarm::ledger::hnsw` (pure Rust) |
| `memory/src/hybrid-backend.ts:27-73` | structured store + vector store, dual-write, routing `auto`, semantic threshold 0.7, hybrid max 100 | `vesper-swarm::ledger` hybrid query split |
| `memory/src/agent-memory-scope.ts` | `project\|local\|user` scopes; transfer at confidence ≥ 0.8, ≤ 20 entries, category filters | Ledger scopes + bounded transfer |
| `providers/src/*` (vendor adapters) | LLM binding | **Dropped.** Replaced by `ProviderSession` (`crates/vesper-provider/src/ports.rs:217`), `ToolExecutor`/`ToolService` (`crates/vesper-agent/src/executor.rs:184,208`), `AuxiliaryRequestPort` for embeddings |

### 0.5 Definition of done per feature

- **F1 Hive Orchestrator:** a deterministic (seeded) test drives a swarm of N
  synthetic workers across all four topologies; assignment, priority mapping,
  timeouts, cancellation, and heartbeat expiry behave exactly as specified;
  zero `vesper-provider` dependencies inside `vesper-swarm`.
- **F2 Shared Memory Ledger:** recall@10 ≥ 0.95 against brute-force cosine on
  10k seeded synthetic vectors; filtered search respects scope/agent/task
  filters; persistence round-trips losslessly; concurrent readers/writers
  never observe torn state (deterministic interleaving tests).
- **F3 Sandbox Concurrency:** capability fail-closed gate rejects
  under-provisioned backends before spawn (mirroring
  `crates/vesper-security/src/sandbox.rs` `SandboxCapabilities::satisfies`);
  shared and isolated boundary modes demonstrated with a fake lease backend;
  teardown provably releases every lease on swarm shutdown, cancellation, and
  panic-path drop.

---

## 1. Feature 1 — The Hive Orchestrator

### 1.1 Problem

Vesper has one agent loop (ADR 0010) and a bounded worker concept used for
`context: fork` skill bodies, but no way to run many agent turns concurrently
under a coherent plan: no topology, no assignment policy, no inter-worker
messaging, no lifecycle. The swarm oracle solved exactly this coordination
problem in a provider-neutral way; Vesper needs it as a native, opt-in layer
that composes the existing loop rather than replacing it.

### 1.2 Architecture: a pure-logic coordination core

`vesper-swarm` is a **coordination engine, not an execution engine**. It owns
topology, pooling, assignment, messaging, and telemetry. Actual turns are
executed by workers behind a trait port, satisfied at the composition boundary
(`vesper-harness`/apps) by an adapter over `ProviderSession` + the existing
`ToolExecutor` registry:

```rust
// vesper-swarm — pure logic; no provider crate dependency
pub trait WorkerPort: Send + Sync {
    /// Run one bounded turn for one worker. Adapter maps this to a
    /// ProviderSession::start() stream with its own CancellationSignal.
    fn run_turn<'a>(&'a self, task: &'a WorkerTask, cancel: Arc<dyn CancellationSignal>)
        -> BoxFuture<'a, Result<TurnReceipt, SwarmError>>;
    fn capabilities(&self) -> WorkerCapabilities;
}
```

This mirrors the verified upstream fact that `swarm/src` never imports a
vendor SDK: the oracle's providers package is a sibling, not a dependency.
Vesper keeps the same discipline structurally — the crate cannot even name a
provider.

### 1.3 Topology model

Port `SO swarm/src/types.ts:23-77` and `topology-manager.ts`:

- `TopologyKind::{Mesh, Hierarchical, Centralized, Hybrid}`.
- `TopologyConfig { max_agents, replication_factor, partition_strategy:
  Hash|Range|RoundRobin, failover_enabled, auto_rebalance }`.
- `TopologyState { nodes, edges, leader, partitions }`; node roles
  `Queen | Worker | Coordinator | Peer`; statuses
  `Initializing | Active | Syncing | Failed | …` (adapted to Vesper naming).
- Manager operations: `add_node`, `remove_node`, `update_node`,
  `elect_leader`, `rebalance` (per-kind), partition assignment when
  `nodes > nodes_per_partition`, edge creation per kind (mesh: bounded degree;
  hierarchical: parent/child; centralized: hub/spoke).
- Partition leader failover on removal (upstream `remove_node` reassigns the
  partition leader to the first remaining member — kept, made deterministic by
  insertion order rather than map iteration order).
- `auto_rebalance` defaults **off** (explicit is better; upstream default on).

### 1.4 Worker lifecycle (pool port)

Port `SO agent-pool.ts` semantics:

- Pool bounds `min_workers ..= max_workers`; initial fill creates workers in
  parallel (upstream `initialize` lines 50-60 use `Promise.all`; Rust:
  `join_all` over the port, bounded by `min_workers`).
- `acquire()` → idle worker or create-if-below-max (upstream 89-131);
  `release()` returns it (133-151) with `acquired_at` cleared.
- `scale(delta)` grows/shrinks within bounds (183-206); health monitor
  replaces unhealthy workers (394-403) and emits lifecycle events
  (`agent.acquired/released/created` → `SwarmEvent` enum).
- Heartbeat expiry (upstream coordinator `handleHeartbeat` 933, intervals at
  159-161) marks a worker `Failed`, fails its in-flight task, triggers
  replacement. All timers are **cancellation-safe tokio intervals owned by the
  orchestrator task**, never global state.

### 1.5 Task assignment: capability scoring

Port `SO unified-coordinator.ts:787-812` exactly, as a pure function:

```
score(agent, task) = (100
    + 50  * type_match            // task-kind → preferred worker kinds
    - 20  * agent.workload        // normalized 0..1
    * agent.health                // multiplicative penalty
    + 10  * agent.success_rate
    - 5   * agent.avg_turn_secs / 60)
```

- Best-score assignment (742-770); queue the task when no worker is available.
- Priority mapping `critical→Urgent, high→High, normal→Normal, low/background→Low`
  (821-833).
- Per-domain queues (upstream `AgentDomain` at types 58; domain pools at
  145-149) are generalized to **named worker classes**; the oracle's fixed
  five-domain 15-agent layout is configuration, not code.
- Task timeout (`task_timeout_ms`) and cancellation (434-481) propagate to the
  worker's `CancellationSignal`; a cancelled task never reports success.

### 1.6 Message bus

Port `SO message-bus.ts` with Rust-native primitives:

- Four priorities; strict priority dequeue (30-223 semantics) via one
  `VecDeque` per priority — O(1) pop from the highest non-empty tier.
- Per-worker inbox subscription with message-type filters (365-380);
  `broadcast` fans out to all subscribers (309-323).
- `ttl_ms` expiry and `requires_ack` acknowledgement; bounded queue with
  **deterministic eviction of the lowest-priority oldest entry** when full
  (350-355) — never silent drop of an urgent message.
- Backpressure: bounded channels (`tokio::sync::mpsc`); senders receive
  `SwarmError::BusFull` rather than blocking the caller.

### 1.7 Driver and Navigator roles over generic sessions

Role taxonomy is Vesper's own, mapped onto upstream role semantics:

- **Navigator** — the strategic role (upstream queen/coordinator, SO
  `queen-coordinator.ts:591 analyzeTask`, `:1104 delegateToAgents`): decomposes
  a goal into `WorkerTask`s, assigns via §1.5, reads ledger context, and
  produces the final synthesis. A Navigator is itself a worker turn over a
  `ProviderSession` whose tools are the orchestrator's control surface.
- **Driver** — the execution role (upstream worker, `worker-dispatch.ts`
  triggers): runs one bounded task with a scoped tool registry
  (`ToolExecutor` set filtered per task), writes its trajectory to the ledger,
  reports `TurnReceipt { artifacts, ledger_refs, metrics }`.
- Both roles are `WorkerPort` implementations differing only in configuration
  (system instructions, tool subset, turn budget). Upstream's 13-value
  `AgentType` list collapses into capability sets (SO types 65-79) — kind
  matching in §1.5 becomes capability-vector matching.

Spawn budget: the oracle's 50 ms worker-spawn target (`worker-dispatch.ts:406`)
becomes a soft SLO with telemetry, not a hard gate.

### 1.8 Provider decoupling map (verified upstream evidence)

The oracle binds vendors in exactly one place: `providers/src/` (one adapter
file per vendor, plus `base-provider.ts`, `provider-manager.ts`, `types.ts`).
The swarm core receives inference through that manager. Substitution:

| Oracle binding | Vesper replacement | Evidence |
|---|---|---|
| Provider manager → vendor adapter `complete()` | `ProviderSession::start(ProviderRequest, CancellationSignal)` streamed turn | `crates/vesper-provider/src/ports.rs:217,230-236` |
| Per-vendor embedding calls | `AuxiliaryRequestPort` (opt-in) or explicit `EmbeddingPort` at composition | `ports.rs` `AuxiliaryRequestPort` |
| Worker tool invocation | `ToolExecutor`/`ToolService` registry, per-task filtered | `crates/vesper-agent/src/executor.rs:184,208` |
| Vendor auth/env handling | `ProviderCredentialPort` pool-safe contract | `ports.rs` (`ProviderCredentialPort`) |

`vesper-swarm` itself defines only `WorkerPort` and `EmbeddingPort` (§2.6).
Adapters live in the composition crate and are tested with
`vesper-provider-synthetic`.

### 1.9 What F1 explicitly does not do

- No cross-process or cross-machine swarming (single process only).
- No byzantine/raft consensus: `propose_consensus` (SO 482-507) is **not**
  ported in VRO-15. A Navigator's decision is authoritative; disagreement is
  an error path, not a voting protocol.
- No automatic topology migration heuristics beyond explicit `rebalance()`.
- No new model-facing tools beyond the opt-in swarm control surface (§4.2
  PR-9); no changes to the existing nine parity-critical executors.

---

## 2. Feature 2 — The Shared Memory Ledger

### 2.1 Problem

Parallel workers produce partial reasoning that other workers need: the
Navigator needs Driver findings; a Driver needs its peers' discoveries without
reading their entire transcripts. Upstream solves this with a hybrid
structured+vector memory whose vector side is a hand-written HNSW index. That
index — not the vendor embedding service behind it — is the transferable
architecture.

### 2.2 Architecture: HNSW graph + filtered retrieval

Port `SO memory/src/hnsw-index.ts` to pure Rust (`vesper-swarm::ledger::hnsw`),
preserving the upstream contract:

- **Config defaults** (upstream `mergeConfig` 741-750): `M = 16`,
  `ef_construction = 200`, `max_elements = 1_000_000`, metric `cosine`,
  dimension **runtime-configured** (upstream default 1536 is a vendor
  embedding size and is not carried as a default — dimension comes from the
  active embedding port at composition time).
- **Node model** (192-208): id, raw vector, pre-normalized vector for O(1)
  cosine, per-layer connection sets, node level.
- **Level draw** (752-758): geometric with p = 0.5, capped at 16 — replaced by
  an injectable seeded RNG so index construction is **deterministic and
  testable** (same seed ⇒ same graph).
- **Search** (314-388): top-down layer descent from the entry point with
  heap-based layer search (`ef = max(k, ef_construction)` for construction,
  caller-supplied `ef` for queries); min-heap candidates / max-heap results
  (31-190).
- **Filtered search** (390-398): over-fetch `k * over_fetch_factor` then apply
  predicate filters, so filters never break graph connectivity.
- **Quantization** (optional, upstream `Quantizer`): out of scope for VRO-15
  (§2.7); the port keeps the config field reserved.

### 2.3 Hybrid storage split

Port `SO memory/src/hybrid-backend.ts:27-73` as a two-store ledger:

- **Structured side:** in-crate, append-only entry log keyed by
  `(scope, agent, task, kind, timestamp)` supporting exact-match and range
  queries. No SQL dependency — `vesper-cognition` is the only crate permitted
  `rusqlite` (`crates/AGENTS.md`), and the ledger does not need it.
- **Vector side:** the HNSW graph of trajectory embeddings.
- **Dual-write** on `record()` (upstream default `dual_write: true`): a ledger
  entry is durable only when both sides accept it; a vector-side rejection
  (dimension mismatch, capacity) fails the write closed.
- **Query routing** (upstream `auto`): exact/filterable queries hit the
  structured side; semantic queries hit HNSW; hybrid queries run both and
  merge with the upstream defaults `semantic_threshold = 0.7`,
  `hybrid_max_results = 100` as *configured ceilings*, with exact-side matches
  always winning ties.

### 2.4 Scopes and knowledge transfer

Port `SO memory/src/agent-memory-scope.ts` semantics onto Vesper's existing
scope discipline:

- Ledger scopes: `Swarm` (whole hive, the default sharing surface), `Worker`
  (per-driver private scratch), `Task` (per-task). These are ledger-internal
  and do **not** touch the host's durable project memory
  (`crates/vesper-memory`) — a swarm ledger is ephemeral to its hive run
  unless explicitly exported.
- **Bounded transfer** (upstream `TransferOptions`): copy entries between
  scopes only when `confidence ≥ 0.8`, at most 20 entries per call, with
  category filters — the Navigator's explicit act, never automatic leakage
  between worker scopes.
- Every entry records provenance: worker id, role, task id, turn sequence, so
  the Navigator can cite which worker produced which trajectory step.

### 2.5 Concurrency and persistence

- **Concurrency:** the ledger is `Arc<Ledger>` with interior mutability —
  writes take a short critical section (append entry + HNSW insert), reads are
  lock-free over a snapshot (index reads in HNSW are safe once a node's
  connections are published; publication follows an epoch/arc-swap of the
  entry map). Interleaving is tested deterministically with a single-threaded
  executor driving explicit futures, plus `loom`-style ordering tests for the
  publication protocol.
- **Persistence (own format, not upstream's):** binary snapshot — fixed header
  (magic, version, dimensions, M, ef_construction, entry point, max level,
  counts) followed by node records (id, level, vector, connection lists) and
  the structured entry log. Upstream's header layout (`hnsw-index.ts:553-578,
  645-647`) is the inspiration; the format is versioned and
  backwards-incompatible-change-rejected. Load validates the header and fails
  closed on dimension or count mismatch.
- **Bounds:** entry text bounded by the same `BoundedString` discipline used
  across the harness; ledger size caps with oldest-lowest-confidence eviction
  only in `Swarm` scope; `Worker` scopes cap strictly FIFO.

### 2.6 Embedding port (composition boundary)

Following the `vesper-cognition` precedent (ports fulfilled at composition,
never inside the crate — `crates/AGENTS.md` Stage-16 rule):

```rust
pub trait EmbeddingPort: Send + Sync {
    fn embed<'a>(&'a self, texts: Vec<BoundedText>)
        -> BoxFuture<'a, Result<Vec<Embedding>, SwarmError>>;
}
```

Production adapters (provider auxiliary embeddings) live in the composition
crate. Tests use a deterministic hasher-based fake embedding so recall,
filtering, and persistence tests never make live provider calls (root
`AGENTS.md`: no live provider calls in verification).

### 2.7 What F2 explicitly does not do

- No quantization, no incremental re-indexing, no delete-with-compaction
  (append + tombstone only; a compact operation may come later).
- No cross-run durable memory: the ledger dies with the hive unless exported.
- No vendor embedding defaults; no network calls of any kind inside
  `vesper-swarm`.
- Does not replace or read `vesper-cognition`/`vesper-memory`; it is a
  parallel, swarm-scoped surface.

---

## 3. Feature 3 — Sandbox Concurrency

### 3.1 Problem

Driver turns execute tools. Parallel Drivers sharing one machine must not (a)
trample each other's filesystem writes, (b) exceed the host's sandbox budget,
or (c) silently escape an isolation level the operator requested. Upstream has
no equivalent — its workers trust the host process. Vesper has
`vesper-sandbox` (ADR 0022) and the fail-closed capability model in
`crates/vesper-security/src/sandbox.rs`; the swarm must compose them for
*many concurrent* workers.

### 3.2 Shared vs isolated boundary modes

Two operator-selected modes per hive run (default `Isolated`):

- **Isolated (default):** each Driver holding tools with side effects gets its
  own sandbox lease — a supervisor provisioned at
  `IsolationRequirement::Filesystem` or higher. No shared mutable workspace;
  artifacts cross workers only through the ledger and explicit export paths.
- **Shared:** workers with identical isolation requirements share **one**
  supervisor instance provisioned at the maximum requirement across the group
  (typically `IsolationRequirement::Network` + an explicit network grant,
  mirroring the `vesper-web-fetch` precedent: network I/O only inside a
  sandbox with `Network` isolation and an explicit grant). Sharing is only
  permitted when: requirements are equal, the network grant provenance is
  identical, and per-worker write subdirectories are disjoint
  (`<sandbox_root>/w/<worker_id>/`). Unequal requirements never share — the
  weaker worker is refused or upgraded, operator's choice, fail-closed default
  refuse.

### 3.3 Fail-closed capability gate

Before any worker spawn, the orchestrator queries the backend's
`SandboxCapabilities` and requires `satisfies(requirement)` — the same
fail-closed ordering already tested in
`crates/vesper-security/src/sandbox.rs` (`ProcessTree < Filesystem < Network/
Full`, `SecurityStrength` ordered). An `Unavailable`/`Unknown` capability
status is a denial, never a warning. A hive configured above the backend's
verified strength fails at `init` with a diagnostic naming the unmet axis.

### 3.4 Lease lifecycle and teardown

```rust
pub trait SandboxLeasePort: Send + Sync {
    fn acquire<'a>(&'a self, spec: LeaseSpec) -> BoxFuture<'a, Result<Lease, SwarmError>>;
}
```

- `LeaseSpec { requirement, network_grant, mode: Isolated|Shared(group),
  worker_id }`; a `Lease` is RAII — `Drop` releases.
- Every acquisition path (worker start, scale-up, health replacement)
  funnels through a `LeaseBook` enforcing `max_concurrent_leases`; exhaustion
  queues the worker, it never over-provisions.
- Teardown guarantees: swarm `shutdown()`, task cancellation, heartbeat
  failure, and orchestrator panic (via `JoinHandle` + guard drop) all release
  leases; a release asserts no descendant processes remain (process-tree
  capability) and fails loud if it cannot verify.
- Teardown verification is a first-class test target with a fake lease
  backend counting acquire/release pairs, including panic paths.

### 3.5 Context-collision avoidance

Beyond OS isolation: each Driver owns a distinct session transcript and a
ledger `Worker` scope; the Navigator merges. Filesystem collisions are
prevented by per-worker subdirectory confinement; concurrent writes to shared
export paths are serialized by the orchestrator; the existing
stream-interruption rules (never replay a possibly side-effecting tool call)
apply per worker unchanged.

### 3.6 What F3 explicitly does not do

- No new sandbox backends; no changes to `vesper-sandbox` or the supervisor
  binary. F3 composes only.
- No cross-platform sandbox invention: capability-gated exactly as today
  (honest `CapabilityStatus`, fail closed).
- No network egress outside a provisioned grant; the swarm never performs
  network I/O itself.

---

## 4. Verification & Phasing

### 4.1 Merge gates (every PR, no exceptions)

1. **Monotonic test floor:** `cargo test --workspace` green with passed count
   ≥ the predecessor PR's count; baseline at PR-1 is the measured 1,706
   (2026-09-10). No test deleted or weakened; floors are recorded per PR in
   the PR body.
2. **Zero-degradation:** with `--no-default-features` (swarm off), the
   workspace builds and passes identically; the single-agent loop crate's
   diff must be empty or trivially additive.
3. **Lint/supply chain:** `cargo clippy`, `rustfmt`, `cargo deny`
   (advisory/source/wildcard fail-closed) and MSRV checks all green.
4. **Purity:** `cargo tree -p vesper-swarm` shows only `vesper-domain`,
   `vesper-security`, and standard utility crates (futures/tokio/serde/thiserror);
   never `vesper-testkit`, provider crates, or frontend crates.
5. **No live provider calls** in any test; all swarm tests use
   `vesper-provider-synthetic` or in-crate fakes.
6. **Naming-rule guard:** a grep over `docs/`, `AGENTS.md`, `README`, and
   `crates/**` rejects the forbidden upstream brand strings (case-insensitive;
   pattern list maintained in the guard script, not restated in documents).
7. **DOX pass:** each PR updates the nearest `AGENTS.md` for new durable
   structure (`crates/vesper-swarm/AGENTS.md` at PR-1).

### 4.2 PR plan (strictly isolated, one PR each)

| PR | Scope | Key tests |
|---|---|---|
| **PR-1** | Crate scaffold `vesper-swarm` (feature `swarm`, default off), topology data model (types + config), `AGENTS.md`, naming guard script | Topology type serialization round-trips; guard script self-test |
| **PR-2** | `TopologyManager`: add/remove/update, leader election, partition assignment, per-kind `rebalance`, edge construction | Deterministic topology property tests (seeded); leader failover on removal |
| **PR-3** | `WorkerPool` lifecycle over a fake `WorkerPort`: parallel fill, acquire/release, scale, health replacement, heartbeat expiry | Lifecycle state machine tests; parallel fill with bounded concurrency |
| **PR-4** | `MessageBus`: priority tiers, filters, TTL, ack, bounded eviction | Priority/eviction ordering tests; no silent urgent drop; backpressure error |
| **PR-5** | Assignment scoring (pure fn), priority mapping, per-class queues, timeout/cancel propagation | Scoring table tests (ported upstream values); cancel never yields success |
| **PR-6** | HNSW core: insert, layer search, filtered search, seeded determinism, persistence round-trip | Recall@10 ≥ 0.95 vs brute force on 10k seeded vectors; snapshot load validation fails closed |
| **PR-7** | Ledger composition: hybrid split, dual-write, scopes, bounded transfer, concurrency publication protocol | Interleaved reader/writer tests; transfer bounds (0.8/20/categories); scope isolation |
| **PR-8** | Sandbox concurrency: `LeaseBook`, `SandboxLeasePort` fake, shared/isolated modes, fail-closed gate, teardown incl. panic path | Capability denial matrix (ported from `sandbox.rs` ordering); lease leak detector |
| **PR-9** | `HiveOrchestrator` end-to-end + `WorkerPort` adapter over `ProviderSession`/`ToolExecutor` in composition crate, Navigator/Driver roles, opt-in host surface, host-parity evaluation | Full synthetic swarm e2e (Navigator + 3 Drivers, mesh + hierarchical); parity decision recorded |
| **PR-10** | Docs: migration-status entry, evidence index, this PRD's status → accepted; ADR for the swarm crate decision | Evidence cross-links |

PRs 2-8 are orderable in parallel after PR-1 where disjoint (2/3/4/6 are
independent; 5 depends on 3; 7 on 6; 8 independent; 9 last).

### 4.3 Evidence obligations

- Recon evidence for this PRD: the mirror at
  `/home/Alex/Projects/harness-swarm-oracle` (shallow clone, commit recorded
  in PR-1) with the cited file/line references in §0.4.
- Each feature's definition-of-done (§0.5) becomes a named integration test in
  PR-9's e2e suite.
- Floor ledger: PR bodies record `passed` counts; any count regression blocks
  merge regardless of cause.
