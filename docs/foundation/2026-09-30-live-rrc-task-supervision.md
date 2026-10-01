# Live RRC task supervision — execution report

**Date:** 2026-09-30  
**Status:** ACTIVE AND PROGRESSING; NOT STUCK AT OBSERVATION TIME

## Objective

Determine, without changing or signalling any process, whether Alex's other Agent Vesper instance running the fresh RRC task-lifecycle candidate is actively working, normally waiting on owned subprocesses, or stuck.

## Methods

Read-only inspection only:

```sh
ps -eo pid,ppid,user,lstart,etime,state,wchan:32,cmd --sort=pid
pgrep -af 'agent-vesper|cargo|rustc|xtask'
ps -eo pid,ppid,etime,state,%cpu,%mem,wchan:32,cmd --forest
ps -T -p 4129427 -o pid,tid,etime,state,%cpu,wchan:32,comm
readlink /proc/<pid>/cwd
readlink /proc/<pid>/exe
cat /proc/<pid>/task/<pid>/children
cat /proc/<pid>/wchan
python3  # bounded projection of the active RRC JSON record
sleep 8 # second process-tree observation
find .../release-recovery -type f -printf ...
```

No process was signalled, stopped, resumed, attached with a debugger, or modified. No file in the live release workspace or user-owned RRC state was written.

## Exact evidence

### Candidate process

```text
PID 4129427
started Wed Sep 30 12:40:12 2026
executable /home/Alex/Projects/agent-vesper-prerelease-candidates/rrc-task-lifecycle-20260930T042554Z/agent-vesper-tui-rrc-task-lifecycle
cwd /home/Alex/Projects/agent-vesper
state S
wait channel ep_poll
threads 25
```

The TUI itself is correctly idle in its event poll while the registry-owned background worker's subprocess tree runs. This is not the prior defective condition: the TUI owns a live child chain.

### Owned verification process tree

First observation:

```text
4129427 agent-vesper-tui-rrc-task-lifecycle
└─4136619 target/debug/xtask verify                    state S / do_wait
  └─4140833 cargo test --workspace --all-features     state S / do_wait
    └─4178242 voice_speech_pipeline-...               active test child
```

Eight seconds later:

```text
4129427 agent-vesper-tui-rrc-task-lifecycle
└─4136619 target/debug/xtask verify                    state S / do_wait
  └─4140833 cargo test --workspace --all-features     state S / do_wait
    └─4182541 context_paging_composition-...          active test child
```

The leaf test PID and test binary changed from `voice_speech_pipeline` to `context_paging_composition`. That is direct evidence of forward test-suite progression, not a launcher-only receipt or a stationary wait.

At the second observation:

```text
TUI elapsed 04:33, CPU 0.6%, wait ep_poll
xtask elapsed 03:41, CPU 0.2%, wait do_wait
cargo elapsed 03:08, CPU 2.2%, wait do_wait
context_paging_composition elapsed 00:07, CPU 0.3%, wait futex_do_wait
```

The parent `do_wait` states are normal: `xtask` waits for Cargo, and Cargo waits for the current test executable. At report closeout the leaf had advanced again to `routing_quality_eval-...` (PID 4183030), consuming 99.5% CPU after 1:32, while the same ownership chain remained intact.

### Persisted RRC projection

Active record:

```text
epoch: 20260930T044035Z-3f8ea52e7ee6
state: local_verification
release workspace: /home/Alex/.local/state/agent-vesper/release-recovery/worktrees/abde39a508845675/20260930T044034866Z-3f8ea52e7ee6
version: 0.24.4 -> 0.24.5
workspace-verify: running
command: cargo xtask verify
```

Later gates remained truthfully `not_started`: acceptance, architecture, MSRV, supply-chain policy, advisories, and release build. The controller has therefore progressed beyond admission, stale-epoch reconciliation, source resolution, isolated-worktree creation, and version mutation into the first real local gate.

The release workspace showed generated/updated build artifacts and version files, including `Cargo.toml`, `xtask/Cargo.toml`, `registry/agent.json`, `Cargo.lock`, `target/CACHEDIR.TAG`, and `target/.rustc_info.json`. The active ledger was written when `workspace-verify` entered `running`; it is expected not to change again until that gate settles.

## Classification

**WORKING — NOT STUCK.**

Reasons:

1. The exact fresh candidate process is running from the requested preserved path.
2. It owns `xtask verify`, which owns Cargo, which owns a real current test binary.
3. The leaf test changed during an eight-second interval, proving forward progression.
4. Persisted RRC state says `local_verification` and `workspace-verify: running`.
5. Parent sleep states are explainable waits on live children, not orphaned or childless waits.
6. No evidence of a panic, exited worker, zombie, missing controller child, or `Ready / No active tasks` contradiction was observed.

A separate older installed TUI process (`PID 3524937`) was also present, but it is not the candidate under test and does not own this release verification chain.

## Files

- Created: `docs/foundation/2026-09-30-live-rrc-task-supervision.md`
- Linked from: `docs/foundation/evidence-index.md`
- Linked from the owning RRC PRD.
- Indexed in `docs/foundation/AGENTS.md` and `docs/AGENTS.md`.

## Deviations

- This was a bounded live-process observation, not a completion wait. The active `cargo xtask verify` gate had not settled when observed.
- No terminal screen capture was taken; classification relies on authoritative process ownership, child turnover, and persisted controller state.
- No live acceptance input was generated by this observer.

## Unresolved items

- `workspace-verify` and all later local gates must still settle normally.
- A later check is warranted only if the same leaf process remains unchanged for an abnormal duration, CPU/context-switch counters stop advancing, the controller loses its child chain, or the ledger records failure.
- This observation does not authorize release, publication, tag, push, installation, or VRO-19 work.

## Readiness effect

The live candidate has passed the specific lifecycle observation that previously failed: admission produced a retained controller-owned process tree and real local verification activity. The release objective is still in progress, not complete.
