# VRO-15 independent implementation audit

## Verdict and scope

**REJECT the “Accepted and Implemented / COMPLETE” claim. Retain default-off status.**
There are useful, tested coordination foundations, but the delivered Hive is not the
specified parallel agent orchestrator. Several security/lifecycle contracts also fail.
This is an audit and proposed repair package; **no production fixes are implemented**.

- Reviewed implementation commit: `fb14ea211849f23cfa778c615f863d0b2f52f42e`.
- Reviewed checkout: `f662519` (v0.21.4 release bump), initially clean.
- Requirements: `docs/swarm-oracle-extraction-prd.md`, ADR 0025, root DOX,
  crate/host contracts and the implementation's own API promises.
- Swarm oracle: `/home/Alex/Projects/harness-swarm-oracle`, verified HEAD
  `e341ec8c4aba8ea616499180dee53035af7e295c`, clean before/after inspection.
  `SO` citations below are relative package paths with mirror branding elided.
- Method: changed-file review; source-to-requirement tracing across all new modules,
  adapter, tests and xtask changes; oracle comparison; local regression suites;
  16 bounded adversarial probes against unmodified compiled production libraries.
- Platform: local Linux x86-64, Rust 1.95.0; swarm tests also run on MSRV 1.88.0.
- Created evidence: this report and `vro15-audit-probes.rs`; updated nearest DOX and
  evidence index. Only documentation/evidence paths changed. Build products/logs
  are under ignored `target/`; no live provider calls or user-state writes.
- Severity: **P1** blocks activating/shipping the claimed swarm capability;
  **P2** is a required correctness/contract/verification repair. Default-off and
  missing host wiring limit current exposure; this audit does not claim an active
  end-user sandbox exploit.

## Acceptance matrix

| Deliverable | Actual disposition |
|---|---|
| Pure provider-neutral dependency boundary | Present; architecture gate passes. “No clock” wording is false for bus/pool/timeout. |
| Four topology data models | Present and tested; leadership policy defects below. |
| Worker lifecycle and timeout | Implemented separately, defective on adversarial paths, not used by Hive. |
| Priority bus | Basic priority/eviction/filtering works; closure, fanout and ACK defects below. |
| Capability scoring faithful to oracle | No: health multiplication was ported incorrectly. |
| Parallel Navigator + 3 Drivers | No: default test configuration has one Driver; all three tasks run sequentially on it. |
| Navigator decomposition and synthesis | No real subtask contract; task count only; synthesis never receives the ledger. |
| Provider + ToolExecutor composition | Text-stream adapter only; tool events are ignored. |
| Shared/isolated sandbox execution | Standalone fake-tested lease policy only, not composed with Hive or real tool execution. |
| HNSW Recall@10 on prescribed seeded data | Existing test passes; does not establish snapshot integrity or general data quality. |
| Full ledger persistence, filters, eviction, snapshot readers | Substantial omissions; only HNSW has snapshot methods. |
| Native opt-in host surface | Absent in both hosts. TUI DOX itself labels the command planned. |
| Workspace test floors | Reproduced exactly, but insufficient as completion evidence. |
| All platform/release/supply-chain gates | Not independently certified by this audit. |

## Findings and solutions

### F01 — P1: orchestration is a sequential scripted pipeline, not a swarm

**Evidence:** `crates/vesper-swarm/src/hive/orchestrator.rs:170-181,394-569`.
`select_best` is called at 459 and discarded at 460. Every assignment targets
`driver-0` at 470; every turn is awaited serially at 491. `WorkerPool`,
`LeaseBook`, heartbeat/health replacement and `execute_bounded` are absent from
Hive. `max_workers` is not applied to execution. Named role classes advertised
by the API reduce to hard-coded `navigator` and `driver`. Topology edges do not
constrain dispatch or communication. The received bus message is discarded;
the locally constructed task runs regardless of the message received.

`count_tasks` (575-586) recognizes the fake navigator's `tasks: N`, not a
structured decomposition. Each generated prompt repeats the original goal
(435-446), losing the actual subtasks. Synthesis (525-540) asks to read “the
ledger” but provides neither ledger entries nor a ledger/control port/tool.
The real adapter starts a fresh one-message request, so it cannot infer those
findings from conversation history either.

**Impact:** duplicated work, no parallelism, no meaningful load routing, and
synthesis without Driver evidence. Several claimed integration layers are decorative.

**Solution:** build a real goal/task state machine and validated, bounded task-DAG
contract; retain subtask bodies, prerequisites and capabilities. Dispatch through
per-worker pool leases and bounded concurrent futures; honor eligible scored
selection and topology policy. Pass bounded, provenance-bearing ledger findings
into Navigator synthesis. Make bus delivery task-ID-checked, not ceremonial.
**Acceptance:** three distinct workers demonstrably overlap using barriers, exact
subtask prompts reach the right workers, synthesis contains canary Driver findings,
and all four topology modes exercise real routing/failover behavior.

### F02 — P1: the provider adapter cannot execute agent tasks

**Evidence:** `crates/vesper-harness/src/swarm_adapter.rs:94-123,127-170`.
The adapter advertises filtered tools, but processes only text deltas and
`Completed`; every other successful event is ignored. There is no ToolExecutor,
permission decision, sandbox route, tool-result message or continuation loop.
`Completed` unconditionally sets `success = true` without evaluating the terminal
reason; an exhausted stream without completion produces `Ok(success=false)`.
A later error returns `Err` and discards accumulated text. Cancellation is checked
only before dispatch. `ContentText::new(task.prompt).expect(...)` can panic because
`WorkerTask.prompt` is an unrestricted `String`.

This path also bypasses the shared agent-loop seams for compaction, skills,
stream recovery, history/transactions and model capability policy. A registry
filter is not an execution authorization boundary.

**Solution:** compose the existing provider-neutral agent runtime and ToolExecutor
rather than inventing a separate text-only agent loop. Give each worker isolated
transactional history and scoped tools/permissions/leases; preserve partial output,
validate completion reasons, validate all input bounds and recheck cancellation.
**Acceptance:** synthetic tool-call → permission → sandbox → result → continuation
trace; denied and ambiguous tool calls never run/replay; partial interrupted output
survives; pressure-driven compaction and applicable shared cognition rules are tested.

### F03 — P1: Hive turns are unbounded and failed goals disappear

**Evidence:** `hive/orchestrator.rs:319-322,394-423,433-446,490-491,537-540,575-586`.
Goal queue and model-selected task count are unbounded. Navigator/Driver calls
ignore their deadlines except as data. Cancel flags are created locally and never
triggered or exposed. A hanging port hangs the tick. The goal is popped before
any await; cancellation/error loses pending goal state while earlier actions may
already have completed. `success=false` Driver receipts still lead to synthesis;
Navigator success flags are not checked. Task IDs/goals are not deduplicated.

**Solution:** bounded admission, hard task/goal/output budgets, external cancellation
and shutdown, persisted-in-run task status and explicit failed/partial outcomes.
Do not auto-requeue ambiguous side effects. Validate role bounds/unique IDs and
make topology admission transactional (currently a bus subscription can remain
when node admission fails).
**Acceptance:** hangs, malformed/huge task counts, duplicate IDs, cancellation at
every await and each intermediate failure preserve truthful state and resource bounds.

### F04 — P1: host activation is missing; ACP exclusion is not established

**Evidence:** search of both app `src/` trees and Cargo manifests finds no swarm
command/feature composition. Both normal host dependency trees contain no
`vesper-swarm`. `apps/agent-vesper-tui/AGENTS.md` explicitly calls the surface
“planned” and says it lands with a later host-wiring PR. ADR 0025 and migration
status nevertheless describe an explicit TUI `/swarm` delivery.

ACP DOX says no progress/notification expression exists, but existing code already
maps streamed output and tool updates (`crates/vesper-acp/src/mapping.rs:172-280`)
and VRO events to `AgentMessageChunk` (`src/vro_events.rs:229-249`). This does not
prove a rich parallel-agent UI exists; it does undermine blanket exclusion of the
host-neutral capability. A current primary-doc read was attempted at
`https://agentclientprotocol.com/protocol/session-updates` but the reader failed
with authentication unavailable. Neither casing of the configured frozen Python
oracle path exists on this checkout. Thus the required exclusion evidence is
incomplete, not an established impossibility.

**Solution:** wire runtime enablement in native Settings with persisted choices,
then explicit command activation, behind safe feature defaults. Evaluate a shared
ACP command with ordinary progress/text/tool notifications; reserve exclusions for
specific rich UI affordances, not orchestration itself. Record current primary-doc
and frozen-oracle evidence before accepting any protocol exclusion.
**Acceptance:** cross-host command registration/advertisement and opt-in/off behavior,
provider reuse, cancellation and nonblocking UI integration tests.

### F05 — P1: assignment health arithmetic contradicts the oracle

**Evidence:** `hive/assignment.rs:90-100` versus
SO `swarm/src/unified-coordinator.ts:787-817`.
The oracle executes `score -= workload * 20; score *= health;` after type matching.
Correct algebra is:

```text
(100 + 50*type_match - 20*workload) * health
    + 10*success_rate - 5*(avg_turn_secs/60)
```

Rust instead multiplies only the workload penalty by health. Matching worker,
workload=0.5, success_rate=1, duration=0: healthy scores 150 in both; dead scores
**160 in Rust versus 10 in the oracle**. The test
`workload_penalty_scales_with_health` explicitly blesses this wrong result.
`select_best` additionally has no hard capability/liveness eligibility filter:
unsupported workers can still win rather than queueing/refusing.

**Solution:** correct the formula and PRD's ambiguous algebra; apply hard eligibility
before scoring, with bounded finite load inputs. Capture oracle-derived scoring
vectors, including dead/unhealthy and no-eligible-worker cases.

### F06 — P1: timeout cancellation is disconnected from the worker

**Evidence:** `hive/timeout.rs:42-92`. The helper accepts an already-created future,
creates a different private flag, discards its signal at 59, and cancels only that
unused flag. The worker cannot receive it. The existing timeout test merely sleeps
and returns late success; the pre-cancel test cancels a different flag before calling
this helper. Neither proves timeout propagation.

**Reproduced:** `timeout_does_not_signal_worker`: a cooperative worker remains
uncancelled and is abandoned after grace.

**Solution:** accept the actual flag or a turn factory receiving the boundary-owned
signal; connect external cancellation too. Use this one boundary from pool and Hive.
Test signal observation, grace teardown, external cancel and no late success.

### F07 — P1: pool invariants fail on ordinary lifecycle boundaries

**Evidence:** `pool.rs:357-378,383-430,459-505,560-591`.
- `initialize` always appends `min_workers`, including on repeated calls, with no
  capacity recheck. Probe: min=max=2, initialize twice → **4 workers**.
- `replace_failed` checks `total >= max` before removing the failed worker.
  Probe: min=max=1, fail worker → **0 replacements and no usable worker**.
- Acquire's new-worker branch does not check `supports(required)`.
  Probe: missing capability → successful lease to incapable worker.
- `run_task` returns an `Ok(success=true)` port result without checking whether
  health monitoring already cancelled its flag. `close` only flips a boolean;
  it does not cancel live work. Dropping a running future releases the lease but
  does not explicitly signal provider cancellation.
- “Parallel boot” is `join_all` over synchronous `capabilities()` calls, not actual
  asynchronous worker/session creation. High-water is assigned from requested
  count, not measured simultaneous activity. The one port has no worker identity
  argument or per-worker creation lifecycle.

**Solution:** atomic idempotent/reserved boot accounting; replace-in-place while
respecting cleanup ownership; hard capability checks on every admission; real
worker instance/factory seam; close/drop cancel guards and late-result normalization.
**Acceptance:** repeated/concurrent boot, full-pool replacement, unsupported request,
cancel-ignoring success and actual overlapping initialization tests.

### F08 — P1: topology failover bypasses policy and can promote failed nodes

**Evidence:** `manager.rs:228-245,299-324,352-365`.
Removal selects `join_order[0]` without eligibility. With auto-rebalance on,
`reconcile_leadership` re-elects even when `failover_enabled=false`.
Both reproduced: disabled failover still elects the remaining node; enabled failover
promotes an older `Failed` node ahead of a healthy one. Partition failover similarly
uses the first partition member, not necessarily oldest eligible admission.
Centralized rebalance wires edges before reconciling leadership, permitting edges
around the old hub after a status-driven election; idempotence needs mixed-transition
tests, not just settled-state repeats.

**Solution:** one explicit leadership-policy function for topology and partitions;
distinguish initial/manual election from automatic failover. Reconcile eligible
leadership before wiring. Test all flag combinations and failed/syncing/admission
orders, including centralized and partition transitions.

### F09 — P1: shared sandbox “disjoint paths” checks are unsound

**Evidence:** `sandbox.rs:140-149,426-446`. `can_share` checks only string inequality,
not path disjointness. `admit` compares incoming specs only with the origin, not all
live members. Probes admit workers A/B/B into one boundary and accept
`/w/a/` alongside `/w/a/nested/`. Worker IDs/path strings are unrestricted, so path
traversal/canonical aliases are also not rejected by this policy layer.

**Solution:** validated workspace-relative worker paths and IDs, component-aware
non-overlap against every live member, duplicate-worker refusal; backend-enforced
confinement handles filesystem aliases/symlinks. Require scoped permission/grant
provenance at composition rather than treating arbitrary strings as authorization.
Test three-member collisions, parent/child paths, dot segments, separators and aliases.

### F10 — P1: failed sandbox teardown frees capacity anyway

**Evidence:** `sandbox.rs:366-417`. The boundary is removed before backend release;
on failure only a bounded diagnostic is retained, `total_released` increments and
queued/new acquisitions are allowed. Probe: backend reports “still alive”, book
reports zero active/one released and provisions another boundary.
Backend acquire/release also run synchronously while the book mutex is held; a
panicking/reentrant backend can poison/deadlock bookkeeping. Existing panic test
covers a holder panic, not a backend panic or descendant cleanup failure.

**Solution:** quarantine failed-release boundaries, retain capacity until verified
cleanup, return an explicit shutdown result and stop/deny further provisioning on
unverified teardown. Design bounded async backend operations outside the state lock,
with reservation/commit guards. Test failing/panicking cleanup and actual supervisor
integration before claiming process-tree teardown guarantees.

### F11 — P1: bus close does not release blocked receivers

**Evidence:** `bus.rs:193-195,300-307,339-397,421-425`.
`notify_all_waiters` is a no-op; `close` neither wakes inbox waiters nor causes
`try_recv`/`recv` to return `BusClosed`. Unsubscribe removes the inbox without waking
its parked readers. Probe: receive from an empty, already closed bus times out.
Multiple readers on one inbox also need the standard pre-registered Notify pattern;
current notify-one permits can coalesce before readers park.

**Solution:** define drain-vs-immediate-close semantics, check terminal state under
the lock, register notifications before rechecking queues and wake all waiters on
close/unsubscribe. Test parked and future readers, unsubscribe and multiple readers.

### F12 — P2: broadcast is partial/nondeterministic; ACK state is unbounded/leaky

**Evidence:** `bus.rs:318-332,339-365,300-307,401-417`.
Broadcast claims all-or-nothing but iterates unordered HashMap recipients and calls
admit one at a time. Probe: capacity=1/two Urgent recipients → Err plus one queued
message. Retrying risks duplicates; which recipient wins is unstable. ACK tracking
is inserted before TTL expiry checking, so expired messages create debts never
returned to a consumer; unsubscribe does not decrement the aggregate. Both leaks
reproduced. Draining requires-ack traffic without acknowledging grows `pending_acks`
beyond queue capacity. Payloads/subscriber count are unrestricted despite bounded
payload/nonblocking claims. Very large TTL overflow becomes `None` (no expiry).

**Solution:** sorted recipient snapshot and atomic capacity/eviction preflight,
then commit all deliveries or none (or change API to explicit per-recipient results,
never an ambiguous aggregate error). Track ACKs only for live delivery, bound their
count/lifetime and clean them on unsubscribe/close. Bound bytes, subscribers and TTL.

### F13 — P1: HNSW snapshot validation accepts panic-inducing graphs

**Evidence:** `ledger/hnsw.rs:579-715`, traversal at 374-389 and 416-432.
Loader checks ordinal ranges but not layer consistency between entry/header/nodes
or neighbor targets. Probe changes header max-level to 15: load succeeds, subsequent
search panics indexing nonexistent layers. Duplicate IDs are silently collapsed in
the ordinal map; finite/normalized vectors are not validated. Node-record allocation
trusts declared count before checking adequate bytes; vector size arithmetic is
unchecked. Expected max-elements/seed policy is not enforced as dimensions/M/ef are.

**Solution:** validate every count with checked arithmetic and byte budget before
allocation; validate unique IDs, entry maximal level, neighbor layer membership,
self/duplicate edges, finite vector policy and caller capacity ceilings. Return
structured corruption errors, never search-time panic. Add fuzz/property corruption
coverage and cross-target loader tests.

### F14 — P2: HNSW round-trip/continued determinism claims are false

**Evidence:** `hnsw.rs:163-193,545-575,639-643,688-715`.
Level generation permits 16, but loader rejects `level >= 16` and `max_level >= 16`.
A valid seed equal to the XOR constant creates the PRNG zero fixed point; first
node gets level 16 and the index cannot load its own snapshot (reproduced).
Only initial seed is persisted; loader resets RNG instead of restoring/replaying
its state. Identical insertions after reload produce different snapshots from an
uninterrupted index (reproduced). `over_fetch_factor` is supplied by the caller on
load, not persisted/validated. Only normalized vectors survive; raw vectors required
by the PRD's lossless snapshot model are not stored.

**Solution:** repair level bounds and zero-state seed handling, persist exact RNG
state and all semantic configuration in a versioned format, decide/record whether
raw vectors are required. Test byte-identical continued insertion across reload,
all legal levels/seeds and nondefault configuration, not approximate overlap only.

### F15 — P2: HNSW pruning/math has correctness gaps beyond the recall fixture

**Evidence:** `hnsw.rs:223-233,287-313,461-489`.
Pruning a neighbor's adjacency (`from`) scores candidates relative to the newly
inserted node (`to`), not the node whose adjacency is being pruned. This biases
retention toward the latest point rather than nearest neighbors of the owner.
Nonfinite values are accepted and can produce nonfinite similarities (reproduced).
Finite large values whose norm overflows are returned unnormalized, violating
cosine bounds. `search_layer` allocates a visited vector sized to the entire index
on every invocation, so default million-entry scale needs measurement beyond 10k.

**Solution:** owner-relative adjacency pruning, robust finite/norm validation and
explicit zero-vector policy; test separated clusters/adversarial order and invalid
vectors. Benchmark high-dimensional/default-capacity behavior and use bounded or
reused visitation state where justified. The passing prescribed recall test remains
valid evidence for that dataset, not proof that these paths are correct.

### F16 — P2: the ledger omits accepted persistence/concurrency/filter contracts

**Evidence:** `ledger/store.rs:131-165,247-250,312-353,375-529,545-590` versus
PRD §§0.5, 2.3-2.6.
- No full-ledger export/import: HNSW snapshots alone omit text, scopes, keys,
  provenance and sequence state. Ephemeral-by-default does not remove the PRD's
  explicit opt-in snapshot requirement.
- All queries/inserts run under one blocking std mutex, including graph traversal
  and entry cloning; no lock-free snapshot publication or deterministic publication
  protocol tests. Safety from the mutex is real; promised reader semantics are not.
- Missing worker/task provenance and timestamp/range query filters, transfer
  category filters and per-scope eviction/tombstone policy. Global HNSW capacity
  refusal is not the specified Swarm confidence/age or Worker FIFO policy.
- `semantic` does not apply its documented 0.7 threshold; only hybrid does.
  Embedding calls accept any nonempty vector list, silently ignoring extra vectors
  despite the explicit arity error contract. `scope_search` ignores configured
  over-fetch and fetches `k*400`; narrow scopes can be starved by unrelated entries.
- Transfer validates/copies sequentially. A later unknown/foreign entry or embedding
  failure returns Err after earlier copies committed, without exposing partial IDs.
  Retrying can duplicate copies. `record` increments next-ID before vector admission.

**Solution:** implement versioned whole-ledger snapshots and validated loading;
complete bounded query/transfer/eviction contracts. Make transfer prevalidated and
transactional or expose explicit partial outcomes; enforce exact embedding arity.
For concurrency, implement the accepted snapshot design, or obtain an explicit ADR/
PRD amendment for a simpler measured lock-based design. Do not silently claim both.
Test complete state round-trips, failed batch atomicity, selective filters, capacity
pressure and actual scheduled reader/writer interleavings.

### F17 — P1: tests certify weaker behavior than their names and closeout claims

**Evidence:**
- `tests/hive_tests.rs:151-155,214-245`: balanced config has driver.min_workers=1;
  “one_navigator_three_drivers” asserts only >=2 nodes and 3 calls to the same port.
- `tests/pool_tests.rs:262-268`: “idempotent” boot test accepts doubling from 2 to 4.
- `tests/hnsw_tests.rs:268-289`: continued determinism accepts 8/10 search overlap;
  it never compares continued snapshots or RNG state.
- `hive/timeout.rs:156-191,214-277`: timeout tests do not observe the actual boundary
  flag in the running worker.
- `vesper-harness/tests/swarm_adapter_tests.rs:90-135`: filter test does not capture
  outgoing tools; provider-neutral request test inspects only declared capabilities.
- ADR 0025 says 171 swarm tests; actual current swarm suite has **163 passing** tests.
  Workspace totals 1,893/1,869 are nevertheless reproducible.

**Solution:** require named tests to assert the actual contract, inject capture ports
and barriers, include failure and permission paths through the same composed Hive
used by hosts. Keep existing floors but add requirement-to-assertion traceability.
Record source-captured oracle vectors, not hand-authored expectations mistaken for
oracle evidence. Do not treat the squash commit as independently proving ten PR
bodies/CI gates or a per-PR monotonic history.

### F18 — P2: governance guards and DOX overstate what they enforce

**Evidence:** `xtask/src/main.rs:936-970,1501-1510,1882-1895`.
Architecture allows the new dependency names, but deserialized `Dependency` has
no optional/features fields; it does not verify the advertised swarm feature gate.
Current Cargo gating is present; the ratchet does not prevent an unconditional edge.
Purity wording says no clock, but bus uses std Instant and pool/timeout use Tokio
clocks. `crates/AGENTS.md` and swarm DOX still describe later pooling/ledger PRs
and pre-composition restrictions despite detailed later implementation ownership.
The naming guard has no new self-test in this change; its baseline key includes
line number even though a digest already captures file/content, so unrelated line
insertions can produce false violations. The `.json` baseline is line text, not JSON.

**Solution:** enforce optionality/feature activation in Cargo metadata and verify
absence of the swarm dependency in both default hosts. Choose and document the
clock boundary truthfully (injected time vs explicit runtime-dependent policy).
Remove stale DOX statements when the repair's architecture is accepted; add guard
fixture tests and a stable baseline representation. Correct completion status and
acceptance evidence rather than merely renaming missing work.

## What is genuinely working

The Rust/provider separation is useful and current defaults keep the new swarm
adapter out of normal hosts. Basic topology serialization/deterministic settled
wiring, scoring priority mapping, four-tier queue operations, many RAII happy paths,
mutex-protected dual writes, scope matching, confidence/copy caps and the specified
seeded HNSW recall scenario have real passing tests. This is not a recommendation
to discard the crate; it is a recommendation to repair and genuinely compose it.

## Verification performed

| Command | Result |
|---|---|
| `cargo test -p vesper-swarm` | 163 passed, 0 failed, 1 ignored doctest |
| `cargo test -p vesper-harness --features swarm --test swarm_adapter_tests` | 4 passed |
| `cargo test --workspace --all-features` | 1,893 passed, 0 failed, 21 ignored |
| `cargo test --workspace` | 1,869 passed, 0 failed, 15 ignored |
| `cargo +1.88.0 test -p vesper-swarm` | 163 passed, 0 failed, 1 ignored |
| `cargo fmt --all --check` | passed |
| `cargo clippy -p vesper-swarm --all-targets -- -D warnings` | passed |
| `cargo xtask architecture` | passed, 27 packages |
| `cargo xtask naming-guard` | passed, 30 frozen hits |
| default normal Cargo trees for both hosts | no swarm dependency |
| standalone adversarial probes | 16/16 reproduce observed defects |

Workspace logs: `target/vro15-workspace-{all,default}-audit.log`; other local logs
use `target/vro15-*-audit.log`. These are disposable build outputs, not tracked CI
attestations. An initial standalone probe compile selected an old MSRV Tokio rlib;
explicit compatible dependencies resolved it. One draft probe used an incorrect
header offset; corrected to bytes 44..48 and rerun. Neither tooling issue is reported
as a product defect.

**Not independently verified:** full canonical `cargo xtask verify`, workspace-wide
Clippy/MSRV suite, supply-chain advisory/license gates, five-target CI or exact-commit
release acceptance. `cargo deny` is not installed. Web primary-doc reader is unavailable;
frozen Python path is absent. No production provider, real swarm sandbox workload,
or rich ACP UI was executed. Existing synthetic tests do not substitute for these.

## Reproducing the adversarial evidence

`vro15-audit-probes.rs` is deliberately outside production/test targets. It asserts
buggy behavior, so **passing means a defect is reproduced**, not acceptance. Convert
these to desired-behavior regression tests only in an approved repair.

After building `vesper-swarm`, use dependency filenames from the current compiler's
Cargo build invocation (do not choose arbitrary rlibs from a mixed-toolchain cache).
The exact successful local command was:

```sh
rustc --edition=2024 --test -L dependency=target/debug/deps \
  --extern vesper_swarm=target/debug/deps/libvesper_swarm-028deaa93ef56dcb.rlib \
  --extern tokio=target/debug/deps/libtokio-4e58f0b1496af932.rlib \
  --extern futures_util=target/debug/deps/libfutures_util-773b789a4e5b2b45.rlib \
  --extern vesper_security=target/debug/deps/libvesper_security-64def7b3ec76e14b.rlib \
  docs/foundation/vro15-audit-probes.rs -o target/vro15-audit-probes
target/vro15-audit-probes --test-threads=1
```

## Proposed repair sequence — awaiting Alex's approval

1. **Reopen acceptance and lock real requirements:** correct status/DOX, add the
   failing-contract tests and source-derived oracle vectors. Decide explicitly
   whether snapshot-reader architecture is retained or amended. Do not activate yet.
2. **Repair foundation safety/correctness:** F05-F16 in bounded, independently tested
   changes: cancellation/pool/topology, sandbox, bus, then HNSW/ledger.
3. **Implement genuine orchestration and agent composition:** F01-F03, isolated worker
   sessions, real tool/permission/sandbox pipeline, task state machine, actual
   parallelism, bounded evidence-fed synthesis and shutdown.
4. **Wire native Settings and both hosts:** F04, capability/command parity; justify
   any narrow UI exclusion using primary evidence. Add same-engine synthetic host
   acceptance and deterministic default-off checks.
5. **Re-run closure gates:** F17-F18, full canonical/default/all-feature/MSRV,
   supply-chain and target matrices, supervisor acceptance, source invariance;
   only then change completion status and consider release under exact-commit policy.

No phase above is authorized by this report. Production/host/ADR/PRD files remain
unchanged pending approval. The nearest evidence DOX and index link this audit as
counter-evidence to historical completion claims; they do not silently rewrite the
accepted scope. No claim is made that finite testing proves absence of all other bugs.
