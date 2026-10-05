# Release Recovery Controller parity continuation

**Date:** 2026-10-05 UTC / 2026-10-06 Asia/Manila  
**Objective:** Complete the binding RRC PRD requirements after Alex requested continued work to full parity.  
**Status:** COMPLETE for binding PRD sections 1–36. Final-source native, controlled GitHub, complete exact-commit prerequisites, actual producing workflow/assets and native closeout passed. Production publication and local installation were not performed.

## Methods and findings

- Re-read binding PRD requirements and applicable DOX ownership; preserve the prior deep-debug report and archived receipts.
- Publishing workflow formerly accepted any old successful push run for a SHA. `scripts/release_gate.py` now requires latest exact-SHA main-push identity, complete attempt-specific successful jobs, bounded pagination, freshness and a 120-second total request budget. Publication rechecks gates and retains the originally admitted driver artifact run.
- Last-green comparison wrongly inferred absence from a missing or skipped matching job. New regression failed before repair; comparison now refuses and retains unknown context.
- Bound native host Git metadata commands through the same process-tree executor.
- Native TUI/ACP fixture observes one ledger: partial matrix, blocked no-write retry, Published versus degraded main, and ACP cancellation observed by TUI with identity/tag/assets retained. No provider calls.
- Combined two-provider scripted fixture executes real file/command tools, failing/passing Cargo tests, isolated worktree, native focused proof and one-patch promotion. Its fixture-wide post-proof verification port is substituted; production still requires the complete native gate set. This satisfies fixture composition evidence, not real-model effectiveness.
- Persist measured native CI wait and model-repair wall time plus actual autonomous retry refusals. Pure status queries and idle time across host restarts are excluded; legacy checkpoints default the new measurements to zero without backfilled claims.

### Reproducible local commands

```sh
cargo xtask verify
cargo xtask acceptance
cargo test --locked -p vesper-harness --lib --all-features release_ -- --test-threads=1 --nocapture
cargo +1.88.0 test --locked -p vesper-harness --lib --all-features release_ -- --test-threads=1
cargo clippy --locked --workspace --all-targets --all-features --offline -- -D warnings
cargo test --locked -p agent-vesper-tui --lib --all-features auth_settings::tests -- --test-threads=1
cargo build --locked -p vesper-harness -p agent-vesper-tui -p agent-vesper-acp --all-features
python3 apps/agent-vesper-tui/tests/release_hosts_pty.py target/debug/agent-vesper-tui target/debug/agent-vesper-acp
python3 scripts/test_release_gate.py
```

Native fixtures use synthetic credentials/private roots. The read-only GitHub
probe sources, exact commands/logs, failed preparations and complete run/job
metadata are preserved in the sanitized receipt archive; its index pins every
receipt's byte count and SHA-256. Runtime source is pinned independently in the
source manifest; sanitized evidence-only probe text is not production source.

## Earlier candidate evidence

- `python3 scripts/test_release_gate.py`: seven offline prerequisite regression cases pass.
- Read-only real publication prerequisite check at v0.24.4 SHA `d22113528362706fa1672051dcb9385c82c22d8d` passed and selected driver run `36510375876`; historical published release, not current producing workflow acceptance.
- Missing/skipped last-green regression: red before repair, green after.
- `python3 apps/agent-vesper-tui/tests/release_hosts_pty.py target/debug/agent-vesper-tui target/debug/agent-vesper-acp`: native process fixture passed. First fixture versions incorrectly expected a captured cause before settlement and no TUI checkpoint root; those were fixture errors and are retained separately.
- Combined isolated AgentLoop/native proof/promotion case passed for both fixture IDs. Native worker wait timing and legacy checkpoint compatibility case passed.
- Previous candidate `2ca3dcbd28a1d98ac7fbf055b1eab858937ae494`: **74 release-filter cases pass on stable and MSRV 1.88 (73 RRC cases plus one existing native dependency guard)**, **60 exact acceptance cases pass**, and workspace clippy passes. Final frozen-source `cargo xtask verify` passed, including all 60 exact cases (25,275 ms for that gate). Earlier `verify` runs (56–59 cases) remain earlier-source evidence.
- Controlled previous-candidate real GitHub runs **37290746838** (red, observed one failed/four running before settlement), **37290935256** (same cause under a different run title, identical fingerprint and retry denied), and **37291227777** (five terminal green). The initial repeated-red probe omitted the explicit resume transition and correctly hit an illegal-transition guard; corrected probe follows the production worker resume path, with both receipts retained. These are Ubuntu-hosted protocol fixtures, not native platform proof.
- Previous-candidate native focused run **37290680196** passed all five targets: 73 RRC cases plus one existing native dependency guard per target, typed repair/retry, nine command-settlement cases and ten isolated voice-pack cases. Its predecessor **37290310437** is preserved but cannot certify the subsequently added failed-job-only port. GitHub's combined log omitted the ARM job; its direct job-log receipt is collected separately, with the collector failure preserved.
- Downloaded the exact amd64/arm64 browser `.deb` files from Debian security (61,168,348 / 54,265,316 bytes), checked official SHA-256 values and parsed package/version/architecture identities without installation. This is focused proof for the unavailable-pin failure, not contained-browser execution.
- After all focused native jobs settled green, private `main` was fast-forwarded from `28ad0b33bb5610bf61da5ab494c2772a6ac1304b` to the exact final candidate. That candidate’s production main-push runs: canonical **37292033400**, MSRV **37292033297**, five-target **37292033237**, web-driver **37292033235**. Web-driver is now complete green: both real contained-browser architectures and native namespace/Hive acceptance passed, with exact tested image artifacts retained. Canonical failed while LLD linked the TUI exact-acceptance binary; MSRV hit its thirty-minute job deadline during later acceptance cases. Foundation subsequently settled with all five targets green. No tag or blind rerun is admitted.
- Rebuilt native TUI/ACP parity fixture passed after measured metrics and cached redaction changes.
- Actual production-linked GitHub adapter recovered job **111672553145**'s disk-full failure annotation after its log endpoint returned 404. One thousand cached redactions took **80 ms** on this machine; this is a local measurement, not a platform latency guarantee.
- At that earlier checkpoint, exact-source prerequisites and producing acceptance were pending; final-source results appear below.

## Causal failures and focused repairs

Private candidate **28ad0b33bb5610bf61da5ab494c2772a6ac1304b** ran all four production prerequisites in `99percentgrip/agent-vesper-rrc-acceptance`. Historical failed runs: canonical **37282176136**, MSRV **37282176008**, web-driver **37282176033**, five-target foundation **37282176182**. The first complete foundation matrix settled with Apple Silicon/Linux ARM64 green and Windows/Intel macOS/Linux x86_64 red. No failure has been rerun without a state change; no tag has been created.

- Linux x86_64 passed the workspace and policy gates but exhausted disk at the explicit RRC lifecycle step. That command omitted `--lib --all-features`, compiling unrelated default-feature integration binaries after the full eligible build. Select the same exact lifecycle case in the existing all-feature unit binary. Every workspace case and that explicit process body remain required; local invocation executes one case.
- Canonical runner exhausted disk before uploading logs. The check annotation explicitly records `System.IO.IOException: No space left on device`; this is repository-side evidence, not an external outage claim. MSRV independently failed its native linker with the same disk exhaustion. Match the already-used platform workflow's non-incremental, line-table debug profile in canonical/MSRV/web-driver checks. Every test remains required.
- Both browser image lanes failed because Debian replaced exact headless pin `154.0.8037.57-1~deb12u1`. Refresh to **154.0.8037.92-1~deb12u1**, still exact. [Debian's package page](https://packages.debian.org/bookworm/chromium-headless-shell) and current amd64/arm64 security package inventories confirm that version; package identities/digests are retained in the receipts. Both native builds and browser/Hive checks remain required; no unpinned fallback.
- Windows command-settlement failed a three-second total-time assertion. PowerShell process startup was included in that bound. The descendant-after-leader cases now signal a leader-exit marker and measure settlement from that marker; they retain the three-second assertion and delayed-marker absence proof. Nine local native cases passed at this checkpoint; final Windows proof appears below.
- Intel macOS's persisted-cancellation fixture missed its two-second scheduling bound under the concurrent native suite. Retain the production 100-ms watcher polling; give the fixture a bounded ten-second scheduling window. Final native proof appears below.
- Local full verification again passed all ten voice-pack assertions and then crashed. A native debugger captured ONNX runtime finalization racing a detached speech worker loading Alex's installed neural pack. The test inherited the real workspace's saved neural selection; the worker outlived its environment guard. Replace unsafe in-process environment mutation with **one bounded child process per complete case**, setting workspace/HOME/XDG roots before startup and reaping the child before removing those roots. The construction case explicitly selects neural speech against an empty fixture pack. Ten cases and **50 complete suite repetitions (500 child cases)** pass. The old failing fixture did read the installed runtime; whether it wrote a lease is not established. Do not claim that earlier run was hermetic or alter Alex's installed assets to hide it.
- Ledger redaction repeatedly compiled fixed regexes for every persisted string. Cache the unchanged redaction and normalization expressions using thread-safe `LazyLock`; preserve all credential rules. The then-current 71-case release-filter suite passed in **5.47 seconds** locally.
- Missing runner logs now use bounded failure annotations only for the completed failed job's repository-owned check identity; absent causal annotations stay unknown, and credential/transport errors cannot take this fallback. Captured disk-full and unavailable-package errors are recognized as causal text, and worker-log timestamps normalize without hiding the actual cause.

The first full matrix is now settled. Focused candidate **34cf147d41e04930207310d43863f89769fb28c1**, run **37286179116**, passed all 72 RRC cases, typed repair/retry and nine command-settlement cases on all five native targets. Linux x86_64/ARM64 and Windows also passed all ten isolated voice-pack cases. Both macOS jobs then failed the new fixture-root identity assertion: `/var` is an alias of `/private/var`. A local symlinked `TMPDIR` reproduced that same failure before repair. Canonicalize the parent root before setting child cwd/environment; retain both identity checks. Ten cases and **50 complete suite repetitions (500 children)** through the symlink alias now pass. The failed native matrix is retained. At that checkpoint, a corrected exact candidate still needed focused native proof before its full matrix; later results appear below.

Current production-linked adapter probe observed controlled real GitHub red run **37286253506** at that candidate: five settled jobs, canonical workflow-ID identity, real causal panic, secret redaction and no-proof retry refusal. This controlled Ubuntu-hosted fixture does not substitute for native platform or producing release gates.

## Earlier private full-matrix failure and read-only diagnosis

- Canonical run **37292033400**: workspace tests passed, then `rust-lld` terminated with signal 7 (bus error) while linking the TUI exact-acceptance binary. Its annotation only records exit 1; **disk capacity is a hypothesis**, not a proven explanation for this run. Earlier explicit disk exhaustion and [LLVM's primary explanation of LLD SIGBUS with full mmap-backed output](https://lists.llvm.org/pipermail/llvm-dev/2017-October/118414.html) support testing capacity. Test/feature coverage cannot be reduced to obtain success.
- MSRV run **37292033297**: complete workspace tests and Stage 2/3/5 checks passed; sixty-case acceptance was still progressing when the thirty-minute job bound cancelled it. Preserve this as failed/unexecuted acceptance, not benign user cancellation.
- The captured complete Canonical log reproduced another RRC parser defect: `error:` matches a passing `test error::tests::... ok` line and hides the later real linker error. A current-production-linked offline probe fails on that exact receipt. Known OS labels also turn unrecognized log text into tentative `PlatformSpecificFailure`; it should remain `Unknown`.
- All five foundation lanes subsequently settled green. The complete prerequisite snapshot is two green workflows (foundation/web-driver), Canonical failed and MSRV deadline-cancelled. Only then were nine prepared owning paths promoted from the isolated worktree; no primary Git commit/push, tag or publication occurred.

## Prepared repair after full settlement

- Native regressions reproduce passing `error::` and `unauthorized` test names, generic linker wrappers and a preceding >4-KiB linker command hiding the true failure. One cached causal matcher now drives both excerpts and fingerprints, excludes test-status/exit wrappers and reserves bounded space for the diagnostic. The exact full Canonical receipt now selects `ld terminated with signal 7 [Bus error]`; the initial namespace-only fix still failed that receipt and is preserved.
- Unrecognized messages stay `Unknown` despite OS metadata; concrete unavailable package/version diagnostics become supported dependency failures. Remote operation cancellation stays failed/uncertain CI evidence and cannot become a locally user-cancelled epoch.
- Stable/MSRV `release_` selection passes **77 cases (76 RRC + one native dependency guard)**; the expanded fixed gate passes **63 exact cases**. The parser-repair source passed workspace clippy and 63 exact cases (35,276 ms). Its shared-target xtask root error is preserved; the later account-restriction source has a fresh passing primary-tree verification receipt.
- Canonical/MSRV use a guarded CI-only helper to remove unused Android/.NET/GHC SDK trees on disposable hosted Linux machines and record actual free space. Local invocation refuses before any mutation. MSRV's bounded job limit becomes forty-five minutes because the thirty-minute run was still progressing. All verification cases remain required.
- The next native focused gate compiles the workspace and default-doc graphs and runs the exact acceptance gate on both Linux architectures to test the link/storage hypothesis, alongside all five native RRC/repair/command/voice lanes. Further private development validation is user-requested verification of changed implementation; it does not reset or waive production RRC's persisted one-full-retry budget.

## Historical private account restriction and verification

- Candidate **`c802e93ea7ce53f97f84fdc39ac15287170a049c`**, native focused run **37300015141**, settled with all five jobs failed **before any step started**. Every owned failure annotation states: “The job was not started because recent account payments have failed or your spending limit needs to be increased.” This is repository-side account evidence, not a confirmed external outage or source test failure. No workflow retry, main push or tag followed that refusal.
- A new regression reproduced `Unknown` classification for that exact message. The shared causal selector/classifier now recognizes this execution restriction as a credential/permission failure; the core directive escalates for owner action without requesting model source repair, health/outage checks or full/infrastructure retry. The subsequent executor audit found that the real worker did not consume this directive; its correction and independent regression are recorded below. Ordinary billing words do not produce this classification.
- A current-production-linked **read-only real GitHub probe** reconstructed that exact five-job run, recovered its missing logs from owned failure annotations, verified classification and owner-action escalation, and refused both retry kinds. No provider or GitHub write was made.
- Frozen private candidate **`a3168652013ce2845474c76523dd17c21a65632c`** matched the then-current runtime/test/workflow bytes. That source passed **78 release-filter cases (77 RRC + one native dependency guard)** on stable and MSRV 1.88, **64 exact acceptance cases** (65,757 ms), workspace clippy and rebuilt native TUI/ACP ledger observation. Its then-current native and producing gates remained unexecuted. The twelve corrected library authentication cases pass, including the code-plus-URL browser launch, failed-launch retry/copy and complete-link validation; actual browser/account completion is not inferred.
- Local verification tooling errors are preserved: an xtask executable reused through a shared target directory retained its private-worktree root; cleaning that package while an earlier workspace suite was running removed its not-yet-executed unit-test binary and failed that run. The fresh primary-root `cargo xtask verify` subsequently passed, including all workspace/default-doc/stage checks and **64 exact cases in 26,103 ms**. The failed tooling run remains preserved. An auth test command initially selected the binary and executed zero tests; only the corrected library invocation is authentication evidence.
- The private test repository restriction is historical. It was an avoidable validation choice, not a requirement of RRC or normal public releases. Alex directed correction: use a separate public repository with standard runners, preserving production main/tags and the old private evidence. New exact-source native/prerequisite/publication gates remain required; no billing change is required by this validation plan.

## Public validation correction

Alex rejected the avoidable private-runner prerequisite and directed correction.
Created **`99percentgrip/agent-vesper-rrc-public-acceptance`**, a separate public
repository with standard hosted runners. Existing private visibility/history and
production main/tags remain unchanged. The public copy contains a fresh source
snapshot without private Git history, a visible test-only README notice and
minimal public audit summaries; detailed private diagnostic archives remain in
this workspace. Reviewed changed-file token-shaped matches: synthetic redaction
canaries and documentation filenames only; no unresolved sensitive-material flag.
The binding PRD §§1–36 and all runtime/test bytes are unchanged.

Public candidate **`35026933eff7720c98c6cc2c5d39732e325a9e6a`** contains the locally
verified source. Native focused run **37303833246** started real steps on all five
targets; the public route resolves the private-runner restriction. No payment or
billing change is required. Complete focused proof still must settle before
creating acceptance `main`, running the four main-push gates and admitting a test
tag/producing workflow. The old failed/preparation receipts remain historical.

First public-snapshot controlled runs **37304066998** (one failed/four actually running
observed before settlement), **37304250214** (identical normalized fingerprint,
no state change and retry refused) and **37304525331** (green) use the current
production-linked reader and an isolated synthetic ledger. These Ubuntu-hosted
protocol fixtures do not replace native runner proof or producing release gates.

Native public run **37303833246** subsequently settled **all five targets green**:
78 release-filter cases (77 RRC plus one native dependency guard), both named typed
repair integrations, nine command-settlement cases and ten isolated voice cases
per target. Both Linux jobs additionally passed workspace/default-doc linking and
all 64 exact cases. Direct job logs cover all five targets. Measured post-proof
free storage was **92 GiB x86_64 / 103 GiB ARM64** with **11 GiB** Cargo targets.
This proves the current public runner capacity; the earlier private SIGBUS cause
remains a hypothesis rather than a retroactively proven disk failure.

Only after that complete focused proof was acceptance `main` created at the same
SHA. Current four main-push prerequisites: Canonical **37305251282**, MSRV
**37305251328**, foundation **37305251429**, web-driver **37305251272**. No test tag
is admitted while any required workflow or job remains unfinished or unsuccessful.

## Executor directive integration correction (implemented; evidence below)

The public main-push matrices exposed no account restriction, but code audit found
that `advance_release` ignored the core owner-action directive and the continuous
worker requested repair permission/journaled classification before uncertainty
settlement. The new exact executor regression failed on the original public source
with an active-provider repair error. In an isolated local worktree, direct and
continuous routes now escalate account restrictions or request more evidence before
repair permission, model dispatch, health claims or mutation journals. Existing
local diagnosis without a captured failure still requires mutation permission.
Prepared local commit **`be1f945f654499f5cf52806fd23bd8dbce6884fa`** passes **79
release-filter cases** (78 RRC plus the existing dependency guard) on stable and
MSRV 1.88, **65 exact acceptance cases**, full workspace clippy, and `cargo xtask
verify` (including the repeated 65-case gate in 26,253 ms). Rebuilt native TUI/ACP
observation/cancellation and all twelve authentication UI library cases pass.
At that checkpoint, native/full publication checks for this additional source change remained pending;
preparation does not promote source while the previous complete matrices are
unfinished.

All four complete prerequisites for public source
`35026933eff7720c98c6cc2c5d39732e325a9e6a` subsequently settled green, including
all five foundation targets. The real release-gate helper passed that exact matrix.
Only then was correction `be1f945f654499f5cf52806fd23bd8dbce6884fa` copied into the
primary workspace (four scoped paths; all prior bytes checked) and pushed to the
public acceptance branch. That five-target native run **37311288734** subsequently passed.
No source main push or tag preceded its complete focused proof.
The earlier green matrices cannot certify this additional correction.

## Read-only infrastructure health and rerun policy correction (implemented; evidence below)

Native run **37311288734** settled all five targets green for source `be1f945`.
Before admitting a main push, permission audit reproduced another integration gap:
read-only infrastructure health diagnosis requested mutating/source-repair permission
and created a mutation journal. The new named worker regression failed on that
assertion (not compilation). In the isolated worktree, health diagnosis now runs
without source mutation permission; an admitted infrastructure rerun still requires
owner permission and journal settlement. The authorizer scans its actual scoped
`gh api --method POST .../rerun` command instead of unrelated Git patch commands.
The fixture proves a Git-only deny cannot substitute for GitHub policy, a GitHub deny
remains authoritative even under bypass mode, and denied reruns consume no budget.
An intermediate Rust match-expression compilation error was corrected and retained;
it is not counted as the reproduced runtime defect. The focused case now passes.
The corrected source **`d50f2cad784d666327dff919e2f1f21ee51343d3`** passes 80
stable/MSRV release-filter cases (79 RRC), all **66 exact cases** (36,660 ms), full
workspace clippy/verify (repeated exact gate 26,466 ms), twelve authentication UI
cases and rebuilt TUI/ACP process checks. All eight observed command exits are zero.
Four scoped paths were promoted only after previous native run 37311288734 settled
all targets green; no intermediate `be1f945` main push/tag was admitted after the
health-policy gap was known. Native run **37315097076** subsequently settled all five targets green, with direct job logs proving 80 selected cases and both typed repair integrations per target; Linux also passed the 66 exact case gate. Current
controlled run **37315700812** demonstrated actual failed-plus-running waiting and
settled causal capture using current-source production-linked probes. Repeated
cause/green/main-push/producing acceptance remains open. Probe build mistakes
(private method visibility and selecting an MSRV dependency with stable rustc)
were corrected and retained; no production visibility change or artifact clean
was made. The producing probe holds the same fs2 ownership lease in its isolated
ledger root.

## Repair tool authority correction (implemented; evidence below)

A native temporary-Git regression reproduced an additional bypass: the bounded
repair AgentLoop could execute `git tag rrc_unadmitted_fixture` through its normal
command tool before controller tag admission. The exact named test failed its
semantic assertion on the previous code; the preliminary short-name `--exact`
invocation matched zero tests and is preserved without being counted as proof.
An intermediate lifetime compile error is likewise retained separately.

The RRC-only registry now delegates core source tools under the existing authority
ports, restricts command dispatch to supported focused verification, refuses
explicit Git metadata write paths. Its initial implementation also cleared
provider-owned selections; the later retention regression below corrects that
mistake, while the client registry remains restricted. It refuses direct Git/GitHub lifecycle and compound-shell commands.
Source text mentioning `.git` remains editable. Rustup toolchain-qualified Cargo
proof uses the same nonzero-test and identifiable-failing-test checks; `cargo xtask
msrv` is supported. Verification subprocesses retain the existing configured
sandbox; this role restriction is not an additional OS confinement guarantee.

The new mandatory case
`release_executor::tests::repair_worker_cannot_create_unadmitted_release_tags`
passes with real native tools. Frozen public candidate **`ced7b9b767b9958b722eb54817c110f83c8f074d`** passes
**81 selected cases (80 RRC)** on stable/MSRV 1.88, **67 exact acceptance cases**,
workspace clippy and complete workspace/default-doc/stage/architecture verification
(repeated exact gate **27,111 ms**). All twelve authentication UI cases and rebuilt
real TUI/ACP process checks pass. A preceding eight-check run is preserved
separately from the final case-insensitive `.GIT` path coverage. All five scoped
runtime/owning-contract changes were compared against the previous source before
promotion; primary HEAD and unrelated dirty paths remain untouched. Native run **37321632214** subsequently settled all five real platform jobs green.
Direct logs prove all 81 selected cases, both typed repair cases, nine command
settlement cases and ten voice cases per target; both Linux targets additionally
prove all 67 exact cases.

Current-source real workflow **37322296191** was observed with a failed job plus
queued/running siblings: no causal capture and no retry before complete settlement.
Its settled causal capture and redaction probe passed. Repeated-red **37322561548** and green **37323091435** current-source
probes also passed, with unchanged fingerprint/refused retry and five terminal
green jobs respectively. The previous source `d50f2cad` also completed all controlled red/repeated-red/green probes
(runs **37315700812**, **37316196123**, **37319364570**). The repeated fingerprint
`2adb625d22fbc217556294d58b1d97cc4284ca059f63ff2ce10544e353e6e84d` remained unchanged
and a full retry was refused. No intermediate source is pushed to acceptance main
or tagged after a known source defect is found.

## Disabled host cap bypasses repair segment budget (implemented; evidence below)

The final native-factory audit found that both hosts use zero to disable ordinary
user iteration caps. `run_coding_turn_in_workspace` previously took `min(24)`,
retaining zero and thereby selecting the agent loop's 4,000-iteration ceiling
instead of the intended 24-iteration repair segment. The separate 20-minute
repair deadline still existed; this was a segment-limit bypass, not an unlimited
wall-clock claim.

The real-tool/fake-provider regression
`release_executor::tests::repair_iteration_budget_survives_disabled_host_cap`
executed distinct Cargo checks. Before repair, host cap zero produced **32 provider
requests**, failing the required **24** assertion. After repair, zero and 100
use 24 iterations and a smaller positive cap of five remains five. All three
exhaustions remain unsuccessful; the caller's ordinary configuration remains
unchanged. Existing native plan continuation and the separate repair deadline
remain enforced.

The prepared correction passes **82 selected cases (81 RRC)** on stable/MSRV,
**68 exact acceptance cases (43,812 ms)** and workspace clippy. At that checkpoint,
full workspace/authentication/host checks were still running before promotion.
No tag was admitted on source `ced7b9b` after this gap was demonstrated. Its now-historical
main-push runs are Canonical **37324862762**, MSRV **37324862819**, foundation
**37324862796**, and web-driver **37324862722**; their complete matrices settled
before the corrected source was sent through new CI.

## Preserve provider-owned selections across repair (implemented; evidence below)

The final audit also caught a regression introduced by the role restriction:
`AgentLoopConfig.hosted_tools` contains provider-owned server tool selections,
not client registry gateways. Clearing it removed previously selected native
search/other provider features from repair requests. The native fixture
`release_executor::tests::repair_preserves_provider_owned_hosted_tool_selections`
failed with an empty request list instead of its selected fixture tool.

The corrected factory preserves the caller's provider-owned selections and their
existing adapter policy. The client tool registry still has only its restricted
core executors and no hosted gateways; direct lifecycle/compound shell and explicit
Git metadata writes remain refused. The receiver fixture verifies the selection
on all three requests and leaves the caller configuration unchanged; this is
request data-flow proof, not evidence of live hosted-tool execution.

The preceding budget-only prepared commit **`a92f822e3b5a253eeaac9604c365b940809e018e`**
passed all eight local checks but was never promoted to public CI or primary source.
Its receipts remain separate. Locally frozen correction **`7394fc54be71ff761cce5873f2ca12ac91db616b`** passes
**83 selected cases (82 RRC)** on stable/MSRV, **69 exact cases (42,576 ms)**,
workspace clippy, complete workspace/default-doc/stage/architecture verification
(repeated exact gate **32,803 ms**), all twelve authentication UI cases and real
rebuilt TUI/ACP process checks. Only after all four preceding main-push workflows and every job completed green
was it promoted to the public candidate and five scoped primary paths, with
previous-source bytes checked before copying. Primary HEAD stays unchanged.
Current native run **37331489894** settled: four targets passed; Windows passed
83 selected RRC-related cases and typed repair, then failed command cancellation
fixture startup readiness after five seconds. Voice acceptance was consequently
skipped on that lane. Its failure is retained; no tag is admitted. Current
controlled source probes **37331597095**, **37332272036** and **37333915956**
settled expected red/red/green; the native reader passed actual partial-matrix
waiting, causal capture/redaction, repeated fingerprint/refusal and terminal green.
Source `ced7b9b` subsequently completed all four green main-push workflows,
including all five foundation targets. No tag/publication is admitted before the final corrected source
has its own complete native, prerequisite and producing evidence.

## Command fixture startup repair (implemented; evidence below)

Run **37331489894**, Windows job **111835428569**, failed with `cancel.ready`
absent after five seconds, before cancellation was requested. The receipt does
not establish whether shell startup or early termination caused the absence;
no outage or billing conclusion follows. The preceding eight remaining command
cases passed. The original readiness helper concealed early command errors.

The prepared fixture separates a bounded 15-second startup allowance from the
unchanged three-second post-cancellation settlement assertion, reports the actual
result of early termination, and aborts its fixture task on readiness timeout.
Production command deadlines and descendant-cleanup assertions are unchanged.
A real shell deliberately delays six seconds: restoring the old five-second
readiness bound fails one executed exact case; the corrected 11-case suite passes.
A second regression requires early-failure diagnostics. The focused native
workflow adds three extra executed Windows cancellation repetitions. These are
fixture repairs; cold Windows startup remains a hypothesis about the original
failure until the improved diagnostics and native proof run.

Receipts: `rrc-parity-hosted-native-windows-failure.log`,
`rrc-parity-hosted-native-settled.json`,
`rrc-parity-startup-readiness-five-second-red.log`,
`rrc-parity-startup-readiness-red.json`, and
`rrc-parity-startup-command-proof.log`. Final fixture source **`bd24bb48765d73c704fb81945ca87e33d093e121`** passes all
nine local checks, including complete workspace/default-doc/stage verification
and 69 exact cases (31,345 ms on the repeated full gate). It also passes stable
and MSRV 83 selected cases, clippy, 12 authentication UI cases and real rebuilt
TUI/ACP process checks. After checking each primary file against source `7394fc54`,
six scoped paths were copied and the public candidate fast-forwarded. Primary
HEAD is unchanged. Native five-target run **37337247051** passed all targets. Direct logs prove
83 selected cases, typed repair/retry, 11 command cases, one isolated ACP case
and 10 voice cases on every target, plus three extra Windows cancellations and
69 exact acceptance cases on both Linux architectures. Final main-push gates
passed; producing acceptance is recorded below.

## ACP provider fixture isolation (implemented; evidence below)

Prepared-source `cargo xtask verify` failed in the existing synthetic provider
fixture at `apps/agent-vesper-acp/tests/provider_selection.rs`: a receive timed
out after five seconds. The preceding focused RRC, 69-case acceptance, MSRV,
clippy and 11-case command checks passed. Preserve the nonzero full verification
receipt; it cannot certify completion.

Inspection found that this independent fixture, unlike the shared process
harness, did not supply explicit signed-out native provider records or an
isolated child cwd. HOME/environment isolation cannot exclude OS credentials.
The correction supplies owner-only signed-out OpenAI/xAI records, uses a private
temporary cwd and session workspace, drains stderr, diagnoses the missing response
ID, and reaps the owned child on failure. Protocol response/EOF assertions retain
their five-second bounds. A focused native process case and ten complete
repetitions pass. The native five-target workflow now requires this
fixture as well. The original timeout mechanism and any earlier credential access
are unestablished; no live-provider or user-state safety claim is backfilled.

Failed receipts: `rrc-parity-startup-pre-acp-local-verification.json` and
`rrc-parity-startup-pre-acp-verify-failed.log`. Focused proof:
`rrc-parity-startup-acp-focused.log` and the numbered repetition logs.

## Final-source native and protocol evidence

Source **`bd24bb48765d73c704fb81945ca87e33d093e121`** passed all five native jobs
in run **37337247051**. `rrc-parity-startup-native-log-checks.json` binds every
job ID, full log digest and nonzero case count. Controlled runs **37337917723**
(red with actual failed-plus-running waits), **37338534868** (same fingerprint,
no state change, retry denied) and **37339051487** (five terminal green jobs)
passed the current production-linked readers and persisted their synthetic ledger.

Two earlier partial probes correctly failed their own proof requirements: a
fresh terminal matrix superseded their first snapshot, and an immediate dispatch
listing initially selected an older completed run. A repeated-red watcher also
queried the listing before registration propagated. These instrumentation errors
changed no runtime behavior; sampling every five seconds and pinning the exact
intended run ID produced actual repeated `WaitingForMatrix`/no-capture/retry-refusal
proof before fresh terminal classification. The failed logs and tooling receipt
remain archived; no empty/failed probe is credited.

After complete native logs and controlled receipts were verified, only the public
acceptance `main` advanced from `ced7b9b` to this source. Its four production
prerequisites passed all eleven jobs. No primary repository push occurred. The
native executor created an annotated tag only in the public test repository;
publication run **37346108219** passed all seven jobs and native closeout completed.

## Actual final-source prerequisites and producing acceptance

Public test repository: **`99percentgrip/agent-vesper-rrc-public-acceptance`**.
The production repository and Alex's installed application were not mutated.

| Gate | Exact-source run | Terminal evidence |
|---|---|---|
| Canonical | 37339588552 | Quality and supply-chain successful |
| MSRV | 37339588477 | Rust 1.88 successful |
| Foundation | 37339588698 | All five native targets successful |
| Web driver | 37339588839 | Both contained browser images and native namespace/Hive successful |
| Publication | [37346108219](https://github.com/99percentgrip/agent-vesper-rrc-public-acceptance/actions/runs/37346108219) | Exact-commit gate, five builds and publication: all seven successful |

The actual strict helper exited zero and returned `driver-run=37339588839` only
when every prerequisite job was complete green. Its earlier while-running refusal
is retained. All four full prerequisite logs and the complete publication log are
archived with byte counts/digests; failed read attempts, if any, are not credited.

The production-linked native probe began from an independently observed,
externally prepared local/remote candidate. It did not claim to perform that
candidate's version bump or initial commit/push; those native stages are proven
separately by real temporary-repository/tool/worktree fixtures. The probe held an
exclusive ledger owner and followed the production resume transition before
fresh exact-SHA evidence refresh. The initial probe's direct already-green
refresh correctly failed before mutation; the corrected helper and both receipts
are retained, without modifying the frozen production runtime.

Actual native progression:

1. `observe`: four complete gates → `RemoteGatesGreen`.
2. `tag`: fresh evidence, persisted operation journal and native executor →
   annotated `v0.24.4`, tag object `b5e47769feb7fa060af4667e73f02825534f0e81`,
   targeting `bd24bb48765d73c704fb81945ca87e33d093e121`; native push → `Publishing`.
3. `poll`: actual successful producing workflow, annotated target, non-draft and
   non-prerelease metadata, all required nonempty assets and actual downloaded
   checksum content → `Published`. Release ID **403959731** has **16 assets**:
   five host archives, five host checksums, two driver archives, two driver
   checksums and two image-identity files. All 14 mandatory assets are present.
4. `closeout`: immutable publication retained → `WaitingForMatrix`.
5. `poll`: fresh current-main identity and all four complete gates → `Complete`.

The seven actual downloaded checksum files match both their archive server
SHA-256 digests and their own downloaded-byte/server digests. Verification does
not claim a local installation or a whole-archive download. No clobber, retag,
blind workflow retry, production publication or Registry write occurred.

Reproducible authorized test-repository commands:

```sh
python3 scripts/release_gate.py --repository 99percentgrip/agent-vesper-rrc-public-acceptance --commit bd24bb48765d73c704fb81945ca87e33d093e121
/tmp/rrc-parity-native-public-producer /tmp/rrc-public-validation-u97tsy8b bd24bb48765d73c704fb81945ca87e33d093e121 observe /tmp/rrc-public-producing-ledger
/tmp/rrc-parity-native-public-producer /tmp/rrc-public-validation-u97tsy8b bd24bb48765d73c704fb81945ca87e33d093e121 tag /tmp/rrc-public-producing-ledger
/tmp/rrc-parity-native-public-producer /tmp/rrc-public-validation-u97tsy8b bd24bb48765d73c704fb81945ca87e33d093e121 poll /tmp/rrc-public-producing-ledger
/tmp/rrc-parity-native-public-producer /tmp/rrc-public-validation-u97tsy8b bd24bb48765d73c704fb81945ca87e33d093e121 closeout /tmp/rrc-public-producing-ledger
/tmp/rrc-parity-native-public-producer /tmp/rrc-public-validation-u97tsy8b bd24bb48765d73c704fb81945ca87e33d093e121 poll /tmp/rrc-public-producing-ledger
```

These commands record the existing run; repeating `observe` refuses to overwrite
its epoch and repeating `tag` cannot recreate an immutable tag. Probe source,
compiled-library identity, command exits and corrections are archived.

Exact receipts: `rrc-parity-startup-main-settled.json`,
`rrc-parity-startup-main-complete-log-index.json`,
`rrc-parity-startup-prerequisite-final.json`,
`rrc-parity-startup-producing-tag-object.json`,
`rrc-parity-startup-publication-settled.json`,
`rrc-parity-startup-publication-release.json`,
`rrc-parity-startup-publication-log-index.json`,
`rrc-parity-startup-producing-checksums.json`,
`rrc-parity-startup-producing-acceptance.json` and the complete
`rrc-parity-startup-producing-ledger.json`.

The first ledger-copy assertion used a nonexistent `status` field and failed
before copying; the corrected check uses real `run_state`/job `state` fields.
It changes no runtime state and is retained as instrumentation failure.

## Acceptance traceability

[The section trace](release-recovery-controller-parity-continuation-requirements.json)
pins every binding section and names its actual current-source cases. All 82
names were verified in each of the five direct native job logs; this mapping
is supplemented by the actual producing evidence below.

The mandatory specification is sections 1–36. AC-16 explicitly requires multiple
provider fixtures; section 30 calls for mocked outage tests and controlled real
GitHub workflows. This continuation corrects the earlier status paragraph's
additional live-model requirement without removing any original requirement.
Live provider judgment, a real outage and Alex's browser/account completion are
unexecuted robustness checks and are never inferred from fixtures.

| Criterion | Current scope-appropriate evidence |
|---|---|
| AC-01 | `partial_matrix_blocks_retry`, actual earlier partial-matrix adapter probe; current native TUI/ACP partial ledger observation |
| AC-02 | Causal parser/compiler/panic/cache/ANSI cases; current actual unavailable-log GitHub annotation probe |
| AC-03 | Repeated fingerprint/no-state-change refusal; actual controlled repeated-red probe retained |
| AC-04 | Typed hypothesis/proof admission plus combined isolated real tools/native proof/promotion with both fixture factories |
| AC-05 | Final-source five-target run `37337247051` passed; direct logs include three Windows cancellations and both Linux workspace/default-doc/69-case gates before the new full matrix |
| AC-06 | Persisted retry/focused-family budgets; combined AC-23 refuses a speculative second full retry |
| AC-07 | Changed fingerprint fixture and composed macOS-to-Windows new diagnosis |
| AC-08 | Exact immutable last-green adapter comparison and missing/skipped-lane refusal |
| AC-09 | Deterministic panic cannot become outage even with official/community red inputs |
| AC-10 | Official degradation requires infrastructure causes and repository/source exclusions |
| AC-11 | Community-only outage refusal |
| AC-12 | Missing source exclusion/mixed health remains unconfirmed |
| AC-13 | Persisted paused epoch reopens with exact identity and fresh remote evidence |
| AC-14 | Composed published/docs-red/repair/different-platform-red preserves version, release SHA, tag object, assets and publication run |
| AC-15 | Rebuilt real TUI/ACP processes display Published separately from degraded main |
| AC-16 | Both registered fixture factories execute real tools, native failing/passing tests, isolated proof and promotion; no concrete provider in core |
| AC-17 | Both real hosts observe one ledger and ACP cancellation is visible in TUI |
| AC-18 | Stale SHA/provenance/run-attempt cases cannot advance a newer candidate |
| AC-19 | All ledger strings redact credential canaries; actual controlled log probe and missing-log annotation canary fixture |
| AC-20 | Native terminate/reap/restart fixture retains epoch/run/job identity; final-source focused native matrix `37337247051` passed; direct case/run/job receipts retained |
| AC-21 | Native process-tree cancellation and cross-host checkpoint watcher; final-source focused native matrix `37337247051` passed; direct case/run/job receipts retained |
| AC-22 | Six stagnant actions stop progress; passive CI waits do not consume active budget |
| AC-23 | `published_docs_red_repair_then_other_platform_red_stops_speculation`: complete composed sequence, persisted publication archive and exhausted-budget refusal |

Every row maps to named cases observed on all five final-source native targets
in the companion trace. Earlier-source native checks remain historical evidence.
Section 32 is additionally closed by the actual producing workflow/tag/assets
and native closeout below.

## Requirements beyond the AC list

| PRD sections | Implementation and current evidence |
|---|---|
| 1–6, 35–36 | Deterministic directive/reducer; typed hypotheses, failed-proof and repeated-cause refusal; AC-01–23 below define the required product behavior. |
| 7–9 | Shared harness ownership; both native hosts compose the same worker; legal/illegal transitions, transactional rejected events, immutable source/version/tag provenance and monotonic persisted transition records. |
| 10 | Causal parser/classifier, last-green comparison, confidence gates; tentative/unknown cannot authorize source repair. |
| 11–12 | Actual controlled failed-plus-running observation; exact workflow-ID/main-push/attempt correlation, bounded pagination and polling; job/run/failed-job-only writes require admission. |
| 13–16 | Stable normalized fingerprints, persisted budgets, native red/green focused proof, isolated worktree and one-patch promotion, source-change invalidation of old push receipts; no speculative mutation. |
| 17–19 | Official/intrinsic infrastructure late admission with source exclusions, community-only/mixed-status refusal, bounded health request, paused checkpoint refresh, six-action/20-minute active watchdog and passive wait separation. |
| 20–22 | Composed immutable Published/docs-red/repair/different-platform-red sequence; real TUI/ACP status/evidence/retry/cancel observation and shared slash-command registration. |
| 23–24, 26–28 | Vesper-owned isolated ledger roots, secret/control redaction, immutable causal history, exclusive cross-process owner, stale writer refusal, policy/firewall denial, terminate/reap/restart and shared read cancellation. |
| 25, 34 | Repair context begins with captured repository/log evidence; reports separate actual native/CI receipts, primary Debian sources and fixture/inference limitations. |
| 29–31 | All 23 acceptance rows plus 82 current RRC regressions, two typed-repair integration cases, controlled real GitHub and real host processes. |
| 32 | Final-source five-native and controlled protocol gates passed. Exact-SHA Canonical `37339588552`, MSRV `37339588477`, foundation `37339588698` and web-driver `37339588839` passed every job. Native annotated tag `v0.24.4` targets the final source; producing workflow `37346108219` passed all seven jobs, all 16 assets were verified and native closeout reached `Complete`. |
| 33 | Persisted epoch/transition/retry/repair/pause/escalation history plus measured CI wait/model activity and actual rejected/repeated/prevented retry counters; legacy defaults tested. |

## Earlier native failures and cancellation audit

- Candidate `7d038da2384243f40b3182c3b29251e1039666f3`, run `37287586345`: Linux x86/ARM and both macOS lanes passed; Windows passed RRC, typed repair and command settlement but failed the voice child identity guard because canonical paths have a `\\?\` prefix. Canonicalize both comparison operands; retain the failed run.
- Audit found background GitHub/status and native publication reads using independent false cancellation flags. Bound adapters now retain the worker token; native reads use the executor token. A mandatory offline regression flips the shared token after construction and rejects all six reads before process/network dispatch.

- PRD §12 failed-job-only rerun capability now exists on the shared evidence port; consumed full/infrastructure admission must own the exact run. Targeted tokens, foreign run IDs and unsafe repositories fail before dispatch; worker cancellation also stops admitted calls.

## Files and DOX

The closeout rechecked all changed paths. Nearest runtime, fixture, CI, scripts,
xtask and documentation contracts were updated; the parent documentation owner
now names controlled producing acceptance. No child boundary/index changed.
Root, application/crate parents, provider adapters, Registry and installers were
intentionally left unchanged at final delivery because these documentation
results do not change their existing ownership or operating contracts. Binding
PRD sections 1–36 retain their exact frozen bytes; only section 37 records status.
Static document review validates local links, JSON and primary HEAD invariance.
No program suite was rerun for final prose-only changes.

Production core/executor, publishing workflow, canonical acceptance steps, fixed xtask cases, read-only prerequisite helper and its tests, native host fixture, and nearest harness/apps/scripts/CI/xtask/documentation contracts; CI artifact profiles, exact browser package pin and its owning contract, native command fixture and agent contract, and hermetic voice fixture. No domain boundary moved. No new child boundary is needed; the root scripts index now includes the release prerequisite helper.

## Deviations and remaining work

- Foundation verification uses synthetic credentials/providers and isolated roots. Real model effectiveness and interactive account entitlement are not inferred from fixture streams.
- All mandatory producing evidence is complete on final source `bd24bb4`; older sources remain historical. Standard public runners required no billing/payment change.
- Application parent, registry, installers and provider-adapter contracts remain unchanged because their ownership/behavior did not change. Root scripts ownership and nearest fixture/runtime contracts are updated.
- No production `main` push, production tag/release, Registry update or installed-application replacement has occurred. The native executor created and verified a tag/release only in the authorized public acceptance repository.

## Readiness effect

All mandatory requirements in binding PRD sections 1–36 now have current,
scope-appropriate evidence on source `bd24bb48765d73c704fb81945ca87e33d093e121`.
All nine local checks and ten ACP repetitions passed; five native targets passed
with nonzero named cases; actual partial/repeated-red/green GitHub probes passed;
all four exact-source main prerequisites passed all eleven jobs; the unchanged
producing workflow passed all seven jobs and produced verified immutable assets.
Native post-publication main-health closeout reached `Complete` with publication
identity preserved. The section trace covers 36 sections and all 23 AC rows;
its named-case mapping and actual producing receipts support this verdict,
rather than counts or plan checkmarks alone.

Using a private test repository was an avoidable validation mistake. The separate
public route completed without payment/account changes. Production `main`, tags,
Registry and the installed application remain unchanged; the complete repairs
are in the scoped primary workspace. No installation was run.

The observed installed-ORT detached-worker finalization race remains outside the
RRC repair: hermetic fixture isolation is verified; production voice shutdown is
not claimed repaired. Interactive OpenAI browser/account completion, live-model
repair effectiveness and a real external outage remain unexecuted optional
robustness checks. These are not inferred from fixture or CI success.
