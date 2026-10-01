# RRC current-pressure defer and zram repair — execution report

**Date:** 2026-10-01  
**Implementation commit:** `7760e5079538219e2b9c0dd7e8a75322b7d62444`  
**Scope:** Linux source repair, deterministic local verification, and one non-publishing Linux x86-64 release-profile TUI candidate.  
**Boundary:** No natural-language release request, RRC release epoch, version mutation, push, tag, GitHub Release, Registry update, installation, provider call, `swapoff`, `swapon`, service restart, or `drop_caches` action occurred.

## Objective

Repair the live resource-admission defect that rejected a healthy host because historical logical zram swap occupancy exceeded a fixed percentage. The controller must instead classify current risk from available/effective memory, cgroup constraints, configured reserve, gate headroom, owned process-tree RSS, memory PSI, swap trend, and zram physical backing. If current pressure prevents an expensive gate, it must persist one typed defer, watch passively with bounded backoff, and continue automatically only after stable recovery without spending retry budget or repeatedly spawning the gate. TUI state must remain visibly deferred rather than presenting `Ready` or `No active tasks`.

## Methods and commands

### Source and invariant inspection

- Re-read the applicable root, crate, app, xtask, documentation, and foundation `AGENTS.md` chain.
- Audited the complete worktree diff and the release ledger/executor/resource/TUI projections.
- Confirmed forbidden host-recovery operations are absent from the diff:

```sh
git diff --check
! git diff | grep -E 'swapoff|swapon|drop_caches|systemctl|service restart'
```

- Re-derived the authority path manually:
  1. Linux observation separates logical zram occupancy from `mm_stat` compressed and physical bytes.
  2. Pressure classification uses effective cgroup/host memory, effective available memory, reserve plus gate headroom, PSI, swap growth, zram physical backing, and RRC-owned RSS.
  3. Resource refusal resets the pending gate to `NotStarted`, records no source failure, and persists `ResourceDeferredRecord`.
  4. Passive observations update the persisted telemetry/backoff only; they do not call the expensive gate or consume retry budget.
  5. Three safely admissible `Normal` observations produce the sole recovery transition and milestone, clear defer detail, and return to `LocalVerification`.
  6. The registered worker carries an explicit `resource_deferred` flag into TUI visual and screen-reader projections.

### Focused verification

```sh
cargo test -p vesper-harness host_resources::tests -- --nocapture
cargo test -p vesper-harness host_resources_constrained_tests -- --nocapture
cargo test -p vesper-harness release_executor::tests::resource_governor_defer_keeps_local_epoch_resumable_without_source_failure -- --exact --nocapture
cargo test -p vesper-harness release_progress_tests -- --nocapture
cargo test -p agent-vesper-tui --lib ui::tests::registered_release_task_replaces_ready_and_no_active_tasks -- --exact --nocapture
cargo test -p agent-vesper-acp --test release_resource_governor -- --nocapture
cargo test -p agent-vesper-tui --test r3_voice_pack --all-features
for i in $(seq 1 20); do cargo test -q -p agent-vesper-tui --test r3_voice_pack --all-features || exit 1; done
```

Results:

```text
host_resources::tests: 17 passed
host_resources_constrained_tests: 2 passed
resource defer/recovery regression: 1 passed
release_progress_tests: 6 passed
registered deferred TUI projection: 1 passed
ACP process resource regression: 1 passed
r3_voice_pack after isolation repair: 10 passed
r3_voice_pack repetition: 20/20 invocations passed (200 tests)
```

### Repository and policy verification

```sh
cargo xtask architecture
cargo deny --all-features check
cargo audit
cargo xtask msrv
cargo xtask verify
cargo xtask acceptance
```

Final receipts:

```text
architecture boundaries validated for 31 packages
cargo deny --all-features check: exit 0 (allowed duplicate-version warnings only)
cargo audit: exit 0; 511 dependencies scanned; no vulnerability reported
cargo xtask msrv: exit 0 under rustup toolchain 1.88.0
cargo xtask verify: exit 0
cargo xtask acceptance: 45/45 exact cases passed in 22992 ms
```

The successful `cargo xtask verify` ran formatting, strict workspace/all-target/all-feature Clippy, and the complete all-feature workspace test suite. Existing explicitly environment-dependent contained-runtime tests remained ignored under their documented contracts.

### Current host-risk observation before candidate build

A fresh read-only Linux observation immediately before the build recorded:

```text
source commit: 7760e5079538219e2b9c0dd7e8a75322b7d62444
MemTotal: 29024219136 bytes
MemAvailable: 22139371520 bytes
effective memory: 29024219136 bytes
cgroup memory.max: max
cgroup memory.current: 16318259200 bytes
reserve: 5804843827 bytes
required gate headroom: 4294967296 bytes
normal admission floor: 10099811123 bytes
memory PSI avg10: some 0.00%, full 0.00%
swap total: 8589930496 bytes
swap free: 4265672704 bytes
zram logical: 4051939328 bytes
zram compressed: 1396037281 bytes
zram physical: 1722781696 bytes
classification: Normal; admitted: true
```

Logical zram occupancy was about 3.77 GiB, while actual physical backing was about 1.60 GiB. The decision used the physical value and current memory/PSI/headroom rather than treating the historical logical occupancy as physical RAM pressure.

Durable preflight receipt:

```text
/home/Alex/Projects/agent-vesper-prerelease-builds/v0.24.4-resource-defer-zram-20261001T125345Z/receipts/governor-preflight.json
sha256: 119e17bfcf54f315218a5cc19ca7b02a1bbded02fcd83d33b7e47d40ef3b2fe0
```

### Post-commit release-profile candidate build

The implementation was committed before any fresh release-profile build:

```text
7760e5079538219e2b9c0dd7e8a75322b7d62444 fix(rrc): defer release gates on current pressure
```

The build preserved the prior production candidate feature set and bounded compiler/test concurrency:

```sh
CARGO_BUILD_JOBS=2 \
RUST_TEST_THREADS=2 \
CARGO_TARGET_DIR=/home/Alex/Projects/agent-vesper-prerelease-builds/v0.24.4-resource-defer-zram-20261001T125345Z/target \
cargo build --locked --release --package agent-vesper-tui \
  --features docker,swarm,bridge,voice-kokoro,voice-flm
```

Receipt:

```text
build start: 2026-10-01T12:54:15Z
build end: 2026-10-01T12:59:31Z
exit code: 0
Finished `release` profile [optimized] target(s) in 5m 12s
observations: 63
minimum MemAvailable: 20553109504 bytes
maximum swap used: 4323299328 bytes
maximum owned build-tree RSS: 1925107712 bytes
maximum concurrent rustc: 2
```

Durable build receipts:

```text
/home/Alex/Projects/agent-vesper-prerelease-builds/v0.24.4-resource-defer-zram-20261001T125345Z/receipts/cargo-build-release-tui.json
sha256: e4fac640fc93d0f74565924c45a47dc62b2572b8bfccc174afb7c891a70e4867

/home/Alex/Projects/agent-vesper-prerelease-builds/v0.24.4-resource-defer-zram-20261001T125345Z/receipts/cargo-build-release-tui.log
sha256: ef4c6e7fe02b3ffac8afede8418f8faf474e27d806d378db07d272f84eda85ed

/home/Alex/Projects/agent-vesper-prerelease-builds/v0.24.4-resource-defer-zram-20261001T125345Z/receipts/build-telemetry.ndjson
sha256: 378ff4a03baeccf3a38e863765357fe891f93e0e559cfa13d7c359b1e3f954fe
```

The build was admitted by a source-matched read-only preflight and monitored at five-second intervals with bounded Cargo concurrency. It did not create an RRC epoch or acquire the production cross-epoch governor lease; it is candidate-build evidence, not a live automatic-defer/recovery trace.

### Candidate identity

```text
path: /home/Alex/Projects/agent-vesper-prerelease-candidates/v0.24.4-resource-defer-zram-20261001T125345Z-linux-x86_64/agent-vesper-tui
sha256: 85feb7013dc67e0435c083cb0c9ea684db11385fbf358cf0a669a5d87da0bb70
version: agent-vesper-tui 0.24.4
size: 27176688 bytes
mode: 0755
format: stripped x86-64 ELF PIE
source commit: 7760e5079538219e2b9c0dd7e8a75322b7d62444
```

The copied artifact was rehashed against the source build output and `sha256sum -c SHA256SUMS` returned:

```text
agent-vesper-tui: OK
BUILD_RECEIPT.txt: OK
```

The digest differs from the rejected candidate as required:

```text
rejected: c3d249f7488b9f07e429719ce46cdb138dd3411b96dfbe8278ef6fa00b93f144
repaired: 85feb7013dc67e0435c083cb0c9ea684db11385fbf358cf0a669a5d87da0bb70
DIGEST_DIFFERS=true
```

Candidate receipts:

```text
/home/Alex/Projects/agent-vesper-prerelease-candidates/v0.24.4-resource-defer-zram-20261001T125345Z-linux-x86_64/BUILD_RECEIPT.txt
sha256: 89981f465f8651c01a999e1fc3c7fb614d40e4f1e5b9f28dcde8d6b5ac630620

/home/Alex/Projects/agent-vesper-prerelease-candidates/v0.24.4-resource-defer-zram-20261001T125345Z-linux-x86_64/SHA256SUMS
sha256: 20b9f5ab6871070d7e53e6432b8c3ef5c282438498a839b3e9b4496e25561dae
```

## Files changed

Implementation commit `7760e507`:

- `crates/vesper-harness/src/host_resources.rs`
- `crates/vesper-harness/src/host_resources_constrained_tests.rs`
- `crates/vesper-harness/src/release_recovery.rs`
- `crates/vesper-harness/src/release_executor.rs`
- `crates/vesper-harness/src/release_progress_tests.rs`
- `apps/agent-vesper-tui/src/main.rs`
- `apps/agent-vesper-tui/src/ui.rs`
- `apps/agent-vesper-tui/tests/r3_voice_pack.rs`
- `apps/agent-vesper-acp/tests/release_resource_governor.rs`

Documentation closeout:

- `docs/foundation/2026-10-01-rrc-resource-defer-zram-repair.md`
- `docs/foundation/evidence-index.md`
- `docs/foundation/AGENTS.md`
- `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md`
- `docs/AGENTS.md`
- `crates/vesper-harness/AGENTS.md`
- `apps/agent-vesper-tui/AGENTS.md`
- `apps/agent-vesper-acp/AGENTS.md`

## Exact behavioral evidence

| Required behavior | Current evidence |
|---|---|
| Use current host/cgroup risk | `telemetry_from` computes effective memory and effective available memory from host plus cgroup values; policy derives reserve and required gate headroom. Focused cgroup and constrained-host tests passed. |
| Include RRC process-tree pressure | Live snapshots obtain complete owned-tree RSS/process/rustc counts; pressure classification includes RSS bounds and burst margin. Existing process-tree and governor suites passed. |
| Include PSI and swap trend | Linux PSI avg10 and per-minute swap growth are persisted and classified; rapid-growth and healthy-`MemAvailable` PSI regressions passed. |
| Separate zram logical occupancy from physical RAM | `/proc/swaps` supplies logical occupancy while `/sys/block/zram*/mm_stat` supplies compressed and `mem_used_total` physical bytes. Exact stale-logical-swap and independent-logical/physical regressions passed. |
| Persist typed defer | `ReleaseRecoveryState::ResourceDeferred` and `ResourceDeferredRecord` persist gate, timestamps, telemetry, normal/unchanged counters and next-check interval. Ledger/directive tests passed. |
| Passive bounded watch | Watch intervals back off from 5 to 15 to 30 seconds, update telemetry in place, and emit no polling milestones. Regression verified unchanged pressure produces no gate respawn. |
| Automatic recovery | Three safely admissible `Normal` observations transition to `LocalVerification`; no `/release resume` route is involved. Regression verified exactly one recovery milestone. |
| No retry-budget consumption | Resource defer and all watch observations preserve the exact pre-defer retry budget. |
| Truthful TUI state | Typed `resource_deferred` reaches `BackgroundTaskState`; visual and screen-reader render `DEFERRED` / `DEFERRED FOR HOST RESOURCES` and reject `Ready` / `No active tasks`. |
| No milestone spam | The defer path creates one defer milestone; unchanged observations create none; confirmed recovery creates one recovery milestone. |
| Exact incident covered | The regression uses the incident-scale 8 GiB zram device, 3,227,684,352 logical bytes used, 20.9 GiB `MemAvailable`, quiet PSI, and 900 MiB physical backing, and requires `Normal`. |
| New candidate differs | Repaired artifact SHA-256 `85feb701…bb70` differs from rejected `c3d249f7…f144`; checksum verification passed. |

## Deviations and corrective actions

1. The first two complete `cargo xtask verify` attempts failed the unrelated `f9_gate_separates_disabled_from_blocked_with_neural_selected` test. The test mutated process-global `XDG_DATA_HOME` from parallel test threads while comments incorrectly claimed the harness was single-threaded. The focused all-feature file passed alone, proving an isolation race rather than a product voice failure. The test guard now holds a poison-tolerant static mutex through environment restoration. Twenty repeated all-feature file runs passed, followed by one clean complete `cargo xtask verify` pass.
2. An independent delegated review was attempted but could not run because the configured OpenAI reviewer model was not present in the current account model list. No reviewer approval is claimed. The final audit was performed manually against the source, tests, command receipts, and forbidden-operation search.
3. `cargo deny` reported existing allowed duplicate-version warnings and exited successfully. They were not introduced or rewritten by this repair.

## Unresolved items and limits

- No live RRC epoch was intentionally forced into resource pressure, so automatic passive recovery is proven deterministically at the production seams rather than by inducing workstation exhaustion.
- The candidate is Linux x86-64 only. It is not five-target release evidence and does not replace exact-commit canonical, MSRV, five-target foundation, or contained web-driver release workflows.
- The candidate has not been installed or run by Alex for interactive acceptance. The existing installation remains untouched.
- No remote release or publication action has been authorized or performed.
- Non-Linux resource discovery remains explicitly unavailable/fail-closed under the existing governor contract.

## DOX closeout

The nearest documentation contracts, production harness/TUI/ACP contracts, and owning RRC PRD are updated to index this report and the durable current-pressure/deferred-state behavior. Parent crate/app indexes are intentionally unchanged because no child DOX file was added, removed, moved, or renamed and the existing child ownership boundaries remain current.

## Readiness effect

The rejected fixed logical-swap threshold is replaced by current-risk classification that distinguishes logical zram occupancy from physical backing. A constrained gate now remains a durable active task, watches passively, and resumes automatically after stable recovery without retry expenditure or UI contradiction. All deterministic local gates, MSRV 1.88, architecture, dependency policy, RustSec, complete workspace verification, and 45-case completion acceptance passed. A fresh checksum-preserved Linux x86-64 TUI built from the committed repair and differs from the rejected artifact. This establishes repaired local candidate readiness only; release, installation, remote matrices, and Alex-operated interactive acceptance remain separate and unexecuted.
