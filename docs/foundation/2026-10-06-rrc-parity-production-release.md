# RRC parity production release — 2026-10-06

## Objective and status

Publish the requested patch release with the RRC parity and native OpenAI sign-in
repairs while preserving the newer changes already committed to production.
**In progress: native-pushed `bd49a5b3` passed all eight local steps and ten of
eleven settled hosted jobs. Windows exposed the previously documented xAI
oversized-SSE fixture timeout. The corrected fifteen-case HTTP suite passed
locally; native Windows focused proof and successor publication gates remain pending.
No release tag, publication or installation performed.**
The latest public release is `v0.24.4`; production already contains the unpublished
`0.24.5` version graph. The intended release target is `0.24.5`, subject to native
version validation and complete exact-source gates.

Owning PRD: [Release Recovery Controller](../Agent_Vesper_Release_Recovery_Controller_PRD.md).

## Source and methods

- Alex's dirty primary workspace at `0b5630d271965e7d9df3a0a09c116c7f8c44042e`
  is preserved. Only the explicit 51-path parity scope and its requirement trace
  were copied into an isolated Git worktree, committed as `a5652325`.
- Reconcile production `7cc391204449a62622c782e923e5954ad69ac1d7`, preserving its
  ancestry and newer autonomy, complete version graph, Host Resource Governor,
  progress/liveness, explicit target reconciliation and responsive OpenAI startup.
- Independent RRC additions share the historical implementation `60eacc5c`;
  use it as the behavioral three-way base, then reconcile Rust items by name and
  inspect each overlapping contract. Temporary merge helpers are preparation
  tools, never product runtime behavior or acceptance evidence.
- Keep the current production binding PRD prefix unchanged (SHA-256
  `e85011c696d769837c6ed5a83e4290e77eadc4ae27a9ea42cffe0966d37518db`). The previous
  public candidate's smaller prefix hash and receipts stay historical.
- Run current `cargo fmt`, locked workspace all-target/all-feature compilation,
  RRC tests, existing named acceptance, full canonical/MSRV/security/build gates,
  then all four exact-SHA hosted prerequisites before native tagging/publication.
- All controlled failure probes use the separate public acceptance repository;
  no private-runner billing requirement is introduced for this public project.

## Files and integration contracts

RRC reducer/executor and hosted repair factory, native ACP/TUI policy composition,
OpenAI authentication presentation, hermetic provider/voice fixtures, named acceptance
inventory, native workflow coverage, fixed-snapshot driver image and owning DOX docs.

- Transactional multi-family repair keeps strict mutation/hypothesis/nonempty-patch,
  native focused proof after the last edit, complete local gates and permission rules.
- Registered workers retain native resource safety, output/progress heartbeat,
  cross-process ownership/cancellation, host policy and uncertain-mutation journals.
- Ledger storage retains redaction, bounded bytes, stale/cancelled writer rejection,
  immutable epoch history and the newer non-flushing scheduling checkpoint path.
- Exact-version clean candidates are reused only after full version-graph validation;
  already-published release metadata stays immutable during later main repairs.
- Chromium headless `154.0.8037.92-1~deb12u1` is present in Debian security snapshot
  `20261005T000000Z` for amd64 and arm64. Retrieved package inventories have SHA-256
  `0f93fdcaaa31d94a709590f802edaa6b2101db69427c0e39282e71bde3f79fc6` and
  `347c5a87312392f82e6199607d56549541329ac3ef0a3a71232c87e375b02864` respectively.
  This metadata is availability evidence; native image acceptance remains required.

## Exact verification evidence

Preparation logs currently reside in task-owned `/tmp/vesper-release-*.log`; durable
receipts will be archived and digest-bound before delivery.

- Initial compile attempts failed on mechanical merge issues (duplicate manifest
  keys, acceptance tuple shape, borrowed admission signatures and a misplaced test
  fixture field). These failures are retained, not counted as passes.
- Fifth locked all-target/all-feature workspace check passed in 6.67 seconds on
  its intermediate source, with warnings subsequently addressed. Final source
  requires fresh canonical verification.
- First combined RRC selection executed 150 tests: **144 passed, 6 failed**.
  The corrected rerun passed **150/150**. The retained initial failures exposed
  clean exact-version reuse, poll-backoff expectations, local failure propagation,
  governor/permission fixture omissions and post-publication SHA tracking.
- Native governed preparation completed all seven local gates: full workspace
  verification, named acceptance (93 cases), architecture, Rust 1.88 MSRV,
  supply-chain policy, advisories and production release builds. Receipt:
  `vesper-release-governed-local-gates-tenth.log`. This precedes the final
  capacity-display correction; final committed-source gates are still required.
- Three current-source repairs have compiled red/green evidence: five-second swap
  trend windows prevent subsecond amplification while retaining sustained pressure;
  causal receipts select late stdout/stderr failure evidence before truncation;
  natural-language release admission inherits the real session permission policy
  in both hosts. Two actual ACP process fixtures passed with explicit isolated
  fixture permissions and cancellation.
- The displayed normal RAM requirement now includes the same near-full-swap and
  owned-process margins enforced by admission. Its new compiled regression failed
  before correction; all **17 resource tests passed** afterward. Safety policy is
  unchanged. The acceptance inventory now includes this case on every platform.
- Preparation commands use the owned target with `CARGO_INCREMENTAL=0` and dev/test
  `line-tables-only` debug information. Native gates preserve default resource
  policy. Earlier resource deferrals, genuine pressure stops, compile failures and
  fixture failures remain failures; they are not counted as successful gates.
- [Preparation manifest](2026-10-06-rrc-parity-production-release-preparation-evidence.json)
  binds ten logs in the companion preparation archive. This is intermediate
  evidence, not a current exact-SHA production certification.
- Earlier frozen public-source acceptance and actual producing publication are
  documented in [the continuation](release-recovery-controller-parity-continuation.md);
  those receipts do not replace current-source acceptance.

## Committed-source verification and runner incident

Candidate `d091dff7a0224bbcb4b8d5d090334b1c0d86a04a` completed native version
validation and **all seven local gates**, including **94 named Linux acceptance
cases**, full workspace verification, Rust 1.88, architecture, supply-chain policy,
advisories and production binary builds. The native ledger reached `CandidateReady`.
This source remains an intermediate candidate because the subsequent observed
runner-annotation repair requires fresh final-source gates.

The standard public controlled run
[37367171770](https://github.com/99percentgrip/agent-vesper-rrc-public-acceptance/actions/runs/37367171770)
queued every job, then cancelled all five without executing steps. Repository-owned
annotations report failure to acquire hosted runners. GitHub's
[official Actions incident](https://stspg.io/c11dc9nb1zdq) independently reports
runner-assignment delays. No billing prerequisite or private runner was introduced.
The short observer timed out while queued. The longer observer correctly refused
premature diagnosis/retry during partial cancellation, then exposed the native
404-log fallback's rejection of cancelled job identities. Those receipts are
failures, not controlled acceptance passes.

The fallback now accepts completed `failure`, `cancelled` and `timed_out` identities,
retaining the exact job ID, repository-owned check URL, bounded pagination, failure
annotation requirement, redaction and 403/transport refusal. The specific acquired-
runner diagnostic is selected and classified as strongly supported runner
infrastructure evidence. Cancellation without annotations does not establish an
outage, and generic cancellation retains its prior classification.

The new named regression failed compiled before correction; its two-case rerun
passed. An initial follow-up assertion incorrectly required the causal excerpt to
start at the diagnostic, contrary to the existing one-line context contract; that
fixture expectation was corrected to require retained causal content and the exact
classification. The initial follow-up failure remains archived. The current native
adapter subsequently read the actual five cancelled jobs, captured their owned
causal annotations, classified all five correctly and refused a full retry. That
read-only probe created only its isolated acceptance ledger, with no production
ledger or GitHub mutation. Both hosts use the shared adapter. The new case is
mandatory on every native target (95 Linux cases after enrollment).

[Integration manifest](2026-10-06-rrc-parity-production-release-integration-evidence.json)
binds these actual local, failed external, focused and native adapter receipts.
The public controlled source was pushed only to its separate test branch; production
main, tag, release assets and Alex's installation remain unchanged at this checkpoint.

## Deviations and unresolved work

Production advanced beyond the original parity candidate, so a reconciliation and
fresh verification are required before release. No acceptance scope is reduced.
Corrective-source local verification, fresh full hosted prerequisites, actual
publication/checksums/native closeout and the existing Registry PR update are outstanding. Live OpenAI account
completion and replacement of Alex's local installation are unexecuted.

## Readiness effect and DOX

No current production release-readiness claim yet. Nearest operational contracts are
reconciled for the combined behavior; parent indexes retain their existing boundaries.
Detailed final receipts and delivery summary will distinguish tested source, immutable
published source and any subsequent documentation-only main commit.


## Production candidate `477f9806` and corrective source

The registered native controller completed version validation and all seven local
production gates (including 95 mandatory Linux acceptance cases), then pushed
`477f9806ce0f3dd78ad96f86e76604f2ad58d22d` to production main. The actual four-workflow,
eleven-job matrix completed: canonical and MSRV failed; foundation failed on Linux
and both macOS targets, with Windows passing; all three web-driver jobs passed.
No failed or historical candidate authorizes publication.

The separate public acceptance repository executed fresh partial/red/repeated-red/
green controls for that exact candidate. Its five platform labels run on Ubuntu,
so they establish controller/evidence behavior, not five-target native acceptance.
The native adapter preserved the repeated causal fingerprint, refused premature
retry and redacted the canary. These passes remain scoped to `477f9806`.

Actual hosted causes and source corrections:

- ACP's generated metadata script put escaped JSON inside the printf format.
  Bash accepted it; Debian dash retained backslashes and returned invalid JSON,
  preventing the first expensive gate receipt. The literal `%s` argument fixes
  portability. The exact Rust-generated old/new scripts were executed under an
  extracted, hash-checked Debian dash package; no runtime was installed.
- The speech enqueue fixture imposed a ten-millisecond shared-runner benchmark.
  Its replacement holds actual worker synthesis and requires both caller returns
  before release. A deliberately blocking production enqueue failed the new
  assertion; original production bytes were restored and the clean suite rerun.
- The output-progress watchdog fixture had a 120-millisecond scheduling margin.
  The revised fixture still exceeds one inactivity window and requires every
  progress receipt, with adequate scheduling margin. Production timeouts are unchanged.
- Actual Rust 1.95 panic messages include a numeric thread ID, which the existing
  exact failing-test parser missed. The new mandatory regression failed with the
  old parser and accepts both old and numeric-ID formats after correction.
- Source inspection found model-issued repair Cargo using generic command execution
  rather than the controller governor. The shared production repair registry now
  inherits the exclusive lease, bounded jobs/test threads, managed cache, native
  progress watchdog and owned cancellation. Policy/workspace overrides refuse;
  missing governors refuse before dispatch. Actual temporary-crate Cargo execution
  verifies the environment/cache and released lease. Watcher cleanup also survives
  unwinding. Legacy ungoverned repair dispatch is restricted to unit-test fixtures.

While the Windows job was still running, the native remote watcher reached its
separate twenty-minute no-new-evidence limit and settled as `Escalated`. This is
required by binding PRD section 19; it is not weakened. The immutable epoch retained
zero used full/infrastructure/diagnostic retries, no captured incomplete-matrix
causes, and no tag/publication. The corrective source must be a genuine canonical
strict descendant. Native admission must archive the complete stopped record
before selecting its successor; no ledger reset or invented repair history is used.

The current sandbox command port lacks managed-cache/environment binding. Governed
repair Cargo therefore refuses selected sandbox routes before unsandboxed host
execution. This is an explicit remaining composition limitation, not a tested
sandbox capability. Production resource discovery remains Linux-scoped; non-Linux
backends are unavailable and fail closed. A real-provider repair producer was
prepared but not executed after the epoch stopped; fixture models and that prepared
producer do not establish live-model effectiveness.


[Corrective evidence manifest](2026-10-06-rrc-parity-production-release-corrective-evidence.json)
and [receipt archive](2026-10-06-rrc-parity-production-release-corrective-receipts.tar.gz)
bind the failed exact-source matrix, public controls and executed corrective proofs.
The current-source executor suite passed 59 cases; the restored speech suite passed
six and ACP metadata/process suite passed three. Full successor certification remains
pending. Parent DOX boundaries and child indexes are unchanged; only owning contracts
changed for the repair-command policy and fixture verification semantics.


### Corrective verification methods

Native default resource admission and its exclusive lease bounded each preparation
Cargo invocation (`CARGO_INCREMENTAL=0`, one admitted job/test thread and the owned
managed task cache). The temporary preparation wrapper supplies admission/lease;
it does not establish registered-worker live-pressure/watchdog acceptance. Production
release progression uses the real registered controller and native executor.

Executed commands: `cargo test --locked -p vesper-harness --all-features --lib
release_executor::tests`, `cargo test --locked -p agent-vesper-tui --all-features
--test r3_speech_worker`, `cargo test --locked -p agent-vesper-acp --all-features
--test release_resource_governor`, and `cargo xtask acceptance`. The first expanded
acceptance pass executed 100 exact cases in 116929 ms. A subsequent source trace
added repair-Cargo output/process heartbeat and registered-worker telemetry wiring;
its current 59-case executor rerun passed, including actual environment/cache,
heartbeat, bounded output, resource telemetry and settled-child assertions. The
100-case preparation receipt remains scoped to the preceding source revision;
final exact-source native and hosted gates must reexecute the complete inventory.

The resource governor withheld compilation while swap was nearly exhausted. An
inactive earlier proof cache contained 1,313,325,056 allocated bytes of generated
incremental data. Cleanup held its exclusive Cargo cache lock, checked 160 owner
processes and found no accessible active references; six protected noncompiler
processes were retained. Only incremental entries were reclaimed, preserving source,
final binaries, dependency libraries and receipts. The unchanged resource policy
then admitted compilation. Primary HEAD `0b5630d2` and its 142 dirty paths remain
preserved; Alex's installed application is untouched.


The final corrective preparation rerun passed all 100 exact acceptance cases in
126866 ms after the repair-command telemetry correction. Its actual managed-cache
case additionally requires the displayed command to match the executed Cargo args,
no stale gate, real resource/output observations, refreshed repair heartbeat and
settled child. The evidence manifest binds final corrected Rust bytes and all
preparation receipts; publication remains pending complete exact-source native and
hosted gates. The canonical provenance marker certifies this source-integration
work unit only, never release completion or unexecuted live-account acceptance.


## Complete `1dab7456` matrix and native host fixture correction

The registered native producer selected the strict canonical descendant and archived
`477f9806` through native admission. Its unchanged default resource governor stopped
only owned compilation groups under critical pressure, passively deferred and resumed
the same epoch. All eight steps passed: version preparation, full workspace verify,
100 exact acceptance cases, architecture, Rust 1.88, supply-chain policy, advisories
and release binaries. Native progression pushed `1dab7456db34c95b1f3385656bbfa84301d56b06`.
The complete four-workflow matrix settled with **10/11 successful jobs**: all five
native platforms, MSRV, supply-chain and all three web-driver jobs passed. Canonical
quality failed only at the subsequent native TUI/ACP observation fixture. Completion
assurance mutations both passed before that failure.

The native controller waited for all eleven jobs, captured the actual first causal
failure and fingerprint `52210489f5afa6dd82d6545806abda9cbabab03059888d2fe8087b34b98e625b`,
and retained unused retry budgets in `ClassifyingFailure`. Its temporary producer
truthfully refused model repair because no active model transport was configured.
No tag/publication was admitted. This corrective work does not claim live-model repair.

`release_hosts_pty.py` used the workspace path for its seeded identity and hash,
while both production hosts use the canonical Git common directory. The exact old
fixture reproduced `release: no active or persisted epoch` against actual native
release binaries. Correcting the identity exposed a second stale fixture field:
the synthetic Published mutation omitted required `objective_evidence_reports` and
`intended_commits` collections. Both are now explicit empty synthetic collections;
production schema validation is unchanged. The corrected fixture passed once and
three additional independent real TUI/ACP runs. Every pass retains partial matrix,
no-write blocked retry, Published/main-degraded projection, cross-host cancellation,
epoch/run/tag/assets preservation and zero provider dispatch assertions.

Actual proof command: `python3 apps/agent-vesper-tui/tests/release_hosts_pty.py
<governed-native-target>/release/agent-vesper-tui
<governed-native-target>/release/agent-vesper-acp`. The companion manifest records
both exact binary hashes, their verified `1dab7456` runtime source and corrected
fixture hash. Runtime Rust bytes did not change in this correction.

Fresh `1dab7456` public controlled runs executed red `37385308962`, repeated red
`37386201129` and green `37386593835`; native incomplete-matrix handling, repeated
fingerprint and retry refusal passed. Two initial partial probes failed by racing
settlement/dispatch visibility. The corrected test-only observer scopes the actual
GitHub adapter to its intended run and performs one native refresh per sample; it
fabricates no job state or logs. Five synthetic Ubuntu labels are not native-platform
certification. The native producer's initial temporary authorization helper also
incorrectly required version preparation to have already resolved the version.
It refused before Cargo/mutation. Corrected helper authorization binds the actual
admitted `0.24.5` objective and permits a null version only during local preparation;
natural-language admission resumed the same epoch with unchanged counters. All failed
helper/probe receipts remain archived rather than counted as acceptance passes.

[Host fixture evidence](2026-10-06-rrc-parity-production-release-host-fixture-evidence.json)
and [receipt archive](2026-10-06-rrc-parity-production-release-host-fixture-receipts.tar.gz)
bind 46 sanitized receipts, the complete matrix and native settled record, scoped
controls, actual red/green host proofs, helper recovery and preserved primary checkout.
The next clean canonical source must undergo fresh native gates, public controls and
all hosted prerequisites. Publication/checksums/native closeout/Registry remain pending;
Alex's installed application remains untouched. Nearest test and foundation contracts
are updated; parent ownership boundaries and child indexes remain unchanged.


## Complete `3614a98d` matrix and isolated OpenAI startup fixture

Native admission archived the preceding failed candidate and started a new epoch
for `3614a98d41d3299186b1355270e64c8826ae3a6d`. The registered producer passed
all eight local steps, including both exact 100-case acceptance executions, then
pushed through the native operation. A real target-filesystem resource defer at
step 5 preserved that epoch and resumed after guarded reclamation of inactive
task-owned preparation caches. The unchanged disk reserve was enforced. Actual
filesystem free-space gain for the test-executable reclamation was about 2.5 GB;
the larger allocated-file total is not the reclaimed physical-space measurement.

The complete production matrix has eleven settled jobs: MSRV `37397644904`, all
five native targets `37397644800`, all three web-driver jobs `37397644159`, and
canonical supply-chain passed. Canonical quality `37397644209` failed in
`openai_startup_does_not_wait_for_unselected_xai_discovery`: the clean runner
rendered required OpenAI authentication rather than the assumed conversation.
The native controller waited for the final Intel macOS job, then captured causal
fingerprint `e5645aa3789243ecdb6be1b67b0d946fc3bcc6e6dba1ff7fde74ef3b549bc8b5`.
It did not tag or publish; persisted retry counters remained unused.

The fixture had no selected-provider loopback route or credential, inherited the
invoker's environment, and left the credential files absent. Its corrected shared
launcher clears the environment, isolates HOME/XDG/global cognition, and writes
private signed-out OpenAI/xAI vaults. Every case uses the existing integration-only
OpenAI loopback adapter with synthetic credentials. The unselected-xAI case now
requires the real OpenAI loading frame while the selected catalog is held, keeps
the assertion that xAI receives no connection, and settles both fixture workers.
This changes test setup, not production sign-in rules or the browser-launch repair.

Focused command: `cargo test --locked -p agent-vesper-tui --all-features --test
openai_startup_responsiveness -- --test-threads=1`, under native default resource
admission and bounded Cargo environment. All three real-PTY cases passed in
3.74 seconds; an independent rerun passed all three in 3.63 seconds. The actual canonical failure is retained as the red receipt.

Fresh public controls on `3614a98d` passed red `37393925022`, repeated fingerprint
`37394081001`, and focused green `37394227894`, including actual partial-matrix
blocking and redaction assertions. These Ubuntu-labelled controls do not substitute
for native platform verification or certify the pending successor.

[OpenAI fixture evidence](2026-10-06-rrc-parity-production-release-openai-fixture-evidence.json)
and [receipts](2026-10-06-rrc-parity-production-release-openai-fixture-receipts.tar.gz)
preserve the complete stopped matrix, native record/log, controls, cache-reclamation
receipts and focused corrected cases. Successor exact-source prerequisites,
publication, downloaded-asset verification and Registry delivery remain pending.
Live OpenAI account/browser completion and actual model-driven repair remain
unexecuted; the sandbox managed-repair and Linux governor limits above still apply.

DOX pass: the nearest TUI test contract now requires cleared child environments,
private signed-out vaults, isolated roots and the selected-provider loopback route.
Root/application parents retain their existing authentication and public-release
contracts; their ownership/index did not change.

## Recurrent Windows oversized-SSE fixture (correction prepared)

The complete `bd49a5b3e481a831f8d5877a8854fc7060ef70fd` prerequisite matrix
settled before diagnosis: canonical `37406416154`, MSRV `37406416250`, web-driver
`37406416149`, and all four Unix foundation jobs passed. Foundation run
`37406416111`, Windows job `112084880583`, failed only
`tests::http::oversized_sse_event_settles_as_protocol_error` at `tests.rs:1103`,
with `Elapsed(())` at its five-second observation bound; 55 other xAI cases passed.
The native controller captured fingerprint
`522896d09b630f4b49a8e2d3540fa5a9926644469931f546d7eb4db3afe2208b`
only after all eleven jobs settled. It retained zero used retry counters, no tag
and no publication. The 28-minute Windows duration includes earlier compilation
and suites; the failed case itself exhausted its five-second observation.

Alex requested inspection of previous Windows documentation. The
[v0.24.0 release report](2026-09-25-v0.24.0-release-execution.md) identifies
this same oversized-SSE observation failure and its earlier change to five seconds.
The bound was retained in current source. Prior command-encoding, startup-readiness,
path-canonicalization and frozen LF repairs also remain present; this failure is
in the xAI HTTP fixture rather than those command or Git fixtures.

`crates/vesper-provider-xai/src/tests.rs` reused the UTF-8 fragmentation helper for
an event exceeding 1 MiB. That helper issued over 349,000 three-byte socket writes.
The prepared correction keeps three-byte writes for the small fragmented UTF-8
case and uses 16 KiB writes for oversized rejection, still spanning multiple reads.
It preserves `MAX_EVENT`, the five-second bound and `ProtocolError`, and adds an
explicit no-second-terminal assertion. Production transport code is unchanged.
The observed timeout and excessive fixture writes are established; their precise
Windows scheduling contribution is not independently profiled.

Governed focused command:
`cargo test --locked -p vesper-provider-xai --all-features tests::http:: -- --test-threads=1`
passed **15/15** in **0.65 seconds**, including oversized rejection, fragmented UTF-8,
cancellation, EOF and WebSocket behavior. Local Linux proof cannot close native
Windows acceptance. Fresh focused Windows execution and complete successor gates
remain required. The first formatting check reported one multiline assertion;
`cargo fmt --all` corrected it and the repeated check passed.

The default governor initially deferred the focused build for filesystem headroom.
Exclusive governor/Cargo locks and process-reference inspection guarded removal of
idle generated incremental files only; actual filesystem free space increased by
**11,263,885,312 bytes**. Sources, executable/library artifacts, historical worktrees,
ledgers, receipts and reserve policy were retained. Focused work then resumed under
the existing default governor. No installation or private billing prerequisite applies.

DOX: the xAI adapter contract now distinguishes fragmentation and oversize fixture
budgets and requires native Windows proof. Root, crate parent and documentation
parent contracts remain unchanged because ownership and release authority are unchanged.

The full xAI package passed **56/56** in **10.58 seconds**; the independent HTTP
repeat passed **15/15**. Frozen [Windows-SSE evidence](2026-10-06-rrc-parity-production-release-windows-sse-evidence.json)
and [sanitized receipts](2026-10-06-rrc-parity-production-release-windows-sse-receipts.tar.gz)
bind the complete failed matrix, native state and local correction.

### Focused diagnostic preparation deviations

The first temporary public diagnostic workflow dispatch was refused because its
plain YAML `run` value contained `tests::http::`. The corrected diagnostic uses a
block scalar and parsed YAML before dispatch. No test executed in the refused attempt.
The diagnostic branch initially retained the production objective provenance; native
admission selected that clean descendant for version preparation. No producer, local
gate, push, tag or publication was started for it. Diagnostic provenance is now a
separate non-canonical objective. The production variant explicitly supersedes that
old source-integration variant; native archival preserves the mistakenly selected
preparation epoch. No ledger bytes or retry counters are manually reset.
