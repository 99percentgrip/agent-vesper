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
