# CRITICAL — recurring tool-output stall: CLOSED — RELEASED IN v0.23.6

## Current issue status

**Severity: CRITICAL. Status: CLOSED — RELEASED IN v0.23.6** on 2026-09-25. The
classification-D shared executor defect reproduced red at the production
`RunCommand` boundary and is green after concurrent bounded draining, owned
process-tree cleanup, truthful settlement, and native five-target behavioral CI.
Exact release commit `1da983400602ec98fa0b6007d90a2591dbe71726` passed
canonical, MSRV, five-target and web-driver gates, and release workflow
`36069150695` published 16 checksum-verified v0.23.6 assets. The historical
incidents and manual recoveries below remain evidence of the pre-repair defect.

**2026-09-25 verification closeout:** the
[permanent-repair execution report](2026-09-24-critical-tool-output-stall-repair.md)
records the red-to-green repair, bounded capture, and TUI/ACP recovery. Exact
candidate `27a3f17bd818294e1a52e0bf194b02ba761e3464` passed the native
Windows x86_64, macOS Intel, macOS Apple Silicon, Linux x86_64 and Linux ARM64
behavioral matrix in workflow run `36036088105`, including descendant-held
pipes, process-tree cleanup and subsequent-command recovery.

The source/installed mismatch remains a historical provenance limitation rather
than a release blocker: current source independently reproduced the same
pipe-pressure mechanism at the real production boundary. The matched live
OpenAI/GLM comparison is **NOT EXECUTED — GLM live access unavailable**. Alex no
longer has an active GLM subscription/access and does not authorize restoring or
purchasing access for this diagnostic. This is an unresolved explanatory
limitation, not a release blocker for the independently proven provider-neutral
repair. The historical observation that stalls appeared more frequently with
OpenAI is preserved without a frequency explanation.

The [platform-verification record](2026-09-25-tool-output-stall-platform-verification.md)
documents the cross-platform test/ownership defects, Windows red-to-green
fixture corrections, exact-ref CI receipts and final five-target PASS.
The [v0.23.6 release record](2026-09-25-v0.23.6-release-execution.md) owns
exact-release-commit gating, publication, asset/checksum verification and the
ACP Registry update.

The [06:10 UTC recurrence](2026-09-21-agent-tool-stall-0610z-recheck.md) involves
a new shell `702497` and `cat` child `702500` under the same TUI `650318`.
At `06:17:18Z`, that command had reached **40m09s elapsed** and was still
blocked in `anon_pipe_write`, with unchanged child I/O and input position.
This is a local Linux observation, not a measured cross-platform failure.

[Latest authorized recovery — 2026-09-24 02:28 +08:00](#latest-recurrence--git-diff-child-1263357): the same TUI `1233587` had a
`git diff` child `1263357` blocked in `anon_pipe_write` for more than seven
minutes. The exact parent/command identity was checked; only that child was
SIGTERM-terminated, the TUI survived, and no direct children remained. This is
another confirmed recurrence of the shared output-pipe stall. Once the active
voice PRD work is complete, Alex's explicitly requested first engineering task
is the permanent tool-output-stall repair, before other follow-up work.

Required patch acceptance (**PASS** at approved scope):

1. Identify the actual installed and source capture paths; reproduce the hang
   in isolated regression tests that fail on the pre-fix implementation.
2. Drain stdout and stderr concurrently while the process runs, independently
   of retained-output limits. Large stdout, stderr, mixed output, and
   truncation must finish within a bounded deadline without unbounded memory.
3. Timeout and cancellation must return within their bounded budgets even
   when shell descendants retain output pipes. Shell exit alone must not be
   assumed to imply pipe EOF; verify descendant cleanup and no stranded
   readers/writers, without affecting unrelated processes.
4. Preserve truthful partial output, truncation, timeout/cancellation, and
   exit status. Never label a killed tool successful or replay an ambiguous
   side-effecting tool call. Keep the original session responsive and intact.
5. Verify the shared executor and both TUI/ACP host routes; cover the repeated
   large-instruction-file scenario and run applicable platform gates. Record
   unsupported/unexecuted cases rather than claiming universal resolution.
6. Provider correlation remains **NOT EXECUTED — GLM live access unavailable**.
   Preserve Alex's observation that the stall appeared much more frequently with
   OpenAI, but do not infer a rate or explanation. Fixture-backed OpenAI/GLM
   tool-surface and protocol checks plus the provider-neutral direct reproduction
   establish classification D; they are not described as a matched live-provider
   comparison. This explanatory limitation does not block release of the proven
   shared-executor repair.

These were the patch requirements established before implementation. The current
PASS rests on the linked red-to-green source and native CI receipts; the earlier
recovery evidence below remains historical.

[Provider-correlation observation](2026-09-21-tool-stall-provider-observation.md):
Alex reports that the symptom occurs more often with native OpenAI selected than
with Z.ai/GLM. Existing incidents lack matched workloads and denominators, so no
adapter fault or measured frequency difference is claimed. The permanent
investigation must separate shared output-drain behavior from provider stream/tool
event handling and their interaction.

[Separate native OpenAI malformed-Responses incident](2026-09-24-openai-malformed-responses-incident.md):
Alex also reported a provider-turn failure classified as `MalformedProtocol`,
with no HTTP/provider code, no visible output and the safe message `OpenAI
returned malformed or oversized Responses data`. This is a separate suspected
OpenAI Responses outage/degradation or protocol-handling gap, not evidence that
the local command-output stall is provider-owned. Its root cause and interaction
with accidental cancellation remain unverified.

[Latest authorized recovery — 08:32 UTC](2026-09-21-agent-stall-0832z-recovery.md):
identity-pinned SIGTERM to blocked child `726814` only; exit independently
verified, original TUI `718229` preserved and runtime I/O advanced. Sustained
task progress remains unverified. Alex directed current implementation and
voice PRD completion first, then focus on this permanent repair. Severity and
all patch acceptance requirements remain unchanged.

### Latest recurrence — git diff child `1263357`

At `2026-09-24T02:18` local time, the resumed TUI `1233587` had a direct child
`1263357` running this exact command:

```text
git diff -- docs/voice-oracle-extraction-prd.md docs/foundation/voice-r6-device-interruption-acceptance.md docs/foundation/evidence-index.md docs/AGENTS.md docs/foundation/AGENTS.md
```

The child was in `anon_pipe_write` for about seven minutes. An eight-second
recheck showed the same wait state and unchanged CPU time (`00:00:00`); the TUI
was alive in `epoll`. At `2026-09-24T02:28:35+08:00`, the child identity was
verified by PID, parent PID `1233587`, and the exact command line. SIGTERM was
sent only to `1263357`; no escalation was needed. The child disappeared, the
TUI remained alive, and `pgrep -a -P 1233587` returned no remaining direct
children.

This recovery is operational only. The shared stall remains **CRITICAL — OPEN,
PATCH REQUIRED**. Alex requested that the permanent repair become the first
engineering task after the active voice PRD is finished.

[Earlier authorized recovery — 08:20 UTC](2026-09-21-agent-stall-0820z-recovery.md):
identity-pinned SIGTERM removed blocked `cat` PID `726520` under TUI `718229`.
Child exit and original agent preservation were verified; runtime I/O advanced.
At closeout, a new `cat apps/agent-vesper-tui/AGENTS.md` child (`726814`)
was again blocked in `anon_pipe_write` with unchanged I/O across two samples.
The agent resumed tool activity but stalled again; the new child was not signaled.
Sustained task progress remains unverified. This recurrence remains **CRITICAL —
OPEN, PATCH REQUIRED**; no source fix or acceptance gate was executed.

[Earlier authorized recovery — 06:18 UTC](2026-09-21-critical-tool-output-stall-recovery.md):
at `06:18:52Z`, the identity-pinned shell `702497` and child `702500` were
terminated with SIGTERM. Both exits and original TUI survival were verified;
runtime I/O resumed, but sustained task progress remains unverified. Severity
and patch requirements remain unchanged.

## Earlier recovery: objective and status

Alex authorized terminating the previously identified command **if it still had
not moved**, preserving the other agent so it could continue, and documenting
the bug. No source repair was requested.

**Operational recovery executed and verified; underlying bug OPEN.** The same
blocked command was re-observed, then its shell and blocked `cat` child were
terminated individually with SIGTERM. Both exited; the original Vesper session
survived and resumed read/write activity. Task-level progress and completion are
not established by those runtime observations.

Environment: local Linux, installed binary
`/home/Alex/.local/share/agent-vesper/agent-vesper-tui`, PID `650318`, session
`0bf4fa51-9045-4d77-a47c-9a180dabeb46`. All new timestamps below are UTC.
Repository HEAD was `94ed16de502e98110498010b399b24659b17a63f`; this does not
assert that the installed binary matches that commit.

Related evidence:

- [Earlier status check](2026-09-21-agent-runtime-status-check.md).
- [Earlier recovery](2026-09-21-agent-tool-stall-recovery.md), for a different
  `cat` PID (`650363`) in the same Vesper session; its timestamps use UTC+08:00.
- [Immediate pre-authorization recheck](2026-09-21-agent-tool-stall-recheck.md),
  establishing this incident's shell `679687` and child `679694`.
- [Owning work PRD](../voice-oracle-extraction-prd.md): recovery does not close
  any voice requirement or acceptance item.

## Bug: recurring stdout-backpressure stall

**Observed behavior:** a read-only shell tool that prints instruction files
stays unfinished, while its `cat` child waits in Linux `anon_pipe_write`.
The original TUI remains alive and owns the corresponding output-pipe reader.
The stalled command blocks the agent's current tool operation until external
intervention. This symptom recurred after the earlier recovery.

**Expected behavior:** reading these files should finish or return a truthful
bounded failure/cancellation, rather than leave the agent waiting indefinitely
for its tool result. Output-size limits must not strand a writer.

Observed trigger, **not re-executed by this recovery**:

```sh
pwd; git status --short; cat AGENTS.md; cat apps/AGENTS.md; cat apps/agent-vesper-tui/AGENTS.md; cat apps/agent-vesper-tui/tests/AGENTS.md; cat docs/AGENTS.md; cat docs/foundation/AGENTS.md
```

The active child was `cat apps/agent-vesper-tui/AGENTS.md`, reading a 71,443-byte
file. The preceding recheck linked its stdout `pipe:[1743834]` to TUI fd `22`.
Its input position remained at `71443`; completed-I/O counters did not change.
The command had reached 14m24s elapsed immediately after signaling. Elapsed
process age is not a claim that every second was blocked.

**Diagnosis boundary:** pipe-output blocking is observed. Failure to keep draining
captured stdout is a candidate cause, not a source-level finding. Buffer capacity,
original timeout arguments, exact capture implementation, and installed-build
provenance were not investigated. No provider outage, quota failure, GPT reasoning
hang, broken configured deadline or OpenAI-adapter defect is inferred. Alex's
provider-frequency observation is retained as a required comparison, not causality.

**Open repair/acceptance work (not implemented or executed):** isolate the capture
path; reproduce large stdout/stderr output beyond capture limits without hanging;
verify draining continues when retained output is truncated; verify timeout and
cancellation settle the tool and its descendants with truthful partial-output and
failure status. Evaluate the shared behavior in both TUI and ACP, without
replaying possibly side-effecting commands. Manual termination is a workaround,
not a fix or blanket authorization to kill future processes.

## Methods and commands

1. Re-read root and applicable documentation AGENTS contracts.
2. Rechecked identities, executable paths, exact command lines, parentage,
   `/proc/<pid>/stat` start ticks, `/proc/<pid>/wchan`, descriptors, and I/O.
   Two samples three seconds apart established no progress by the blocked child.
3. Used a one-shot Python process-control command with `os.pidfd_open` and
   `signal.pidfd_send_signal`, aborting on any identity/command/wait-state mismatch.
   No script or production source was created. Pinned identities `(ppid, start_ticks)`:
   parent `650318: (7301, 20172954)`, shell `679687: (650318, 23685515)`,
   child `679694: (679687, 23685517)`.
4. Sent SIGTERM **only** to shell `679687`, then child `679694`. Terminating the
   wrapper as well prevents the semicolon-chained remainder from creating another
   writer on the same stuck tool output. No process-group signal or SIGKILL.
5. Polled the two pidfds for exit and independently inspected process absence,
   parent survival, I/O, and endpoint-redacted TCP states afterward.

Read-only observation commands included:

```sh
ps -o pid,ppid,stat,etime,time,wchan:30,args -p 650318,679687,679694
ls -l /proc/679694/fd
cat /proc/679694/io
cat /proc/679694/fdinfo/3
cat /proc/650318/io
ps --ppid 650318 -o pid,ppid,stat,etime,time,wchan:30,args
pstree -ap 650318
ss -tpn | grep 'pid=650318,' | awk '{print $1, $NF}'
git rev-parse HEAD
git --no-optional-locks status --short
```

## Exact evidence

Pre-signal samples at `03:12:23Z` and `03:12:26Z` showed the same child in
`anon_pipe_write`, at `13:06` and `13:09` elapsed. Both I/O receipts were:

```text
rchar: 75479
wchar: 0
syscr: 9
syscw: 0
read_bytes: 0
write_bytes: 0
cancelled_write_bytes: 0
```

Both input-position receipts were:

```text
pos:	71443
flags:	0100000
mnt_id:	58
ino:	3472770
```

Verbatim signaling/exit receipt:

```text
2026-09-21T03:13:40.390885+00:00
IDENTITY VERIFIED: original Vesper parent, exact shell command, blocked cat child; pidfds pinned.
SIGTERM sent to PID 679687 only.
SIGTERM sent to PID 679694 only.
EXIT OBSERVED via pidfd: PID 679694
EXIT OBSERVED via pidfd: PID 679687
PARENT IDENTITY PRESERVED: True
No signal sent to Vesper parent, terminal, or process group.
```

The immediate independent `ps` observation briefly showed the exited shell as
`[sh] <defunct>`; this was not reported as already reaped. Both target PIDs
were absent at subsequent checks. Verbatim follow-up at `03:14:41Z`:

```text
    PID    PPID STAT     ELAPSED     TIME WCHAN                          COMMAND
 650318    7301 Sl+     10:00:50 00:07:51 ep_poll                        /home/Alex/.local/share/agent-vesper/agent-vesper-tui --resume 0bf4fa51-9045-4d77-a47c-9a180dabeb46
PID 679687 absent
PID 679694 absent
ESTAB users:(("agent-vesper-tu",pid=650318,fd=22))
```

Verbatim parent I/O subsets before recovery (`03:12:26Z`):

```text
rchar: 10098929932
wchar: 5670663044
syscr: 577296
syscw: 249009
```

After recovery (`03:14:41Z`):

```text
rchar: 10099269860
wchar: 5672304613
syscr: 577528
syscw: 252709
```

These show renewed runtime I/O, not proof of a particular model response or
semantic work completed. Follow-up child snapshots contained no child rows;
short-lived tools between observations are not excluded.

## Files, constraints, and DOX pass

- Created this report; linked it from `evidence-index.md` and the existing runtime
  incident paragraph in `../voice-oracle-extraction-prd.md`.
- Read root `AGENTS.md`, `docs/AGENTS.md`, `docs/foundation/AGENTS.md`, the earlier
  recovery, current index, and the affected PRD paragraph before editing.
- AGENTS files and Child DOX Indexes intentionally unchanged: existing diagnostic
  ownership covers this incident; no durable behavior or workflow is changed.
- No source edits, builds, tests, installations, configuration changes, direct
  session-state edits, terminal input, or manually replayed tool calls.
- No provider request was issued by this intervention. The original agent's own
  resumed activity was allowed to continue as Alex requested.
- Existing unrelated source/document edits were preserved; their implementation
  status is outside this operational recovery.

## Closeout checks

At `2026-09-21T03:17:26Z`, PID `650318` was still the same resumed TUI;
PIDs `679687` and `679694` were both absent. Parent I/O had advanced further:

```text
rchar: 10281768001
wchar: 5951867849
syscr: 605957
syscw: 266923
```

HEAD remained `94ed16de502e98110498010b399b24659b17a63f`. In addition to this
report, the path/status comparison found a new task-related test file that
this intervention did not create or modify. Read-only `TZ=UTC stat` receipt:

```text
apps/agent-vesper-tui/tests/voice_playback_diagnostics.rs
size=1760 bytes
mtime=2026-09-21 03:16:18.136495409 +0000
birth=2026-09-21 03:15:31.300832320 +0000
```

This is additional evidence of task-related workspace activity after recovery,
not independent attribution of the writer, validation of the test, or completed
voice work. No source-content invariance claim is made while another agent is
active; all unrelated work was left alone.

`git diff --check -- docs/foundation/evidence-index.md docs/voice-oracle-extraction-prd.md`,
a trailing-whitespace check on this report, and relative-link existence checks
returned:

```text
changed tracked Markdown whitespace: PASS
report whitespace and local links: PASS
evidence-index and owning-PRD links: PASS
```

## Deviations, verification limits, and readiness effect

- The authorized command termination covered its exact shell wrapper and blocked
  read-only child, not the agent or a broad process-name/group match.
- `rg` was unavailable (`sh: line 1: rg: command not found`) during documentation
  lookup; ordinary `grep` and bounded file reads located the PRD references.
- This report documents an observed recurring bug, not a completed root-cause
  investigation. No regression test or source fix was attempted; no program,
  cross-platform, or release gate was run for the narrow operational action.
- Removal of this blockage, parent preservation, and renewed runtime activity are
  verified. Sustained task progress, later completion, and freedom from recurrence
  remain unverified. Voice/product acceptance status is unchanged.
- Readiness effect: current blocked invocation removed; underlying tool-output
  bug remains **OPEN**, with no release-readiness claim.

## Latest authorized recovery — test-output writer `11954` (2026-09-24 03:03 +08:00)

The active agent TUI `4537` launched shell `9799` for the voice-related test command. The shell waited in `do_wait` for `tee` PID `11954`, which remained in `anon_pipe_write`; the output logs stopped changing at 02:56:08 and 02:56:11 +08:00. Alex authorized termination. Only PID `11954` was signaled. At 03:04:11 +08:00, PIDs `11954` and `9799` were absent while TUI `4537` remained alive in `epoll`. The partial logs included passing tests, but no final exit status was captured. This is another operational reproduction and recovery of the shared output-pipe stall, not a source fix or task-completion claim. The full receipt is [the 03:03 recovery report](2026-09-24-agent-stall-0303z-recovery.md).
