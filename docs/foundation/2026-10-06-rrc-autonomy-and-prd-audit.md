# RRC autonomy and PRD audit

## Objective and status

Audit all binding sections 1–36 and AC-01–AC-23 of the owning RRC PRD, repair confirmed gaps in autonomous recovery and completion, preserve provider neutrality and both-host behavior, then release the verified repair as the next unused patch. Alex authorized this expanded work after the successful immutable v0.24.6 closeout. Worktree: `/tmp/vesper-rrc-closeout-fix`, branch `repair/rrc-automatic-closeout`, base `a56f0ba76525bf4e7e288eefe2dff731cb120914`.

Current status: confirmed repairs and expanded local regression gates passed; native release gates are pending. Hosted gates, new publication and final delivery remain pending. This report does not claim that every possible bug has been eliminated or certify full current PRD parity before the required evidence exists.

## Methods and confirmed gaps

- Read the binding PRD, owning DOX chains, current controller/worker/admission/host paths and historical requirement/evidence traces. Historical passing runs do not certify this changed source.
- Reproduced the publication-terminal failure, then repaired shared current-main closeout, existing Registry PR delivery, local execution reporting, ACP owner retention and TUI final receipt projection. The companion [closeout report](2026-10-06-rrc-automatic-closeout-repair.md) and [actual v0.24.6 receipt](release-v0.24.6-closeout.md) retain exact evidence.
- Reproduced PRD 12.2 polling divergence: the literal regression observed 5 seconds instead of 20 seconds (`/tmp/vesper-rrc-poll-red.log`, one assertion failure). Production intervals now use 20/40/80/120 seconds and reset after matrix changes.
- Separated passive CI waiting from active repair stagnation. Known queued/running workflows receive a bounded two-hour unchanged-state watch, allowing the existing 60-minute platform workflow to settle. Missing workflow runs retain a 20-minute evidence window; stale running rows cannot extend it. No retry budget is consumed while waiting.
- Added a semantic repair watchdog: unique actual successful tool observations reset progress; repeated or failed actions accumulate toward six, and streamed text/status cannot reset the 20-minute no-evidence window. Call IDs and command durations do not create evidence. The real repair registry applies the check to both fixture provider contexts; the controller checks again at turn settlement so a fast final response cannot bypass it. Governed focused compilation retains its separate native subprocess watchdog. Hash storage is bounded to 4096 observations.
- Persisted every admitted causal-family repair before model dispatch. Early model failure or interruption now consumes the same bounded attempt allowance; a fresh process cannot reset it. Existing observed attempts provide the legacy counter floor, new later-main epochs receive their own budget, and prior disproven hypotheses enter subsequent repair prompts as untrusted evidence.
- Bound completed-target admission to the same objective. An old published version cannot certify unrelated new work or trigger republication.
- Later-main recovery clears the old completion receipt, creates a distinct full-SHA report and links that exact report from the evidence index and PRD. Native reports retain failures, focused proof/attempts, reserved repair admissions, retry budgets, transitions, metrics and changed paths. Publication reports and tags remain immutable.
- Corrected the controlled public fixture observation window from 30 to 90 seconds after a real partial-matrix observation was missed. The test repository uses an isolated branch and production receives the same fixture/DOX change in this code candidate; neither production main nor its release workflow is deliberately failed.
- The semantic deadline fixture injects elapsed duration instead of subtracting 20 minutes from the platform monotonic clock, avoiding underflow on recently booted native runners. Production still measures actual monotonic inactivity.
- Enrolled the new shared, TUI and Linux ACP regressions in `cargo xtask acceptance`, which rejects deleted, ignored and zero-match cases. Existing acceptance coverage is retained.

## Requirement trace and evidence

The companion `2026-10-06-rrc-autonomy-and-prd-audit-requirements.json` maps all 36 binding sections and 23 acceptance criteria to named current cases. It records current execution results separately from historical public/production receipts and pending hosted evidence. Fixtures with two registered provider identities prove the provider-neutral composition path, not live-model debugging effectiveness or account entitlement.

Sequential local commands use the existing candidate target cache, `CARGO_BUILD_JOBS=1` and `RUST_TEST_THREADS=1`:

- `cargo test -p vesper-harness --lib --all-features`: 369 passed, zero failed, six ignored; log `/tmp/vesper-rrc-audit-harness-final.log`. The portable monotonic-clock case also passed in an exact follow-up.
- `cargo xtask verify`: the earlier attempt failed the later-main link regression; the repaired full canonical gate must run through native RRC before candidate publication.
- `cargo xtask acceptance`: all 118 exact cases passed; log `/tmp/vesper-rrc-audit-acceptance-final.log`.
- Changed harness, ACP, TUI and xtask Clippy passed with all targets/features and warnings denied; `/tmp/vesper-rrc-audit-clippy-final.log`.
- Fresh controlled public runs on isolated SHA `897de26bd175dbb12a8fe5db26e603e5a0af721f`: red run 37487548629 exposed one failed/four active jobs and withheld diagnosis until settlement; repeated red run 37487842976 preserved fingerprint `3c6137dc79a06f05ae5269a436ef60372b2410631d7db1c131f3e13ed9e424dc` and refused an unchanged retry; green run 37488147422 settled five successful jobs. `/tmp/vesper-rrc-public-audit/orchestration.log` retains the native-reader observations.
- Before the expanded audit changes, the closeout candidate passed 361 harness cases (6 ignored), 5 ACP process cases, changed-package Clippy and all 100 then-enrolled acceptance cases. These are scoped intermediate receipts, not evidence for the later changes.

## Files and DOX pass

Shared controller, executor and new closeout module; harness manifest/lockfile; ACP prompt ownership and process tests; TUI receipt projection; exact acceptance registration; root, harness, ACP, TUI, xtask and foundation contracts; owning PRD, evidence index and release-objective provenance.

Root records the durable autonomous/provider-neutral repair preference. Nearest contracts describe their new workflows and checks. Parent crate/app/documentation ownership and child indices remain unchanged because no new boundary is introduced. Reports and owning documentation accompany the code candidate before release CI. Post-publication receipts remain local until the next authorized code push.

## Deviations and unresolved boundaries

- The first full workspace verification was intentionally interrupted after Clippy passed and while workspace test compilation was active, because the audit found the additional repair-dispatch budget gap. `/tmp/vesper-rrc-audit-verify-interrupted.log` remains an incomplete receipt; the final gate must run on the completed source.
- The expanded acceptance gate caught a second later-main reporting defect: the duplicate-link guard still checked the original publication filename, suppressing the new main report link. The executed assertion failed with zero matching links (`/tmp/vesper-rrc-audit-verify-report-link-red.log`); the guard now checks the actual Markdown target. That workspace attempt is a failed receipt, not a completed canonical gate.
- Two new fixture compilations initially failed (incorrect record field and missing ToolCall extensions); corrected before executed green results. Compile failures are not regression or mutation evidence.
- Unknown causes, exhausted persisted budgets, missing permissions and uncertain irreversible operations still stop with truthful evidence. Autonomous repair cannot safely replay an operation whose result is unproven. Status/activity alone never turns those states into success.
- Fresh controlled public protocol receipts passed; current-source native platform and producing receipts remain pending.
- No separate delegated reviewer has been launched in this audit. No live provider call or user-state write belongs to foundation verification.
- The release remains unpublished until native RRC passes its local gates, all exact-main prerequisite matrices, immutable tag/publication verification, existing Registry PR update and final receipt. Alex’s local installation is unchanged.

## Readiness effect

The repairs remove confirmed premature completion, false receipt reuse, excessive polling and repair/CI stagnation defects. Release readiness depends on the pending current-source and hosted gates; full PRD evidence remains explicit rather than inferred from another agent’s assurance.

## Archived local receipts

[Local and public protocol receipts](2026-10-06-rrc-autonomy-and-prd-audit-receipts.tar.gz), SHA-256 43ec5b9166900bd43f1eae6217730fcdb4a8a2c900a95812735e2d7869afbca4.

The prior epoch was Complete/Idle without an in-flight operation or owner. Scoped Cargo cleaning of only its harness/ACP/TUI products preserved the dependency cache and increased observed free disk from 124 GiB to 155 GiB; native reserve thresholds remain unchanged.
