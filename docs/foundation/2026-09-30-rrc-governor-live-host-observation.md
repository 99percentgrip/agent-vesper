# RRC Host Resource Governor — live Linux capacity observation

**Date:** 2026-09-30 13:30:36 UTC

**Status:** Read-only Linux observation complete; requested TUI test target unavailable; no candidate or release action.

**Scope boundary:** This is a point-in-time, source-matched observation of the current Linux host and the default `ResourcePolicy`. It does not start an RRC epoch, construct `HostResourceGovernor`, acquire its scheduler lease, create its managed target directory, run an expensive Cargo verification workload, rebuild artifacts, dispatch a provider, or make any release-side effect.

## Objective

Inspect the actual Linux capacity inputs and policy values that the RRC Host Resource Governor would derive for this worktree now, then run the requested governed TUI test invocation only if its named target is present. Preserve a precise no-work/no-release boundary.

## Methods and commands

1. Re-read the root, application/TUI-test, documentation, and foundation-evidence DOX contracts.
2. Inspected:
   - `crates/vesper-harness/src/host_resources.rs` — Linux discovery, effective-capacity reconciliation, policy arithmetic, and `CargoResourcePolicy` environment.
   - `crates/vesper-harness/src/release_executor.rs` — default state-root governor construction and repository-scoped managed-target naming.
   - `crates/vesper-harness/src/release_recovery.rs` — default release-state root and repository identity.
   - `apps/agent-vesper-tui/tests/` and the TUI package test targets.
3. Used a read-only Python observation script that mirrors `HostResourceGovernor::snapshot(None)` inputs and `ResourcePolicy::default()` arithmetic against `/proc/meminfo`, `/proc/self/cgroup`, cgroup-v2 memory files, process CPU affinity, and `statvfs`. It follows the source's rule to measure the nearest existing ancestor when the target cache does not exist.
4. Attempted the requested target with conservative Cargo/test concurrency:

   ```text
   CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 cargo test --locked --offline -p agent-vesper-tui --test rrc_governed_resource_recovery -- --exact rrc_governed_resource_recovery
   ```

   `CARGO_TARGET_DIR` was deliberately not pointed at the absent controller-managed cache: doing so would require Cargo to create user state and could trigger the rebuild the directive prohibits. Cargo resolved the requested target before compiling or running a test.

## Files assessed and created

Assessed:

- `crates/vesper-harness/src/host_resources.rs`
- `crates/vesper-harness/src/release_executor.rs`
- `crates/vesper-harness/src/release_recovery.rs`
- `apps/agent-vesper-tui/tests/AGENTS.md`
- `apps/agent-vesper-tui/tests/rrc_terminal_pty.rs`
- `apps/agent-vesper-tui/tests/` (target inventory)

Created:

- `docs/foundation/2026-09-30-rrc-governor-live-host-observation.md`

No production, test, Cargo-manifest, lockfile, fixture, candidate, release-state, or installed-application file was changed.

## Exact observation evidence

At `2026-09-30T13:30:36.282840+00:00`, the repository identity was
`/home/Alex/Projects/agent-vesper/.git`. The default release root resolved to
`/home/Alex/.local/state/agent-vesper/release-recovery`; the corresponding
controller-managed target cache would be:

```text
/home/Alex/.local/state/agent-vesper/release-recovery/host-resources/targets/abde39a5088456752525d06909d10c51fbafd907be499cb113abf15d9afce206
```

That target cache and its `host-resources` parent were absent. The read-only
measurement therefore used the existing release-root filesystem, exactly as
`disk_capacity` would until `HostResourceGovernor::new` creates the empty
managed target directory. No controller was instantiated, so the baseline
owned-process tree is the production `snapshot(None)` value: `0` bytes RSS,
`0` processes, and `0` `rustc` processes.

The source-matched live values were:

| Governor input/output | Exact bytes/value | Rendered value |
| --- | ---: | ---: |
| Physical RAM (`MemTotal`) | 29,024,219,136 | 27.0 GiB |
| Available RAM (`MemAvailable`) | 21,496,590,336 | 20.0 GiB |
| Cgroup memory limit/current | unavailable / unavailable | unrestricted / unavailable |
| Effective RAM | 29,024,219,136 | 27.0 GiB |
| Effective available RAM | 21,496,590,336 | 20.0 GiB |
| Swap total / free | 8,589,930,496 / 6,970,822,656 | 8.0 / 6.5 GiB |
| Swap used (integer floor) | 18% | below the 25% pressure gate |
| Logical CPUs (current affinity) | 20 | 20 |
| Reserve (`max(4 GiB, 25%)`) | 7,256,054,784 | 6.8 GiB |
| Normal headroom (`max(2 GiB, 10%)`) | 2,902,421,913 | 2.7 GiB |
| New-expensive-gate RAM threshold | 10,158,476,697 | 9.5 GiB |
| RAM above reserve | 14,240,535,552 | 13.3 GiB |
| Derived pressure | `Normal` | `Normal` |
| Derived Cargo jobs / test threads | 2 / 2 | `CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=2` |
| Target filesystem total / free | 1,021,431,513,088 / 368,278,962,176 | 951.3 / 343.0 GiB |
| Target filesystem reserve | 102,143,151,308 | 95.1 GiB |
| Expected one-gate growth | 32,212,254,720 | 30.0 GiB |
| Required free space for expensive gate | 134,355,406,028 | 125.1 GiB |
| Managed target size | 0 | 0.0 GiB |
| Disk admission predicate | `true` | sufficient headroom |

The cgroup-v2 traversal found `memory.max = max` at every applicable level;
therefore the production code has no finite cgroup limit/current pair to
reconcile and retains the physical-memory values. Under the default policy,
the observation's action string is:

```text
verification admitted within the RRC resource budget
```

This is capacity/policy eligibility only. Full expensive-gate admission still
requires acquiring the controller's exclusive scheduler lease, which this
read-only observation intentionally did not attempt.

## Requested test receipt

The requested TUI test source and Cargo test target are not present in the
current worktree. Cargo returned before compiling or executing a test:

```text
error: no test target named `rrc_governed_resource_recovery` in `agent-vesper-tui` package
help: available test targets:
    pr4_f9_gate
    pr4_wiring
    r3_speech_worker
    r3_voice_pack
    rrc_terminal_pty
    voice_execution_policy
    voice_flm_assets
    voice_flm_route
    voice_interruption_lifecycle
    voice_multiturn_playback
    voice_playback_diagnostics
    voice_policy_parity
    voice_provider_neutrality
    voice_r20_default_capture
    voice_r6_binding
    voice_speech_pipeline
```

The similarly named existing `rrc_terminal_pty` target validates terminal
stream ownership, not a `rrc_governed_resource_recovery` test; it was not
substituted. No specific assertion ran or failed, so no replacement test was
written.

A prior independent full verification is recorded in
[`2026-09-30-rrc-governor-acp-process-verification-rerun.md`](2026-09-30-rrc-governor-acp-process-verification-rerun.md).
It is historical evidence for the inherited worktree, not a fresh verification
receipt from this read-only observation.

## Deviations and unresolved items

- The requested `apps/agent-vesper-tui/tests/rrc_governed_resource_recovery.rs`
  file and its matching Cargo target are absent. This is an unavailable test
  target, not an assertion failure.
- The managed target cache is absent. Constructing the real governor would
  create it and the scheduler directory; this task intentionally did not make
  those user-state writes.
- Capacity, memory availability, swap and free disk are time-varying. The
  values above are a timestamped observation, not a standing host guarantee.
- The observation does not prove an actual controller worker's owned-process
  telemetry, lease contention behavior, pressure stop, Cargo-path inheritance,
  cross-platform backends, a candidate build, Alex-operated acceptance, CI, or
  release readiness.
- A broad traversal of the pre-existing evidence index still finds the historical
  missing target `2026-09-29-active-agent-supervision-0845z.md`. The new report's
  direct index and PRD links were validated separately; that unrelated historical
  record is not altered by this read-only capacity observation.

## Readiness effect

The current Linux host had sufficient observed RAM, swap and target-filesystem
headroom for the default policy at the capture time, with the derived
conservative concurrency of two Cargo jobs and two test threads. It does not
create an RRC epoch or establish that an expensive gate has acquired its lease.
The exact requested TUI regression cannot be claimed as passed because its test
target is absent; no test was invented or substituted. No release, candidate,
publication, tag, push, installation, provider dispatch, VRO-19 work, or
rebuild occurred.
