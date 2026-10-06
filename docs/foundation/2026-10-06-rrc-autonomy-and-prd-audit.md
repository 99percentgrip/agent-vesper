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
- `cargo xtask acceptance`: all 122 exact cases passed after continuation and terminal-diagnostic regressions; log `/tmp/vesper-rrc-segments-acceptance.log`.
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

[Local and public protocol receipts](2026-10-06-rrc-autonomy-and-prd-audit-receipts.tar.gz), SHA-256 63df39feb245247aecdc31708f93447509baa59f26537c3b490e49724043a52b.

The prior epoch was Complete/Idle without an in-flight operation or owner. Scoped Cargo cleaning of only its harness/ACP/TUI products preserved the dependency cache and increased observed free disk from 124 GiB to 155 GiB; native reserve thresholds remain unchanged.

## Native candidate observation and additional progress repair

The first connected native ACP run on source 2c42ae69 bumped its isolated worktree to 0.24.7 and passed workspace verify, 118-case acceptance and architecture after version preparation. It recovered from resource pressure without a continue prompt. Before publication, live observation exposed a stale host projection: persisted gate-start milestones became visible only after the blocking gate returned. The active snapshot retained its earlier progress tree. Native cancellation stopped this candidate with commit/push/tag/publication flags false; its checkpoints and passing gate receipts remain preserved. This is an interrupted candidate, not a released result. The shared snapshot now projects current same-epoch persisted progress while retaining trusted acceptance-case units; mismatched epochs cannot overwrite a live snapshot. Both hosts already consume this shared snapshot. The added exact regression is enrolled in acceptance. Final-source native release gates remain required.

The expanded progress candidate passed 119 exact acceptance cases (202732 ms). Same-epoch projection and final-case/outer-gate separation passed the focused regression; changed-package Clippy and rebuilt ACP receipts are retained with the final source. Native release gates remain pending.


## Settled production matrix and native repair boundary

Exact candidate `8c3263f3adbf223660f37b9f54f9980df93cd827` passed all eight native local gates. Hosted runs 37500934609 (foundation), 37500934696 (canonical), 37500934692 (MSRV) and 37500934654 (web-driver) all settled before diagnosis. Windows, both Linux targets, Apple Silicon, hosted MSRV, web-driver and supply-chain passed. Intel macOS OpenAI startup and canonical microphone-free voice lifecycle failed. Native RRC captured fingerprints `858a590bd4e234422293644c9673206b4a8b8cacb95ad25fa19225b8fcf488ff` and `4c1f54953946a0322fe3dd1bcca6fa7efd6c7367cb7ab457d53ed21dcaac69c0`, reserved one repair admission for each family and created an isolated worktree. It refused publication when the worker returned a non-completed outcome; the old message discarded the outcome variant, so the exact first live terminal reason cannot be reconstructed and is not asserted here.

Inspection reproduced the unplanned 24-turn segment boundary without continuation. Repair continuation now preserves complete history, ownership, tool restrictions, permission and watchdog state within the same admission for at most four segments. A role ceiling also bounds native planned continuation, including a plan introduced after an earlier segment. Exhaustion and interruption remain explicit failures. Interrupted tool fragments never replay. Two synthetic providers demonstrate completion after the first segment, while hard-limit and native-plan fixtures prove bounded refusal.

The first live worker's partial, unverified startup/voice fixture patch is preserved and adopted for focused verification, not counted as a completed native repair. Cursor query replies no longer replay from the full transcript; cold startup matches the other cases' existing 20-second budget while discovery stays stalled. Voice checks wait for actual frames instead of inspecting before rendering settles. Final-source gates and publication remain pending; the red candidate is not tagged.

## Current continuation and UI focused proof

The final continuation source passed changed-package all-target/all-feature Clippy with warnings denied and 122 exact acceptance cases (228822 ms). Explicit role-ceiling and two-provider preserved-history continuation regressions passed. All three OpenAI startup process cases passed with all features. The ignored microphone-free lifecycle wrapper was explicitly executed with the task-owned NumPy 2.5.3 Python environment: one passed, covering mouse/F5, elapsed recording, editable multi-chunk transcription, retry/discard, early exit, disk failure and shutdown cleanup. Default ignored status is not counted as passing proof. No microphone or provider inference was used by these focused checks.

The initial wrapper invocation lacked its required integration feature, and the system Python lacked NumPy; neither is source regression proof. The corrected all-feature invocation and task-owned verification environment passed. Native final-source local gates and hosted matrices are still required. No release tag exists for the red 8c3263f3 candidate.

## Corrected candidate matrix and scheduler/request boundary

Exact source `80c3960db79d076e66eda763a6a3974e5e973c0d` passed all eight native local gates. Runs 37514876488, 37514876411, 37514876440 and 37514876453 settled completely: canonical quality (including the repaired voice fixture), MSRV, both Linux targets, Windows with prepared conformance, Intel macOS and all web-driver jobs passed. Apple Silicon failed `host_resources::tests::expensive_slot_serializes_runnable_gates` at reacquisition after lease drop. Native RRC waited for all 11 jobs, captured fingerprint `d3eb02288c11fb7e8b5367c5fa27c135ada00d6ebb0f754dcdefb7d7157f0f7c` and reserved one isolated repair admission. The worker returned the safe message `provider turn failed: OpenAI service request failed`; the old conversion discarded HTTP/category metadata, so no specific HTTP status, billing cause or vendor outage is asserted. No tag or publication was created, and no full gate retry was spent.

The lease regression reproduces the retained open-file-description mechanism deterministically with a duplicate descriptor: old source failed the reacquisition assertion, explicit unlock on lease drop passed both this case and the original serialization case. Owned children still settle before admission release.

The shared factory admits one retry of an initial adapter-approved request only before output, tools or compaction; it preserves the same admission and original history, counts the request toward the role ceiling, waits at least two seconds, honors typed server delay up to 30 seconds, and responds to cancellation. Longer delay, Never, repeated failure, executed action and ambiguous tool fragment all refuse restart. The two-provider fixture checks completion, bounded repeat refusal, original request equality, cancellation, long delay, fragment refusal and preserved prior writes. Typed terminal diagnostics now preserve safe HTTP/category/retry/delay values.

Official [OpenAI error guidance](https://developers.openai.com/api/docs/guides/error-codes) documents retry after a brief wait for 500 and server delay for 503; [rate-limit guidance](https://developers.openai.com/api/docs/guides/rate-limits) requires bounded attempts and honoring valid Retry-After without shortening long delays. The native adapter supplies BeforeVisibleOutput metadata only for pre-stream HTTP 500/503, parses numeric and HTTP-date delays, and refuses malformed/overflow headers. Authentication/payment/quota errors remain Never. An offline both-mode regression failed on old adapter code and passes on the repair. Subscription evidence remains the native protocol boundary, not a claim of a stable third-party subscription API. The first test command matched zero cases because its exact module path was omitted; only the corrected one-case assertion failure is red proof.

Final-source local/hosted/producing receipts remain pending for these additional repairs. Reports, PRDs and owning DOX accompany the corrected source before another exact candidate push; target remains 0.24.7.

Current lease/request source passed both scheduler cases, the complete bounded retry fixture, all 66 OpenAI adapter tests, changed-package all-target/all-feature Clippy and all 126 exact acceptance cases (194782 ms). ACP rebuilt successfully. Disabling only the initial retry path caused the named fixture's executed assertion failure; restoring the exact original source SHA-256 `e78480e0cdb2c42b5142482fc5904b8a95a75e26a01316708b54648f8103f6ac` restored green. The initial Clippy attempt rejected a test-module placement; it was moved before the successful final run. Neither the failed lint nor the zero-match command is passing evidence.
