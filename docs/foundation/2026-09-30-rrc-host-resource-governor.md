# RRC Host Resource Governor — execution report

**Date:** 2026-09-30  
**Status:** Implemented and locally verified on Linux; no release action was performed.  
**Scope boundary:** This report covers the controller-owned local-verification resource governor. It does not claim cross-platform governor readiness, a production release, publication, tag, push, installation, live-provider call, or any VRO-19 work.

## Objective

Prevent Release Recovery Controller (RRC) local verification from exhausting an interactive host's RAM, swap, or target-cache filesystem. The controller must admit expensive compiler gates only when observed host/cgroup capacity, swap, disk reserve, and the cross-epoch expensive-work slot permit it; it must bound Cargo/test concurrency, preserve the managed cache, stop only its owned process tree at critical pressure, keep the local epoch resumable, and show the same observed telemetry in TUI RUN and ACP status.

## Methods and implementation

### Discovery and policy

`crates/vesper-harness/src/host_resources.rs` adds the provider-neutral `HostResourceGovernor` and typed telemetry.

- Linux reads `/proc/meminfo`, resolves the process's cgroup v2 or v1 membership, walks from the actual cgroup leaf to its ancestors, and uses the strictest finite memory limit with its corresponding current usage. This avoids assuming that `/sys/fs/cgroup/memory.max` at the mount root represents the terminal's own cgroup.
- Effective capacity is the lower of physical memory and the cgroup limit; effective available memory is the lower of host `MemAvailable` and cgroup headroom.
- The default policy preserves the greater of 4 GiB or 25% of effective memory, requires normal headroom beyond that reserve, treats 25% swap use as pressure and 75% as critical, and estimates one compiler/linker budget at 3 GiB. It caps Cargo at two jobs even on high-core-count hosts and reduces Cargo/test concurrency to one job at pressure.
- The governor samples the complete owned descendant tree from procfs, including RSS, process count, and `rustc` count. It also samples the filesystem containing the controller-owned target cache, current cache size, a 20 GiB-or-10% disk reserve, and 30 GiB expected gate growth.
- `ExpensiveGateScheduler` uses a file lock beneath the user-owned RRC state root. It serializes expensive compiler/linker gates across controller epochs without deleting artifacts.

### Executor admission, containment, and resumption

`crates/vesper-harness/src/release_executor.rs` constructs the governor only for controller-owned release workers. Its target directory is a digest-isolated subdirectory of the user-owned release-state `host-resources/targets/` cache rather than the source worktree's `target/`; the scheduler lives under the same state root.

- `workspace-verify`, `acceptance`, `architecture`, `msrv`, and `release-build` are classified as expensive. Supply-chain and advisory inspection remains cheap.
- Every admitted Cargo path receives inherited `CARGO_BUILD_JOBS`, `RUST_TEST_THREADS`, and `CARGO_TARGET_DIR`, including Cargo recursively started by `xtask`.
- Disk reserve plus expected growth, memory pressure, critical pressure, or an occupied expensive lease withholds a new expensive gate before a Cargo child starts. The activity snapshot keeps the gate context but clears `current_child`, so RUN never implies a process exists.
- While a child runs, the controller refreshes owned-tree telemetry. At critical pressure it calls the existing command-group/Job Object kill route for that owned child group, awaits settlement, and returns a `ResourceConstrained` terminal. The recovery reducer keeps `LocalVerification` resumable rather than recording a source failure or spending retry evidence.
- A telemetry read failure after admission is rendered as unavailable rather than asserted healthy; the controller still retains ownership and waits for the child.

### Host projections

`ReleaseWorkerSnapshot` carries `ResourceTelemetry` from the process-owning executor.

- The Ratatui RUN panel (`apps/agent-vesper-tui/src/ui.rs`) renders observed available/effective RAM, RRC-tree RSS/process/rustc counts, swap use, derived Cargo/test budget and pressure label, disk/cache values, and the latest governor action.
- ACP's controller status calls `render_active_worker_status`, which includes the identical structured `ResourceTelemetry::render()` values. Neither host fabricates a quota, capacity, progress percentage, or health value.

## Files changed for this capability

- `crates/vesper-harness/src/host_resources.rs` — Linux discovery, cgroup reconciliation, policy, admission, disk/cache accounting, scheduler, telemetry, and unit regressions.
- `crates/vesper-harness/src/host_resources_constrained_tests.rs` — deterministic constrained-memory and constrained-disk acceptance fixtures.
- `crates/vesper-harness/src/release_executor.rs` — governor construction, Cargo environment propagation, gate classification, owned-tree pressure stop, resumable resource-constrained result, and status rendering.
- `crates/vesper-harness/src/lib.rs` — shared module export and constrained-test inclusion.
- `apps/agent-vesper-tui/src/main.rs` and `apps/agent-vesper-tui/src/ui.rs` — registered worker snapshot projection and RUN resource rows.
- `apps/agent-vesper-acp/src/lib.rs` — shared controller status route remains the ACP projection.
- `xtask/src/main.rs` — exact acceptance enrollment for the resource-governor cases.
- `AGENTS.md`, `crates/vesper-harness/AGENTS.md`, `apps/agent-vesper-tui/AGENTS.md`, `apps/agent-vesper-acp/AGENTS.md`, `xtask/AGENTS.md`, `docs/AGENTS.md`, and this directory's `AGENTS.md` — durable contract/ownership updates.
- `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md` and `evidence-index.md` — requirement/evidence linkage.

## Exact verification evidence

All commands were run in the isolated `rrc-resource-governor` worktree with one Cargo build job and one Rust test thread where Cargo tests were launched.

```text
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo test -p vesper-harness host_resources --lib
running 9 tests
...
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 171 filtered out; finished in 0.00s
```

Those nine tests include strict cgroup-v2 ancestor selection, memory-over-CPU concurrency, swap pressure, disk reserve/growth refusal, exclusive expensive-slot serialization, live process telemetry, constrained critical-memory refusal before Cargo spawn, and constrained-disk refusal while retaining the cache measurement.

```text
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo test -p agent-vesper-tui registered_release_task_replaces_ready_and_no_active_tasks --lib
running 1 test
test ui::tests::registered_release_task_replaces_ready_and_no_active_tasks ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 267 filtered out; finished in 0.02s
```

```text
cargo fmt --all -- --check && CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo xtask acceptance
Acceptance regression gate: 41 exact cases passed in 20053 ms. Offline fixture model cost: zero; live-model effectiveness is not measured.
```

The enforced acceptance output includes these resource-governor cases:

```text
acceptance verified: host_resources::tests::cgroup_v2_discovery_uses_the_strictest_ancestor_limit_and_its_usage
acceptance verified: host_resources_constrained_tests::constrained_memory_acceptance_withholds_expensive_cargo_before_spawn
acceptance verified: host_resources_constrained_tests::constrained_disk_acceptance_preserves_target_cache_and_withholds_gate
acceptance verified: release_executor::tests::resource_governor_defer_keeps_local_epoch_resumable_without_source_failure
```

The acceptance run was offline and reported zero model cost. It does not substitute for live-provider, production-publication, installation, or cross-platform evidence.

## Deviations and unresolved items

1. **Linux implementation only.** macOS and Windows resource discovery intentionally returns an explicit unsupported error. Expensive local verification therefore fails safely until those platforms have truthful physical-memory, pressure, process-accounting, and filesystem backends. This report does not claim five-target governor acceptance.
2. **No destructive pressure experiment was run on Alex's host.** The constrained-host cases inject capacity/process/disk observations rather than changing the developer machine's cgroup, swap, cache, or running processes.
3. **Repository-wide Markdown-link sweep remains non-green for pre-existing index entries outside this work unit.** The attempted sweep identified stale historical foundation links (including missing dated supervision/VRO-19 records and many `Vesper bridge/recon/` paths). Those unrelated records were not rewritten. This report resolves the new RRC governor link referenced by the PRD and evidence index.
4. **The full `cargo xtask verify`, full workspace tests, and cross-platform CI matrix were not rerun for this constrained-host work unit.** The current acceptance gate and focused resource/TUI suites above were run with conservative local concurrency; no broader result is implied.

## Readiness effect

On supported Linux hosts, controller-owned expensive verification now observes real host/cgroup/swap/disk/process state, limits inherited Cargo/test concurrency, serializes expensive gates, protects the managed cache, and withholds or stops only owned work before it can be misrepresented as a deterministic source failure. The local RRC epoch remains resumable after a resource stop, while both hosts expose the observed reason and values.

This improves local safety and diagnostic truthfulness. It does not authorize a release or change the established requirement for exact-commit, platform-specific release verification.
