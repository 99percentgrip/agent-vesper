# Windows credential persistence repair — 2026-10-08

## Objective and current boundary

Repair the reported v0.24.10 Windows OpenAI device-sign-in save failure through
provider-neutral storage, preserve Linux/macOS behavior, and require actual native
persistence proof before the next release. The implementation and final focused local checks passed; native hosted
verification and release remain pending. This report does not certify publication or universal PRD parity.

Alex's screenshot shows a completed website sign-in followed by `Credential
storage is unavailable. Nothing was changed.` The previously green first-launch
and provider-menu cases did not exercise a successful subscription save. Their
immutable receipts remain valid for their named scope; they cannot certify this
missing behavior. The original Windows laptop has not tested this repair yet.

## Diagnosis and sources

The pinned Windows keyring store writes UTF-16 strings and rejects blobs exceeding
2560 bytes. Composite subscription JSON contains multiple tokens; the prior
single native entry therefore failed on Windows while Unix fallback concealed
that size difference. A synthetic record below the old 16 KiB bound reproduced
this failure without reading any real credentials. Primary contract:
[Microsoft CREDENTIALW](https://learn.microsoft.com/en-us/windows/win32/api/wincred/ns-wincred-credentialw).

A source review also found Windows lock contention was recognized only through
`ErrorKind::WouldBlock`. The updated paths recognize the documented fs2 native
contention error as well; a real Windows lease case and the hosted adapter fixture
require exclusion and release behavior.

## Changes and owning contracts

- `crates/vesper-auth/src/native_records.rs`: bounded immutable native chunk
  generations, SHA-256/length readback, native mutation journal, interrupted-write
  recovery, provider isolation and durable sign-out intent.
- `crates/vesper-auth/src/lib.rs`: Windows production routing and bounded lease;
  retain the public individual-secret 16 KiB validation bound, permit composite
  storage records up to 256 KiB, and reject serialized Unix vaults above 1 MiB
  before replacing previous data. Windows never writes a plaintext vault. A final compatibility review retained
  the original read-only vault precedence, preserving signed-out fixture isolation;
  Windows refuses mutations before native access if an old retained vault value
  cannot be safely retired. The new Windows-only regression proves these paths
  make no keyring access. Final-source format, workspace/Windows-auth Clippy, focused tests and shipping
  host builds passed after this refinement. Four real Linux first-launch and two
  authentication-navigation PTY receipts also passed; native Windows execution
  remains required before publication.
- `crates/vesper-auth/tests/native_persistence.rs`: explicitly hosted synthetic
  production-store save, fresh-process reload, rotation and sign-out acceptance.
- `crates/vesper-provider-openai/src/auth_tests.rs` and `src/credentials.rs`:
  explicitly hosted loopback device exchange followed by production adapter
  serialization/storage, restored selection, preserved API key, switch and logout.
- `apps/agent-vesper-tui/src/auth_settings.rs`: provider-neutral failure notices
  no longer invent a no-mutation or rollback receipt. ACP's existing typed error
  has no such assertion; shared persistence applies to both hosts.
- `.github/workflows/platform-foundation.yml`: require both native persistence
  cases on all five actual platforms before exact-commit publication.
- [ADR 0032](../adr/0032-bounded-native-credential-records.md) refines ADR 0014;
  accepted historical ADR 0014 is unchanged. Owning auth, OpenAI, CI and ADR DOX
  describe the contracts and isolated hosted fixture exception.

## Methods and exact evidence

| Behavior | Required evidence | Current result |
| --- | --- | --- |
| Original single-entry size failure | Windows-sized backend synthetic regression | Red assertion reproduced |
| Large record, restart, rotation, deletion | Public production store on five native runners; fresh child process | Pending exact-source hosted execution |
| Device result remains stored and selected | Hosted loopback exchange plus production adapter persistence | Pending exact-source hosted execution |
| Interrupted chunk/primary write | Named private-backend regressions | Final private-backend suite passed |
| Integrity/missing data and ambiguous changes | Named private-backend regressions | Final private-backend suite passed |
| Unicode and bounded composite size | Named private-backend regression | Final private-backend suite passed |
| Failed sign-out cleanup does not resurrect credentials | Durable deletion intent regression | Final private-backend suite passed |
| API key and other providers remain isolated | Rotation/isolation tests and hosted adapter preservation case | Private-backend pass; hosted pending |
| Windows cross-process exclusion | Windows native lease regression and hosted adapter contention | Pending native execution |
| Unix composite and aggregate vault overflow | Private owner-only vault regression | Final private vault regression passed |
| Existing RRC lifecycle and closeout | `cargo xtask acceptance` plus native RRC release receipt | Pending current-source run; historical .10 Complete retained |

Receipts retained locally so far:

- `/tmp/vesper-windows-credential-size-red.log`: one named assertion failed with
  the former single-entry save behavior.
- `/tmp/vesper-windows-credential-size-green.log`: initial digest formatting
  compile failure; this is not a behavioral pass.
- `/tmp/vesper-windows-credential-size-green-rerun.log`: the named size regression
  passed after the bounded-storage repair and digest formatting correction.
- Initial auth suite: 13 private cases passed. Initial OpenAI suite: 66 passed,
  one explicitly hosted case ignored. Default native persistence integration:
  two ignored cases; ignored cases do not count as native proof.
- Windows target `cargo check -p vesper-auth --all-features`: passed before the
  final contention/bound refinements. Windows Clippy then found three unnecessary
  returns, which were corrected; final rerun remains required.

Final focused-source receipts:

- Format check and strict workspace all-target/all-feature Clippy: passed.
- Auth suite: **15 passed**, no failures. OpenAI suite: **66 passed**, no
  failures; one hosted-only case ignored locally. The separate native-store
  integration has two ignored cases locally, neither counted as passing proof.
- TUI truthful credential-failure notice: **one exact case passed**; other
  filtered test binaries were zero matches and are not counted.
- Windows auth all-target/all-feature Clippy: passed. Attempting the combined
  Windows OpenAI cross-check on Linux failed in aws-lc's C build because a GNU
  compiler cannot build that MSVC target. Native OpenAI compilation/execution
  remains required on the Windows runner; no cross-check pass is fabricated.
- Architecture: **31 packages validated**.
- The broader preliminary `cargo xtask verify` returned zero, including all
  workspace tests and **149 exact acceptance cases**. Source refinements were
  made during that initial run; the final-source focused suite, format, workspace
  Clippy and Windows-auth Clippy were rerun afterward. Native RRC will rerun its
  complete local gates on the committed candidate before pushing; the preliminary
  run alone is not an exact-commit release certificate.
- Shipping-feature native TUI/ACP build: passed. The actual native controller
  must still reach its final release closeout, not merely publish assets.

Local receipts and source hashes are retained in
[the repair evidence manifest](2026-10-08-windows-credential-persistence-repair-evidence.json).
MSRV and all native hosted receipts remain pending.

## Deviations and unresolved items

No live OpenAI calls, real credentials, or user-state reads/writes were used for
foundation verification. Hosted OS persistence is explicit, ignored by default,
and limited to synthetic entries under UUID fixture identities on disposable
runners; cleanup owns those entries. Alex's installed binaries were not replaced.
No new version, push, tag, or release has been performed for this repair.

## Readiness effect

The save failure is reproduced and the provider-neutral repair passed focused
local verification. Release readiness remains pending complete native RRC local
gates and exact-SHA native matrices. The previous menu-only evidence is explicitly insufficient for
credential persistence.


## Exact native receipt hardening before publication

Final review found that Cargo may return zero when a named filter matches no
cases. The child reload and hosted parent collector now require both the exact
named `ok` line and a one-passed/zero-failed/zero-ignored summary, in addition to
successful process exit. Offline Rust and Python regressions reject zero matches,
renamed cases, ignored cases and missing receipts.

The first native v0.24.11 attempt was stopped through `/release cancel` during
local workspace verification before any push or tag. Its controller settled
`Cancelled`/`Idle`, retained its checkpoint and used zero retry budget. No release
was published by that attempt. The corrected candidate must enter native RRC
again through ordinary admission; no journal clearing or counter reset is allowed.

Before that attempt, RRC deferred version preparation under actual exhausted
swap/narrow RAM margin. Two stopped, owned v0.24.10 verification binaries were
retained byte-for-byte off tmpfs, with original-path symlinks and SHA-256 receipts.
The same attached controller then resumed automatically under unchanged policy.
The active owner and user installations were untouched.


The strict-receipt refinement passed three offline Python parser/guard cases and
one exact Rust receipt case, including a real `--show-output` result accepted by
the same CI collector. Workspace and Windows-auth Clippy, format and focused
auth/OpenAI checks passed again. Parent invocations retain captured native backend
receipts with `--show-output`, keeping their named Rust result line intact.
The default integration suite now has one offline receipt case passed and two
native cases ignored; neither ignored case counts as native acceptance.

## Native release follow-up: terminal ownership and repair dispatch

The next admitted native epoch, `20261008T144956.264870742Z-7c3fae229f64`,
recovered automatically from measured resource deferrals and completed version
preparation, but workspace verification failed in the real-PTY RRC ownership case.
Its candidate was not committed or pushed; no tag or publication occurred.
The failed ledger and its repair admissions remain intact.

Three separate defects were then demonstrated and repaired:

1. The Unix test child inherited the release host's controlling terminal, despite
   sending its streams to a private slave. At a 32×140 parent terminal the named
   assertion failed because the telemetry marker was not visible. The child now
   creates its own session and controlling slave with async-signal-safe syscalls;
   parent slave handles close after spawn so EOF settles. The corrected exact
   case passed at the previously failing geometry. The initial 33×142 run passed
   before the fix and is retained as a non-reproducer, not fabricated red evidence.
2. The controller discarded a repair error when it was already diagnosing a local
   failure, then marked unchanged state idle. Only a newly classified local gate
   error may now continue into repair. Repair dispatch errors persist their cause
   and failed liveness without changing source-failure or retry evidence. The
   regression failed with the old behavior and passed after the repair.
3. With the cause visible, a bounded diagnostic resume exposed an invalid-request
   model rejection before HTTP dispatch. The TUI showed the selected model, but
   the RRC repair factory copied the boot configuration. Both TUI routes now use
   the normal execution projection for current controls, while provider dispatch
   still enforces entitlement. ACP's explicit route likewise uses effective
   session configuration, matching its existing natural-language behavior.
   No new provider-specific RRC branch or account-access inference was added.

The diagnostic resume consumed the second persisted repair admission; it is not
reset or erased. The owner quit cleanly after failure settlement. The corrected
committed source must enter ordinary native admission under explicit canonical
supersession; the old epoch and its bounded admissions remain historical evidence.

Focused evidence: all **89 executor cases passed**, including actual source-gate
failure, watchdog stops, repair error settlement and unchanged permission refusal.
The exact TUI selected-model/effort case and ACP fixture-provider session case
passed. Initial intermediary compile/hang/assertion failures remain in retained
logs; only subsequent named successful executions count as proof. Four added
exact cases are mandatory in `cargo xtask acceptance` on every native platform.
Full integrated verification, hosted persistence and publication are still pending.

Resource recovery retained only stopped, owned artifacts with full SHA-256
comparison before relocation off tmpfs: the prior installer verification target
and v0.24.10 host binaries. Original paths remain symlinks. No user installation,
active owner, governor threshold, ledger or retry counter was replaced or cleared.
Normal native release startup used the existing selected provider account; this
is distinct from offline foundation verification. Its observed repair rejection
had `InvalidRequest`, `retry=Never`, and no HTTP status, so it is not billing or
an external outage diagnosis.

Owning harness, TUI, ACP, terminal-test and xtask DOX were updated. Parent ownership
and child indexes remain unchanged because no subtree moved or new boundary was
created. The owning RRC PRD status links this evidence without altering its frozen
requirements. Final release readiness still requires every native local gate,
exact-SHA platform matrix, published asset inventory and actual Complete/Idle
closeout; successful menu navigation alone cannot prove credential persistence.

### Integrated local verification before corrected admission

`cargo xtask verify` exited zero: workspace format, strict all-target/all-feature
Clippy, workspace tests/doc tests, fixture/contracts/architecture checks and
**153 exact acceptance cases** passed. The final selected-model test additionally
checks the iteration cap and retains ordinary coding-turn refusal when account
discovery is unavailable. Z.ai and xAI cases compare repair configuration with
normal execution, including native compaction and hosted tools. The fixed PTY
case also passed with 32×140, 33×142 and 48×180 invoking terminals.
The final small test assertion refinement occurred during this preliminary run;
its exact acceptance rerun passed afterward. Strict Clippy is rerun after all
source edits, and the committed candidate still requires fresh native local gates.

Changed documentation links were checked after URL decoding. No new missing link
was found. Eleven pre-existing targets in the evidence index are present only in
the original checkout and remain outside this candidate; unrelated source and
reports were not imported. The retained document-check receipt distinguishes this
baseline from the newly added links rather than claiming every index link passed.

Final strict workspace Clippy and the shipping-feature TUI/ACP build exited zero
after all source edits. Frozen binaries, the failed native ledger, geometry
receipts, red/green/intermediary logs and integrated verification are retained in
[the RRC follow-up archive](2026-10-08-windows-credential-persistence-rrc-follow-up-receipts.tar.gz),
with hashes and source basis in the existing evidence manifest. The original
storage-repair archive remains unchanged. Native restart uses these separately
frozen binaries; Alex's installation remains untouched.

## Controller ownership status follow-up

Actual restart of the corrected native host exposed another status defect: a
persisted failed or ownerless epoch still rendered `RUNNING` in the TUI header
because the view contained a background task. A named renderer regression
reproduced that incorrect label. The unpublished epoch
`20261008T161337.467762340Z-f7d51b591385` was cancelled through native `/release cancel`
before candidate commit/push/tag. It settled Cancelled/Idle with its checkpoint
and zero retry use retained; the attached owner then quit with exit zero.

The shared snapshot now distinguishes controller ownership from child-process
activity. A registered controller remains active while passively waiting without
a gate child. A failed/ownerless snapshot is inactive. Both native frontends use
that shared status: TUI renders `RECOVERABLE` or screen-reader `RECOVERY REQUIRED`,
and the shared text status explicitly states active versus recovery required.
The existing active/deferred renderer case remains intact. An intermediary fix
incorrectly treated child absence as owner absence; that existing case failed,
was retained as failed evidence, and prompted the explicit ownership field.
The final complete renderer suite passed **33 cases** with both inactive-owner
and live deferred-watcher behavior. The new exact case is mandatory on all native
platforms. Final acceptance, strict Clippy, refreshed hosts and native release
remain pending for this follow-up; the earlier 153-case proof retains its scope.

Final source then passed **154 exact acceptance cases**, all **33 renderer cases**,
all **four shared progress cases**, strict workspace Clippy and the refreshed
shipping-feature TUI/ACP build. The separate final acceptance run made no source
edits during execution. Earlier intermediate failures remain in
[the ownership-status archive](2026-10-08-windows-credential-controller-status-receipts.tar.gz),
with source/binary/archive hashes in the evidence manifest. Native hosted OS
persistence and exact-commit publication are still required; no .11 tag exists.

The same refreshed binaries then passed four real Linux signed-out first-launch
cases and four TUI/ACP release-control cases, one per registered provider. Each
first-launch case showed the landing screen and all four provider choices,
returned from authentication with Back, and quit cleanly. Cross-host cases retained
partial matrices, refused unproved retries without writes, separated Published
from degraded main and propagated cancellation without changing epoch/run/tag/assets.
No provider call was dispatched. [Current host fixture receipts](2026-10-08-windows-credential-current-host-fixture-receipts.tar.gz)
retain these eight exact platform-scoped passes. Windows/macOS execution and
successful native subscription persistence remain required in exact-source CI.

## Managed-cache observation follow-up

The attached native epoch `20261008T163239.014212294Z-7bcc3aef98aa`
passed version preparation, workspace verification, acceptance, architecture and
Rust 1.88 verification. It automatically recovered from measured resource
deferrals without retry-budget use. Before supply-chain verification, the governor
correctly withheld admission because free disk space was below its reserve plus
growth margin. No candidate commit, push, tag or publication occurred.

Read-only inspection identified obsolete compiler incremental intermediates under
the repository-owned managed target. Reclamation revalidated each inventory,
refused symlinks and active compiler/open-handle use, and removed only inspected
entries last modified before noon UTC. Two operations reclaimed 10.7 and 25.1 GiB
of actual filesystem capacity. Source, executables, publication assets, user
installation and release journals were retained. This was operator cache
maintenance; RRC did not automatically delete those entries or lower its limits.

During the second reclamation, the resource watcher stopped with `NotFound` from
resource discovery. The target-size walker propagated missing directory/entry
observations. A deterministic production-scanner regression reproduced this
failure for an entry removed after enumeration and a directory retired before
its queued scan. This reproduces the filesystem race consistent with the native
failure; the original bounded diagnostic did not identify the vanished path.

The walker now omits only `NotFound` observations. Permission and other I/O
failures still propagate, filesystem free-space admission remains authoritative,
and the scanner never deletes entries. Two exact cross-platform cases are now
mandatory acceptance. The disappearance case failed before the correction;
both focused cases passed afterward. The stopped ledger was retained, then the
native epoch was cancelled and its owner quit cleanly before source mutation.
Existing failed evidence and retry admissions were not reset. Current-source
acceptance, strict Clippy, refreshed hosts and native release gates are required
before claiming this follow-up complete.

The expanded acceptance run passed **156 exact cases**, including both new
scanner cases. A formatting-only test assertion correction was applied during
that run; no behavior changed, and native RRC still requires its complete frozen
commit gates. Final format, strict workspace Clippy and shipping-feature TUI/ACP
build passed. The refreshed binaries passed four signed-out first-launch cases
and four provider-neutral TUI/ACP release-control cases on Linux. The
[cache-observation receipt archive](2026-10-08-windows-credential-cache-observation-receipts.tar.gz)
retains red/green logs, host identities, both stopped-ledger snapshots and the
guarded inspection/reclamation inventories. Hashes and scope are recorded in the
existing evidence manifest; earlier archives remain unchanged. Native Windows
and macOS persistence/execution, exact-source publication and final closeout
remain required.


## Settled .11 platform matrix and repair handoff follow-up

The native attached owner completed all eight local gates and pushed candidate
`61bc1ffeb4d1ec7ad710d582e794eb5ad3c75116`. It waited for all twelve prerequisite
jobs to settle: nine passed and three failed. Both Linux foundation lanes passed
native credential acceptance. Windows and both macOS foundation lanes failed
compilation before credential acceptance, which was skipped and is not proof.
No .11 tag or publication occurred.

The actual compiler causes were an immutable Windows `xtask` acceptance vector
and macOS `TIOCSCTTY` request type mismatch. RRC fingerprinted all three failed
jobs and admitted one focused worker for the two related causes. It edited both
sites, executed compilation, then wrote the required report/DOX files. The last
edit invalidated its observed proof. The controller correctly refused promotion
but lacked automatic continuation for this ordinary unfinished handoff. The
native owner was stopped with `/quit` after its persisted failed state, exit zero;
no live worker was interrupted. Its reserved admissions and full settled matrix
are retained in the failure-ledger snapshot, with no counters or journals reset.

The shared repair factory now continues a normal completed response with successful
mutations but no post-edit proof using its complete history and existing bounded
segment/iteration ceiling. It preserves the same admission and permissions and
requires final observed proof followed by controller-owned frozen-patch verification.
The prompt puts required documentation before the final check. Missing mutations,
interruption, cancellation and the ultimate ceiling do not authorize promotion.
The two-provider real Rust-command regression failed before continuation (four
requests, premature stop) and passed with continuation (seven requests and final
proof). It is mandatory native acceptance on every platform.

The two compile sites are corrected in the integrated source. The governor now
reads fresh `symlink_metadata` rather than `DirEntry::metadata`: the latter caches
enumeration data on Windows and could retain a retired entry's old size. This
additional gap is a source-backed inference, not an executed Windows failure;
[the Rust DirEntry metadata contract](https://doc.rust-lang.org/std/fs/struct.DirEntry.html#method.metadata)
describes the platform difference. The exact disappearance assertion is retained;
symlinks are still not traversed and non-missing errors still refuse admission.

Current follow-up: strict workspace Clippy passed. Expanded acceptance, refreshed
hosts, native platform credential proof and .11 release/closeout remain required.
The original Windows laptop has not validated this repair. Parent DOX ownership
and child indexes remain unchanged; harness/xtask local contracts were updated.


The first expanded 157-case run stopped at the older unplanned-segment fixture:
it had deliberately ended after reads/source edits without focused proof. The
initial repair-only suite likewise exposed success-path fixture gaps in initial
provider retry and lifecycle-denial coverage (17 passed, two failed). These failed
receipts are retained. The successful fixtures now execute an actual Rust test;
the lifecycle-denial fixture explicitly refuses verified completion. Existing
segment, retry, preserved-action and no-tag assertions remain. Final repair-only
suite passed **19 cases**, no failures. The new documentation-order case also
requires repeated unverified claims to stop at the shared four-segment ceiling.

The isolated repair branch was advanced to the already pushed .11 version commit
before final verification, preserving fast-forward main ancestry and the original
version number. This is still the same unpublished .11 objective; no second
version, tag or release was created. The failed epoch snapshot and unused native
repair worktree remain intact. The updated integrated source must enter native
RRC admission with its own scope-bound proof; historical gates are not reused.


Final .11 source passed **157 exact acceptance cases**, strict all-target/all-feature
workspace Clippy, format and 31-package architecture validation. The Windows
`xtask` cross-check passed. The isolated PTY request expression type-checked for
Intel and Apple-silicon macOS with explicit Rust 1.95.0; the first temporary-crate
attempt used the shell's different default toolchain without that target and is
retained as a failed environment check. These expression checks do not certify
native terminal behavior. The shipping-feature rebuilt TUI/ACP passed four
signed-out Linux first-launch cases and four shared release-control cases.

[Platform/continuation receipts](2026-10-08-windows-credential-platform-continuation-receipts.tar.gz)
retain all intermediate failures, final local passes, the full stopped ledger and
first-candidate causal logs. The existing evidence manifest binds 20 receipts,
source hashes and immutable rebuilt-host identities. Current-source hosted Windows
and macOS persistence, exact-commit matrices and final native closeout remain pending.


## Proof isolation and authoritative cancellation follow-up

### Objective, methods and retained failure

The integrated source `81dec11aee92f99426f25a596aeb222466ff25e9`
entered native `.11` release verification. A server restart killed the first
owner and compiler tree; the same frozen TUI automatically reconciled the
recoverable epoch and resumed it. The resumed full workspace suite failed
`repair_verification_changes_cannot_be_promoted_as_verified` at the assertion
requiring safe failure context. Fingerprint:
`5a3c65acf55aedf381bd9e1e30138d1bf17d192386d9d4ff6cd78cb2c1a05200`.

The RRC admitted one bounded model repair. Its final patch added wording to the
snapshot diagnostic, retaining the `verification changed` phrase already present
in the original source. Its focused test passed, and native full verification
started. This did not explain the original assertion. The operator stopped that
attempt before promotion/push/tag; no successful full-gate receipt or promoted
repair is claimed. The complete cancelled ledger preserves the admission and
unchanged release counters. The native repair worktree remains retained.

A read-only cache audit found that independent temporary `repair-composition`
packages shared the same artifact hash. A new deterministic regression compiled
two different projects into a private shared target and asserted separate proof
executables. It failed before the fix: both executable paths were
`shared-target/debug/deps/repair_composition-7d2322b3d901d44b` on this Linux host.
This proves artifact aliasing. The original full-suite assertion did not include
its actual safe error text, so aliasing remains a supported explanation rather
than a reproduced exact causal chain. A two-thread focused rerun before the fix
passed 90 cases; that successful rerun is retained, not represented as a repair.

Cancellation also reproduced a separate production settlement defect: the
cancelled checkpoint was relabelled with failed liveness for a late `stale release
checkpoint writer` error. The new regression failed against the original code.
The fix returns before late worker-error settlement when the durable state is
already Cancelled; it leaves the checkpoint, evidence, mutation journal and
admission counters intact. Existing active-epoch authorization/watchdog/failure
settlement still reports failures and remains tested.

### Files and contracts

- `crates/vesper-harness/src/release_executor.rs`: authoritative cancellation;
  unique temporary repair Cargo identities retained by Git repair worktrees;
  bounded real binary execution and diagnostic assertion evidence.
- `xtask/src/main.rs`: both new exact cases mandatory on every platform.
- Owning harness/xtask/foundation DOX, RRC section 37, evidence index and this
  report: stable contracts, exact evidence and unresolved platform scope.
- Release-objective provenance: the same Windows-authentication objective and
  `.11` target, with explicit canonical descendant lineage; no second version.

The source does not adopt the unproven model diagnostic change, weaken any
snapshot/promotion assertion, reduce test concurrency, alter resource reserves,
clear a ledger or create a manual release. The factory and isolated promotion
fixtures use per-workspace Cargo package identities; copied repair manifests
retain that identity. The regression runs both compiled binaries only after both
projects have been built, verifies nonzero passing tests, and proves neither
project can overwrite the other's proof executable.

### Evidence and readiness

Retained receipts currently include:

- `vesper-v0.24.11-server-restart-checkpoint.json`: interrupted owner snapshot.
- `vesper-v0.24.11-unproven-diagnostic-repair-cancelled-ledger.json`: original
  failure, stopped repair admission and no candidate/tag/publication mutation.
- `vesper-v0.24.11-shared-cache-diagnostic-red.log`: diagnostic instrumentation
  alone; 90 focused cases passed without reproducing the intermittent assertion.
- `vesper-v0.24.11-shared-artifact-alias-red.log`: exact executable collision
  assertion failed, 0 passed/1 failed.
- `vesper-v0.24.11-cancelled-checkpoint-red.log`: late settlement regression
  failed, 0 passed/1 failed.
- `vesper-v0.24.11-cache-and-cancellation-green.log`: initial repaired focused
  suite, 92 passed/0 failed with two test threads. The final bounded binary-run
  refinement is covered by the subsequent exact acceptance receipt.

Final local checks passed on the corrected source:

- `cargo xtask acceptance`: **159/159**, 220693 ms; no live-model effectiveness
  claim. Both added cases execute on every native target.
- Four shared-cache isolation/snapshot cases repeated **five times**, two test
  threads: **20 passed**, no failures. The original three verifier-mutation
  scenarios run for both fixture providers and both preparation/remote routes.
- Strict all-target/all-feature workspace Clippy, format check and architecture
  (**31 packages**): passed. Windows MSVC `xtask` cross-check: passed; this is
  compilation proof only.
- Shipping-feature TUI/ACP builds and frozen-host Linux process checks: passed;
  four signed-out provider/landing cases and four shared release-control cases,
  with private state and no provider calls.
- Owning report local links (**7**), JSON/receipt hashes and whitespace: checked
  before candidate admission. Root/apps/auth/OpenAI contracts remain unchanged:
  the follow-up changes shared cancellation and test proof isolation, and their
  existing ownership/provider/persistence rules still apply. Owning harness,
  xtask and foundation contracts were updated in the same candidate.

The evidence manifest's `proof_isolation_follow_up` binds final source and frozen
hosts to `2026-10-08-windows-credential-proof-isolation-receipts.tar.gz`, retaining
failed, stopped and intermediate receipts alongside the final checks.
No actual Windows/macOS credential gate for this new source has passed yet;
native exact-source release gates and original-laptop retesting remain pending.
A stopped local repair is not successful autonomous release proof. Native RRC
must finish its full local/hosted gates, publication and Complete/Idle closeout.
