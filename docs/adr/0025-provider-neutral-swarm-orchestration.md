# ADR 0025: Provider-neutral swarm orchestration

## Status

Accepted.

## Context

Agent Vesper is a single-agent harness: one ReAct loop, one provider
session, one transcript. Real work regularly decomposes into parallel,
independent pieces (multi-file research, build-then-test, broad review),
and the single-loop model serializes them. Workers exist for bounded
`context: fork` skill bodies, but there is no way to run many agent
turns concurrently under one coherent plan, share partial findings
between them, or bound their aggregate resource use.

The *swarm oracle* — a user-authorized upstream orchestration repository
mirrored at `/home/Alex/Projects/harness-swarm-oracle` (commit `e341ec8`)
— solved exactly this coordination problem. Reconnaissance (recorded in
`docs/swarm-oracle-extraction-prd.md`) established the load-bearing
facts: topology management (mesh/hierarchical/centralized/hybrid),
capability-scored assignment, a four-tier priority message bus with
deterministic eviction, pooled worker lifecycle with heartbeats, and an
HNSW-indexed hybrid memory — all as coordination logic whose LLM binding
lives in a sibling adapter package, structurally separable from the
coordination core.

Two constraints governed any adoption. First, provider neutrality: the
upstream's per-vendor SDK adapters are exactly the coupling Vesper's
`ProviderSession` port exists to prevent, and the naming embargo forbids
importing upstream brand vocabulary into this repository. Second, zero
harness degradation: the measured 1,706-test floor (2026-09-10), the
single-agent loop, and TUI/ACP responsiveness must be untouched — swarm
execution may only ever be an opt-in layer.

## Decision

1. `vesper-swarm` is a **pure-logic coordination crate**: zero I/O by
   construction (no network, no filesystem, no clock, no process
   spawning, no provider names), depending only on `vesper-domain`,
   `vesper-security`, and standard utility crates (`serde`, `thiserror`,
   `tokio`, `futures-util`). Execution, inference, and sandboxing are
   trait ports fulfilled at the composition boundary: `WorkerPort`
   (`crates/vesper-swarm/src/worker.rs`), `EmbeddingPort`
   (`src/ledger/store.rs`), and `SandboxLeasePort` (`src/sandbox.rs`).
2. The upstream's vendor binding is **not ported**. The adapter that
   satisfies `WorkerPort` over the real seams — one swarm turn = one
   `ProviderSession::start` stream with a cancellation bridge and
   per-task tool-registry filtering — lives in
   `crates/vesper-harness/src/swarm_adapter.rs`, behind a **default-off
   `swarm` feature** whose optional deps (`vesper-provider`,
   `vesper-swarm`) link only when explicitly enabled.
3. The crate's modules port the oracle's paradigms with documented,
   fail-closed divergences: `topology`/`manager` (deterministic
   admission-order elections; `auto_rebalance` and `failover_enabled`
   default **off**), `pool` (parallel `join_all` boot, lease-guarded
   acquire/release, heartbeat fail→cancel→replace, release never
   resurrects a `Failed` worker), `bus` (four-tier strict-priority
   inboxes, deterministic bottom-up oldest-first eviction, **Urgent is
   never evicted**, exhaustion fails loudly with `SwarmError::BusFull`),
   `hive::assignment` (the oracle's scoring formula as a pure function
   over Vesper capability vectors), `hive::timeout` (budget → cancel →
   grace → abandon; success-after-cancel is rewritten to `Cancelled`),
   `ledger::hnsw` (oracle defaults M=16/efC=200/cosine with seeded
   xorshift64* level draws; Recall@10 = 0.997 @ ef=16, 1.000 @ ef≥64 on
   10k vectors), `ledger::store` (dual-write hybrid ledger, strict
   Worker/Task/Swarm scope isolation, bounded transfer ≥0.8/≤20 with
   verbatim provenance), `sandbox` (boundary-accounting `LeaseBook`,
   unmet-axis capability gate with Unknown=denial, cancellation-safe
   FIFO waiters, RAII teardown that survives panics), and
   `hive::orchestrator` (the `Hive` engine composing all of it behind a
   caller-owned tick loop).
4. **The naming embargo is mechanically enforced**:
   `cargo xtask naming-guard` (hex-encoded patterns, word-bounded
   matching, SHA-256 line-digest ratchet against
   `xtask/naming-guard-baseline.json`) runs in the `cargo xtask verify`
   chain; the upstream is referenced only as *the swarm oracle*.

## Consequences

1. **Zero harness degradation, proven**: default builds link no swarm
   symbols; the workspace passes 1,869/0 with default features and
   1,893/0 with `--all-features` (2026-09-10); the single-agent ReAct
   loop, TUI, and ACP compile and behave identically without the
   feature.
2. **Opt-in only**: activation is a default-off cargo feature plus an
   explicit host command (`/swarm`); nothing about single-agent
   operation changes unless the operator asks.
3. **Host parity, decided and documented**: the swarm surface is
   TUI-only initially. ACP v1 has no expression for a multi-agent
   progress stream (per-driver turns, ledger growth, event log); a
   host-owned polling model would be an invented protocol. The engine
   and adapter are host-neutral; the exclusion is recorded in
   `apps/agent-vesper-acp/AGENTS.md` and `crates/vesper-harness/AGENTS.md`.
4. **Monotonic floor discipline held across the whole delivery**: PR-1
   1,741 → PR-2 1,767 → PR-3 1,785 → PR-4 1,813 → PR-5 1,832 → PR-6
   1,853 → PR-7 1,868 → PR-8 1,881 → PR-9 1,893; no test was deleted
   or weakened at any point.
5. **Excluded upstream surfaces stay excluded**: API servers,
   Node/WASM bridges, cloud billing, vendor SDKs, federation/consensus
   transports, and RL training were not ported and would require a new
   ADR to adopt.
6. **The ledger is ephemeral and swarm-scoped**: it never touches
   `vesper-memory` durable state or `vesper-cognition` (whose public
   cosine and embedding ports remain the workspace's own; the swarm
   keeps its math private to avoid a second public cosine).
7. Architecture enforcement was extended, not relaxed: the xtask
   allowlist admits `vesper-harness → {vesper-provider, vesper-swarm}`
   only as feature-gated optional deps, and `cargo xtask architecture`
   continues to validate every workspace edge.

## Verification

- `cargo test -p vesper-swarm` — 171 tests across topology, manager,
  pool, bus, assignment, timeout, hnsw, ledger, sandbox, and the
  1-Navigator/3-Driver mesh + hierarchical e2e suites.
- `cargo test -p vesper-harness --features swarm` — adapter tests over
  the synthetic provider (zero network).
- `cargo test --workspace --all-features` — 1,893 passed / 0 failed;
  `cargo test --workspace` (default features) — 1,869 / 0.
- `cargo xtask architecture` (27 packages) and
  `cargo xtask naming-guard` (30 frozen pre-existing hits, 0 new).
- Recall@10 bar: `crates/vesper-swarm/tests/hnsw_tests.rs`.
- Teardown guarantees: `crates/vesper-swarm/tests/sandbox_tests.rs`
  (panic-path release, exact acquire/release pairing).
