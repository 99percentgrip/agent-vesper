# RRC publication observation recovery

## Objective and status

Repair the actual native publication-read stop and matching tagged-request refusal
observed during v0.24.8. Parent release source is
`9fada0cd06714ba1e754b332b5d44b39d3d9f373`. Work is isolated on
`repair/rrc-automatic-closeout`; Alex's dirty primary checkout, installed v0.24.7
and original TUI are preserved. This repair is outside immutable v0.24.8.
Final local source checks passed. Exact-version native gates and corrective
publication remain required.

## Methods, observed failure and corrections

Read the native controller ledger, worker ownership, exact GitHub prerequisite and
producing runs, and the shared executor/admission source. All eight native gates
and twelve prerequisite jobs passed. The native tag was pushed only after the
complete exact-SHA matrix. Producing run `37648330813` succeeded and v0.24.8 was
published at `2026-10-07T16:14:02Z`.

At `2026-10-07T16:13:52.605090703Z`, a read-only publication evidence request hit
its 120-second inactivity watchdog. The owned command was stopped, but the worker
exited while the publication could continue remotely. No failed source/CI evidence
existed. A matching `/release 0.24.8` then asked which release to continue despite
preserving the same objective, exact target and candidate. An explicit native
`/release resume` completed publication observation, current-main settlement,
existing Registry PR #539 update/readback and the durable completion receipt at
`2026-10-07T16:22:50.292469374Z`. This operational intervention is preserved as a
deviation, not represented as automatic recovery. The temporary native host exited
through `/quit` only after Complete/Idle; Alex's original TUI remained untouched.

Corrections in the shared foundation used by both TUI and ACP:

- Persist at most two automatic publication-observation watchdog retries per
  epoch. Retain the Publishing stage and immutable tag/candidate; the owned worker
  waits through its existing cancellable backoff before reobserving.
- Bound cumulative active publication observation and poll waits to two hours,
  persisting measured time across reload and matching admission; host idle time
  is excluded. Exhaustion returns a typed deadline rather than polling forever.
- The counter survives ledger reload and matching admission. Never reset causal
  full/infra/diagnostic budgets or create source failure evidence for a read timeout.
- Refuse this recovery for cancellation, authorization denial, invalid evidence,
  other watchdog operations, another stage or any uncertain mutation journal.
- Checksum downloads now use the owned typed external-command watchdog and shared
  cancellation token, retaining their 30-second bound and checksum integrity checks.
- A matching tagged request resumes observation only for the same repository,
  objective, exact target, admitted source/candidate and clean owned workspace.
  Different targets, changed implementation and uncertain writes still clarify.

## Verification receipts

- `vesper-rrc-publication-read-red.log`: the persisted bounded-read regression
  failed on the extracted previous behavior; cancellation/invalid-evidence refusal
  passed. The test seam initially forwarded the existing error unchanged.
- `vesper-rrc-publication-admission-red.log`: matching tagged request reproduced
  the unwanted clarification rather than launching the preserved epoch.
- `vesper-rrc-publication-recovery-green.log`: 11 publication-related cases passed,
  including both corrected regressions and existing immutable/integrity guards.
- Broader all-feature RRC suite: 196 passed, zero failed in 252.26 seconds.
  All 100 unique named cases from the existing 36-section/23-criterion trace
  occurred as successful executed cases in this log. This preceded removal of
  the now-unused untyped checksum runner; final-source native gates remain required.
- First strict Clippy stopped on that unused runner. It was removed rather than
  suppressed. Strict harness all-target/all-feature Clippy passed in 17.18 s after cleanup;
  the final cumulative-watch source passed again in 10.09 s.
- Final publication-focused suite after cleanup: 12 passed, zero failed in 11.71 s,
  including the production orchestration timeout-to-Published route under two
  fixture provider identities without another tag or consumed CI/source retries.
- Native default-feature TUI host build passed in 70 seconds; this is an execution
  host, not an installation or publication. Final native gates and corrective
  exact-source hosted release receipts remain required.
- The cumulative-watch red regression reproduced successful publication despite
  an exhausted observation budget. The corrected final publication suite passed
  13 cases, zero failed in 12.12 s.
- Intermediate `cargo xtask acceptance` executed 131/131 cases successfully
  in 323860 ms before the cumulative-watch addition.
- All five new named cases are mandatory in `cargo xtask acceptance` on every
  supported target. The final gate executed 132/132 exact cases successfully
  in 174148 ms, including all five new cases. Offline fixture model cost was zero;
  this does not measure live-model repair effectiveness.

[Evidence manifest](2026-10-07-rrc-publication-observation-recovery-evidence.json)
binds source and raw receipt digests. The [requirement supplement](2026-10-07-rrc-publication-observation-recovery-requirements.json)
records the actual existing 100-case observation and five new mandatory cases. [v0.24.8 native closeout](release-v0.24.8-closeout.md)
and its [local gates](release-v0.24.8-native-local-gates.json),
[exact-main prerequisites](release-v0.24.8-exact-main-prerequisites.json), and
[completion ledger](release-v0.24.8-native-complete.json) preserve the actual prior
release and explicit operational recovery.

## Files and DOX

`crates/vesper-harness/src/release_executor.rs` and `release_recovery.rs`, the
nearest harness contract, five mandatory `xtask` acceptance entries and their
owning contract, owning RRC PRD, foundation ownership/evidence index,
this report/companions and committed release-objective provenance. No provider
names or host-specific recovery path are introduced. Root/crates/docs parent
contracts and their child indexes retain ownership and were reviewed unchanged.
The prior cancellation report receives a separate subsequent-release section;
its interrupted candidate evidence remains historical.

## Deviations, unresolved items and readiness effect

The v0.24.8 immutable tag cannot include fixes discovered after tagging. Its
publication is certified; automatic recovery from this timeout was not. A later
corrective code candidate must pass its own exact-source local and hosted gates
before tagging. No new release exists merely because these regressions pass.
Repeated read failure stops truthfully after the persisted bound rather than
polling indefinitely. Live provider repair effectiveness and Alex's Windows 10
laptop acceptance remain separate; no universal bug-free/100% claim is made.
The pre-existing missing historical process-status document remains outside this
repair and is not fabricated. Documentation accompanies this code candidate;
post-publication-only receipts stay local until another authorized code change.

## Native release preparation

After the previous temporary owner was Complete/Idle and exited, the completed
epoch's disposable managed Cargo cache was cleaned with Cargo's scoped
`clean --target-dir` under the exclusive scheduler lock. No source, user state,
installation, other cache or user process was removed. Cargo reported 154591 files
(88.7 GiB logical) removed; filesystem free space increased from 149 to 184 GiB.
This is explicit execution preparation, not an implemented automatic cache-cleaning
feature or a weakened resource threshold. The secondary proof cache and copied
execution host remain available.

## Final local candidate receipt

Final acceptance: 132/132 exact cases passed in 174148 ms. Final strict
all-target/all-feature harness Clippy and formatting passed; final focused suite
passed 13/13. The native default-feature execution host rebuilt successfully
with the cumulative-watch fix. Its SHA-256 is `586470c932d7ebe77f7c0ffdb1cdb0c7a45b02f511d6f3bd75fed8d636e3a8c5`.
The [raw receipt archive](2026-10-07-rrc-publication-observation-recovery-receipts.tar.gz)
preserves red, intermediate and final scopes, including the earlier Clippy failure.
Native exact-version gates and hosted corrective publication remain pending.
