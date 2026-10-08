# Agent Vesper — Release Recovery Controller PRD

**Document status:** Implementation-ready draft
**Priority:** P0 reliability / autonomy
**Working name:** Release Recovery Controller (RRC)
**Scope:** Agent Vesper release and CI-recovery workflow
**Primary hosts:** TUI and ACP
**Applies to:** `/release`, release-oriented workflows, CI-fix/recovery flows, post-release main-health closeout
**Provider policy:** Provider-neutral; no provider-specific release logic in core
**VRO-19:** Unrelated; remains on hold

---

## 1. Executive Summary

Agent Vesper can build, test, tag, and publish software, but its current release behavior is still primarily an **agent prompt executed through the normal AgentLoop**. That is insufficient for reliable autonomous release engineering.

The demonstrated failure mode is expensive and destabilizing:

1. a release or post-release commit triggers CI;
2. one job fails while other jobs are still running;
3. the agent reacts before the complete matrix is known;
4. it edits and pushes a speculative repair;
5. the next CI run exposes a different failure;
6. the cycle repeats for hours;
7. model quota, CI time, repository history, and user attention are consumed;
8. a successfully published release can be conflated with a later red `main`.

The Release Recovery Controller introduces a **deterministic harness-level state machine** for release orchestration and CI recovery.

The central invariant is:

> **No release retry or repair push without new evidence or a relevant state change.**

A relevant state change is one of:

- source code changed in a way causally tied to the failure;
- workflow/configuration changed in a way causally tied to the failure;
- dependency/toolchain state intentionally changed;
- credential/permission state was repaired;
- an externally degraded service recovered;
- a previously incomplete workflow/job matrix reached a terminal state and produced new evidence.

Waiting, rerunning, or “trying again” without one of those changes is not a valid recovery action.

The controller must:

- wait for the diagnosable CI matrix;
- collect failed job logs;
- identify the first causal failure per failed job;
- fingerprint failures;
- classify source/test/platform/infrastructure/outage causes;
- prefer focused verification before another full matrix;
- enforce strict retry budgets;
- prevent repeated identical failure loops;
- distinguish published-release health from post-release `main` health;
- detect external GitHub degradation only as a late diagnostic branch;
- pause on confirmed infrastructure incidents instead of burning tokens;
- persist a resumable release checkpoint;
- expose clear release/recovery status to the user.

This is a reliability subsystem, not a skill prompt.

---

## 2. Incident Basis

### 2.1 Observed failure pattern

The v0.24.4 release itself completed successfully, but a later documentation closeout commit triggered CI and exposed a test failure. Subsequent attempts to repair `main` produced a sequence of new commits and different failures across Windows/macOS/test paths.

The important product failure was not that CI can fail. CI failures are expected.

The product failure was that the agent lacked a deterministic mechanism to answer:

- Is the published release already valid?
- Which current lifecycle state are we in?
- Are all jobs finished?
- What is the first causal failure?
- Is the failure reproducible?
- Did the last repair actually change the failure signature?
- Is a new full matrix justified?
- Is the problem internal or external?
- Should work pause rather than continue consuming quota?

### 2.2 Current architectural gap

In the current architecture, `/release` is represented as a workflow prompt and is ultimately executed as a normal AgentLoop turn. Existing release discipline is documented and tested, but orchestration decisions remain model-driven.

RRC must move the **control plane** for release progression and retry admission out of free-form model judgment and into typed, persisted, deterministic runtime state.

The model remains responsible for diagnosis and proposing/implementing repairs. It must not be responsible for deciding whether an evidence-free retry is allowed.

---

## 3. Goals

- **G-01 — Deterministic lifecycle:** explicit typed release/recovery states and legal transitions.
- **G-02 — Evidence before action:** every repair/retry cites workflow/job/step, first causal error, and repair hypothesis.
- **G-03 — Full-matrix awareness:** do not treat a partial matrix as the final evidence set.
- **G-04 — Anti-loop protection:** block repeated identical failures with no relevant state change.
- **G-05 — Token/CI protection:** stop non-progressing recovery loops.
- **G-06 — Focused verification:** use the smallest credible regression before another full matrix.
- **G-07 — Outage discrimination:** external health is a late branch, not an excuse.
- **G-08 — Resume instead of restart:** persist recovery checkpoints.
- **G-09 — Published release vs main health:** separate lifecycle states.
- **G-10 — Provider neutrality:** no provider branches in core RRC.
- **G-11 — Cross-host parity:** TUI and ACP observe the same underlying state.
- **G-12 — Honest uncertainty:** `Unknown` is valid and never authorizes blind retry.

---

## 4. Non-Goals

RRC v1 does **not**:

- replace GitHub Actions;
- replace Cargo/xtask tests;
- guarantee CI never flakes;
- auto-rewrite arbitrary GitHub workflows;
- infer outages solely from social/community reports;
- auto-purchase CI capacity or service plans;
- bypass repository permissions;
- move immutable release tags after publication;
- auto-rollback already published releases;
- make arbitrary source changes without normal tool/permission policy;
- eliminate human intervention for ambiguous destructive operations;
- treat a green local build as proof of cross-platform success.

---

## 5. Binding Product Principles

1. **Logs first, edits second.**
2. **First causal failure, not final exit code.**
3. **A retry requires a reason the outcome should differ.**
4. **Full matrices are release gates, not debuggers.**
5. **External outage is a late branch.**
6. **Release publication is an immutable lifecycle boundary.**
7. **No hidden busy state.**
8. **No release retry without new evidence or relevant state change.**

---

## 6. Terminology

### Release Candidate
An exact commit proposed for release.

### Gate
A required verification workflow, such as Canonical, MSRV, five-target foundation, web-driver/native-host, or release/package verification.

### Matrix
The complete job set for a required gate.

### Failure Fingerprint
A normalized identity for a failure:

```text
workflow
job
step
platform
normalized causal signature
relevant tool/test identity
```

Volatile values such as timestamps, temp paths, runner IDs, UUIDs, ports, and request IDs are removed.

### Relevant State Change
A change that can plausibly alter the next outcome.

### Repair Attempt
One evidence-backed code/configuration change targeting a specific fingerprint or causally linked fingerprint group.

### Recovery Epoch
The bounded sequence from one failure observation until green, pause, or escalation.

---

## 7. High-Level Architecture

```text
                    ┌─────────────────────────┐
 /release ─────────►│ Release Intent Resolver │
                    └────────────┬────────────┘
                                 │
                                 ▼
                    ┌─────────────────────────┐
                    │ Release Recovery        │
                    │ Controller              │
                    │ deterministic state     │
                    └────────────┬────────────┘
                                 │
              ┌──────────────────┼──────────────────┐
              │                  │                  │
              ▼                  ▼                  ▼
      GitHub Evidence      Verification       Agent Repair
      Adapter              Adapter            Port
      runs/jobs/logs       xtask/tests         AgentLoop
              │                  │                  │
              └──────────────────┼──────────────────┘
                                 ▼
                    ┌─────────────────────────┐
                    │ Persistent Release      │
                    │ Ledger / Checkpoint     │
                    └────────────┬────────────┘
                                 │
                       ┌─────────┴─────────┐
                       ▼                   ▼
                      TUI                 ACP
```

### 7.1 Ownership

RRC belongs in a provider-neutral crate or harness-owned service. It must not live in a provider adapter, a TUI-only module, an ACP-only module, or a seed skill as the sole authority.

### 7.2 Model role

The AgentLoop may:

- interpret logs;
- research documentation;
- propose a diagnosis;
- edit source;
- run focused tests;
- write evidence reports.

RRC alone decides whether:

- enough evidence exists to repair;
- another CI run is admissible;
- retry budget is exhausted;
- an outage pause is justified;
- release progression may continue.

---

## 8. Release State Machine

```text
Idle
  │
  ▼
Preparing
  │
  ▼
LocalVerification
  │
  ├── failure ──► DiagnosingLocalFailure
  │
  ▼
CandidateReady
  │
  ▼
RemoteGateRunning
  │
  ├── incomplete ─► WaitingForMatrix
  │
  ├── green ──────► RemoteGatesGreen
  │
  └── red ────────► CollectingFailureEvidence
                       │
                       ▼
                  ClassifyingFailure
                       │
        ┌──────────────┼───────────────┐
        ▼              ▼               ▼
    Repairable     ExternalBlocked   Unknown
        │              │               │
        ▼              ▼               ▼
 FocusedRepair     PausedExternal   NeedMoreEvidence
        │                              │
        ▼                              └──► Escalated
 FocusedVerification
        │
        ├── fail ─► DiagnosingRepair
        │
        ▼
 RetryAdmissible
        │
        ▼
 RemoteGateRunning
        │
        ▼
 RemoteGatesGreen
        │
        ▼
 Tagging
        │
        ▼
 Publishing
        │
        ▼
 Published
        │
        ▼
 PostReleaseCloseout
        │
        ├── green ─► Complete
        └── red ───► PostReleaseMainDegraded
```

`PostReleaseMainDegraded` must **not** transition back to `Publishing` for the already released version. It opens a new recovery epoch for `main`.

---

## 9. Typed States

At minimum:

```rust
enum ReleaseRecoveryState {
    Idle,
    Preparing,
    LocalVerification,
    DiagnosingLocalFailure,
    CandidateReady,
    RemoteGateRunning,
    WaitingForMatrix,
    CollectingFailureEvidence,
    ClassifyingFailure,
    NeedMoreEvidence,
    FocusedRepair,
    FocusedVerification,
    RetryAdmissible,
    ExternalHealthCheck,
    PausedExternal,
    Escalated,
    RemoteGatesGreen,
    Tagging,
    Publishing,
    Published,
    PostReleaseCloseout,
    PostReleaseMainDegraded,
    Complete,
    Cancelled,
}
```

Every transition records:

- monotonic sequence;
- UTC timestamp;
- source commit;
- workflow/run/job IDs where applicable;
- triggering reason;
- evidence references;
- repair/retry counters.

---

## 10. Failure Taxonomy

```rust
enum ReleaseFailureClass {
    SourceRegression,
    TestRegression,
    FlakyOrTimingSensitiveTest,
    CompileFailure,
    PlatformSpecificFailure,
    PackagingFailure,
    ReleaseMetadataFailure,
    WorkflowConfigurationFailure,
    DependencyFailure,
    CredentialOrPermissionFailure,
    RateLimit,
    RunnerInfrastructureFailure,
    ArtifactInfrastructureFailure,
    ExternalServiceOutage,
    Timeout,
    Cancelled,
    Unknown,
}
```

Each classification includes:

- supporting bounded log evidence;
- affected platforms;
- whether other platforms passed;
- whether the failure exists on the last known green release commit;
- whether a post-release change touched related source;
- confidence: `Proven`, `StronglySupported`, `Tentative`, or `Unknown`.

Tentative classification does not authorize destructive repair.

---

## 11. Complete-Matrix Rule

### RRC-11.1
When a required matrix has at least one failed job while others remain queued, waiting, or in progress, RRC enters `WaitingForMatrix`.

### RRC-11.2
RRC normally waits for all jobs in that gate to reach a terminal state.

### RRC-11.3 — Fatal exception
Read-only diagnosis may begin early for deterministic independent failures, but no source mutation, repair push, or rerun is admitted until the matrix is complete unless the user explicitly overrides.

### RRC-11.4
Source edits may be prepared in a private worktree while waiting, but promotion/push remains blocked.

---

## 12. GitHub Evidence Adapter

RRC must use structured GitHub state where APIs exist.

Required capabilities:

- list workflow runs for an exact `head_sha`;
- inspect run status/conclusion;
- list jobs and attempts;
- inspect steps;
- download/read job logs;
- record run attempt number;
- record workflow/job URLs;
- inspect artifacts where needed;
- rerun one job;
- rerun failed jobs;
- rerun a complete workflow only when policy permits.

GitHub supports run filtering by `head_sha`, job log retrieval, and targeted reruns. Re-runs use the original `GITHUB_SHA` and `GITHUB_REF`.

### 12.1 Permissions
Diagnosis requires Actions read access. Reruns require Actions write access plus normal Vesper permission enforcement.

### 12.2 Bounded polling
While jobs are active:

- refresh immediately once;
- normal minimum interval: 20 seconds;
- back off to 2 minutes for unchanged long-running jobs;
- reset interval on state change;
- never poll once per model step.

---

## 13. Failure Fingerprinting

Input:

- workflow;
- job;
- failed step;
- platform;
- test binary/command;
- panic/assertion identity;
- normalized compiler diagnostic;
- infrastructure/API class where relevant.

Normalization strips volatile values.

If:

```text
previous fingerprint == current fingerprint
AND
no relevant state change
```

then:

```text
Automatic retry = FORBIDDEN
```

If a repair produces the same fingerprint, the repair hypothesis is marked disproven or insufficient and RRC requires a new diagnosis.

---

## 14. Retry Budget

Default per recovery epoch:

- **0** automatic retries without a relevant state change;
- **1** full remote-gate retry after a focused verified repair;
- **1** infrastructure retry after confirmed service recovery;
- targeted single-job reruns may be used for bounded diagnostic confirmation.

After two evidence-backed repair attempts fail to resolve the same causal family:

```text
state = Escalated
```

User override may authorize another attempt but never erases history or counters.

GitHub's higher technical rerun limit is not Vesper's autonomous policy.

---

## 15. Focused Verification Rule

Before another expensive complete matrix, the repair must pass the smallest credible proof.

| Failure | Focused proof |
|---|---|
| Windows packaging path | Windows packaging/path fixture |
| macOS process cleanup | focused settlement regression |
| one Rust unit test | exact test repeated under relevant conditions |
| workflow YAML | workflow/schema validation |
| version metadata | consistency checker |
| archive composition | local package/extraction test |
| registry URL | bounded HEAD/GET checks |

A complete five-target matrix remains mandatory for final release readiness.

---

## 16. Repository Pollution Control

### 16.1 Recovery workspace
Use a dedicated worktree/branch where repository policy permits.

Example logical identity:

```text
release-recovery/<version-or-main>/<epoch-id>
```

### 16.2 No speculative main pushes
Do not push “maybe fix” commits directly to `main`.

### 16.3 Promotion
Promote only after:

- diagnosis is recorded;
- focused regression is red/green where applicable;
- relevant local checks are green;
- retry admission is granted.

### 16.4 Workflow trigger constraint
If required workflows only produce authoritative evidence from `main`, either add a safe workflow-dispatch/candidate-branch path or explicitly retain one-hypothesis/one-push discipline. RRC must not assume branch CI coverage exists.

---

## 17. External Outage Detection

External outage detection is intentionally a low-priority diagnostic branch.

### 17.1 Admission

Enter `ExternalHealthCheck` only if either:

**A. Repository-side exclusion**
- relevant local checks are green;
- source/configuration does not explain the failure;
- multiple unrelated jobs show infrastructure-like failures;

or

**B. Intrinsic infrastructure signature**
- runner provisioning failure;
- GitHub API/Actions service error;
- artifact/cache service failure;
- rate/service response clearly outside repository logic.

### 17.2 Evidence hierarchy

1. **GitHub official status**
2. direct GitHub API/Actions behavior relevant to the failure
3. cross-job/cross-platform infrastructure signatures
4. secondary community telemetry such as Downdetector, only as corroboration

### 17.3 Secondary-source rule
Downdetector or community reports alone must never classify an outage.

### 17.4 Health-check budget
Target:

```text
10–15 seconds wall-clock
```

with only a small bounded request set.

### 17.5 Confirmed outage

```text
state = PausedExternal
```

User-facing message:

```text
Release paused — external GitHub service degradation detected.
No repository defect has been established.
No retry is scheduled while the incident remains active.
```

### 17.6 Inconclusive health
Use:

```text
External status: UNCONFIRMED
```

Never fabricate “GitHub is down.”

---

## 18. External Pause and Resume

Persist:

- candidate SHA;
- branch/ref;
- required gates;
- run IDs;
- completed/failed jobs;
- fingerprints;
- outage evidence;
- remaining work;
- last health-check timestamp.

Resume refreshes GitHub state and continues from the checkpoint. v1 does not require endless background monitoring.

---

## 19. Token / Work Budget Watchdog

Every substantial recovery action must produce one of:

- new evidence;
- new diagnosis;
- focused test result;
- relevant source/config change;
- CI state change.

Stagnation warning triggers when either:

- 6 consecutive recovery actions produce no new evidence/state; or
- 20 minutes of active autonomous recovery produce no new evidence/state.

Repeated identical fingerprint + no relevant state change is an immediate hard stop regardless of time.

---

## 20. Published Release vs Post-Release Main Health

Once exact release gates are green, the annotated tag points to the verified commit, and assets are published/verified:

```text
ReleaseState = Published
```

A later documentation/evidence closeout commit belongs to a separate `PostReleaseMainHealth` lifecycle.

If that later commit is red:

- do not create another release automatically;
- do not move the release tag;
- do not republish assets;
- do not say the published release failed;
- open a new recovery epoch for `main`.

User-facing example:

```text
Release v0.24.4: PUBLISHED / VERIFIED
Current main: DEGRADED
Cause: macOS Intel post-release CI test failure
Release artifacts affected: NO EVIDENCE
```

---

## 21. User Experience

### Waiting for matrix

```text
RELEASE
State          Waiting for CI matrix
Commit         c8b9d68
Gate           five-target-foundation
Jobs           3/5 terminal
Failures       macOS Intel (1)
Retry          blocked — matrix incomplete
```

### Diagnosing

```text
RELEASE RECOVERY
State          Diagnosing
Failure        voice_speech_pipeline
Platform       macOS Intel
Fingerprint    8f2c…
Evidence       logs captured
Next           focused reproduction
```

### Paused external

```text
RELEASE RECOVERY
State          Paused — external service
Service        GitHub Actions
Repository     no defect established
Retry          none scheduled
Resume         after service recovery
```

### Escalated

```text
RELEASE RECOVERY
State          Needs intervention
Attempts       2/2
Same failure   yes
Automatic retry blocked
```

RRC-owned work must never collapse to an ambiguous `Working…` state.

---

## 22. Slash Command Integration

### `/release`
`/release patch|minor|major` becomes an RRC-start action rather than only a free-form workflow prompt. The model still receives structured release/repair tasks, but RRC owns lifecycle progression.

### `/ci`
Expose:

- current commit;
- active runs;
- terminal/in-progress jobs;
- fingerprints;
- RRC state;
- retry eligibility.

Suggested commands:

```text
/release status
/release resume
/release cancel
/release evidence
/release retry
```

`retry` does not bypass admission rules unless the user explicitly confirms an override.

ACP must expose equivalent semantics without depending on TUI state.

---

## 23. Persistence Model

Suggested shape:

```rust
struct ReleaseRecoveryRecord {
    schema_version: u32,
    repo_identity: String,
    epoch_id: String,
    objective: ReleaseObjective,
    state: ReleaseRecoveryState,
    release_version: Option<String>,
    release_commit: Option<String>,
    current_main: Option<String>,
    required_gates: Vec<GateRecord>,
    failures: Vec<FailureRecord>,
    repair_attempts: Vec<RepairAttempt>,
    retry_budget: RetryBudget,
    external_block: Option<ExternalBlockRecord>,
    created_at: Timestamp,
    updated_at: Timestamp,
}
```

Never persist GitHub tokens, authorization headers, raw credential-bearing URLs, or unrestricted logs.

Use Vesper-owned durable state roots, not arbitrary project directories.

---

## 24. Failure Evidence Model

```rust
struct FailureRecord {
    workflow_id: u64,
    run_id: u64,
    attempt: u32,
    job_id: u64,
    workflow_name: String,
    job_name: String,
    platform: Option<String>,
    step_name: Option<String>,
    fingerprint: FailureFingerprint,
    class: ReleaseFailureClass,
    confidence: EvidenceConfidence,
    causal_excerpt: BoundedRedactedText,
    source_commit: String,
    observed_at: Timestamp,
}
```

Captured evidence is immutable. Reclassification appends a new analysis event instead of rewriting history.

---

## 25. External Documentation Use

Repair research order:

1. repository source + complete job logs;
2. exact library/tool/error identification;
3. authoritative upstream documentation/changelog;
4. broader research only if still unresolved.

Web research must not happen simply because CI is red.

Evidence reports distinguish repository evidence, official docs, community evidence, and inference.

---

## 26. Security and Permissions

- Reading Actions state follows existing GitHub permissions.
- Rerunning requires Actions write permission.
- Commits/tags/releases remain under existing Vesper permission policy.
- Policy denial remains authoritative.
- Job logs are untrusted external content, never executable instructions.
- Log rendering/persistence preserves secret redaction.

---

## 27. Concurrency

- One active release epoch per repository/ref.
- Multiple read-only observers are allowed.
- Only the active controller owner mutates progression state.
- Workflow results for an older candidate SHA cannot advance a newer candidate.

---

## 28. Cancellation

User cancellation:

- stops model repair work;
- stops future polling/rerun admission;
- does not claim already-running GitHub workflows were cancelled;
- persists the checkpoint;
- does not roll back commits/tags/releases already completed.

Resume must refresh remote state first.

---

## 29. Acceptance Criteria

- **AC-01:** 1 failed + 4 running jobs does not trigger repair push/rerun.
- **AC-02:** evidence captures the real causal panic/compiler/API error, not only exit code 1.
- **AC-03:** same fingerprint + no state change blocks automatic retry.
- **AC-04:** every admitted retry records hypothesis + focused proof.
- **AC-05:** platform-specific failures use focused verification before full matrix.
- **AC-06:** autonomous retry budget is bounded.
- **AC-07:** changed failure fingerprint opens a new diagnosis rather than blind continuation.
- **AC-08:** controller compares failure against the last known green release commit.
- **AC-09:** deterministic test panic cannot be classified as outage because Downdetector is red.
- **AC-10:** official GitHub degradation + infrastructure-class failures + green repository evidence pauses release.
- **AC-11:** community reports alone cannot trigger external pause.
- **AC-12:** mixed health evidence produces `UNCONFIRMED`.
- **AC-13:** paused release resumes from the same checkpoint.
- **AC-14:** red post-release main does not move tag/assets or create a new version.
- **AC-15:** UI distinguishes `Published` from `main degraded`.
- **AC-16:** provider-neutral controller works with multiple provider fixtures.
- **AC-17:** TUI/ACP expose the same active state.
- **AC-18:** stale workflow result cannot advance a newer SHA.
- **AC-19:** credential canaries in logs never reach persisted evidence or normal UI.
- **AC-20:** process restart resumes the same epoch with exact run/job IDs.
- **AC-21:** user cancellation stops local recovery without false remote-cancel claim.
- **AC-22:** repeated no-progress checks trigger the stagnation watchdog.
- **AC-23:** fixture the demonstrated pattern `release green → docs closeout → CI red → repair commit → different platform failure`; RRC must keep release Published and prevent speculative loops.

---

## 30. Test Strategy

### Pure state-machine tests
Table-driven legal/illegal transitions.

### GitHub adapter fixtures
Cover queued, in-progress, success, failure, timed-out, cancelled, rate-limited, partial matrices, and multiple attempts.

### Log parser fixtures
Include Rust panic, compiler error, test assertion, packaging-path failure, GitHub API 5xx, runner provisioning failure, cache warning followed by unrelated actual failure, and secret canary.

### Fingerprint tests
Equivalent volatile logs map to the same fingerprint; different causes map differently.

### Retry admission
Exhaust combinations of same/different fingerprint, state change yes/no, focused proof pass/fail/not-run, budget available/exhausted.

### Outage tests
Mock GitHub official status, direct API behavior, infrastructure signatures, community telemetry, and deterministic repository failures.

### Process restart
Persist, terminate host, reopen, resume.

### Real GitHub acceptance
Use a controlled test repository/workflow for deterministic red/green cases. Do not intentionally break Agent Vesper `main` as the primary acceptance mechanism.

---

## 31. Implementation Plan

### PR-1 — Domain model + persisted ledger
Typed states, classes, fingerprints, retry policy, persistence, state-machine tests. No GitHub mutations.

### PR-2 — Read-only GitHub evidence adapter
Runs/jobs/steps/logs, exact-SHA correlation, matrix snapshots, bounded polling, redacted evidence.

### PR-3 — Diagnosis / retry admission
Fingerprint comparison, state-change rules, focused verification contract, retry budget, stagnation watchdog, escalation.

### PR-4 — Release integration
Move `/release` lifecycle authority to RRC while retaining AgentLoop for reasoning/repair. Integrate current exact-commit gates.

### PR-5 — External health and pause/resume
GitHub official status integration, bounded health check, optional secondary corroboration, `PausedExternal`, resume semantics.

### PR-6 — TUI + ACP surfaces
Status panel, commands, parity, restart/resume UX.

### PR-7 — Final audit + real acceptance
Simulated failure matrices, repeated-fingerprint block, focused repair → one retry, outage pause/resume, and published-release/post-main-red scenario.

---

## 32. Release Gate for RRC Itself

RRC is not complete until:

- state-machine coverage passes;
- retry loop is proven bounded;
- cross-host parity passes;
- no provider-specific branch exists in core;
- persistence/restart acceptance passes;
- controlled real GitHub workflow acceptance passes;
- outage classification cannot be triggered by secondary reports alone;
- current release workflow still completes successfully;
- no tag/publication regression exists;
- existing exact-commit release gates remain intact.

---

## 33. Metrics

Track locally:

- release epochs;
- recovery epochs;
- retries admitted/rejected;
- repeated fingerprints blocked;
- repair attempts per failure;
- time waiting for CI vs model-active diagnosis;
- external pauses;
- escalations;
- speculative reruns prevented.

Primary success metric:

> A deterministic CI failure should normally require one diagnosis, one focused repair verification, and at most one new full matrix.

---

## 34. Source / Research Basis

### Agent Vesper
Current source shows `/release` is a workflow command that becomes an AgentLoop prompt. Repository policy already requires exact-commit Canonical, MSRV, five-target, and web-driver gates before tagging.

Repository:
https://github.com/99percentgrip/agent-vesper

### GitHub Actions
GitHub supports:

- listing workflow runs and filtering by `head_sha`;
- job/run status;
- job logs;
- specific-job reruns;
- failed-job reruns;
- full workflow reruns.

GitHub documents that reruns preserve the original `GITHUB_SHA` and `GITHUB_REF`.

References:

- https://docs.github.com/en/rest/actions/workflow-runs
- https://docs.github.com/en/rest/actions/workflow-jobs
- https://docs.github.com/en/actions/how-tos/manage-workflow-runs/re-run-workflows-and-jobs
- https://docs.github.com/en/actions/how-tos/monitor-workflows/use-workflow-run-logs

### External service health
Primary:
- https://www.githubstatus.com/

Secondary corroboration only:
- https://downdetector.com/status/github/

---

## 35. Binding Product Decisions

1. No release retry without new evidence or a relevant state change.
2. Wait for the complete relevant CI matrix before repair admission.
3. Read logs before changing source.
4. Fingerprint failures and block identical no-progress retries.
5. Focused verification precedes another expensive full matrix.
6. Autonomous recovery retry budget is intentionally small.
7. Outage detection is a late diagnostic branch.
8. GitHub official status outranks community outage reports.
9. Community reports alone cannot justify an outage pause.
10. Confirmed external outage pauses work instead of consuming model quota.
11. Published release health and later `main` health are separate states.
12. RRC is harness-level and provider-neutral.
13. TUI and ACP share one release state.
14. Release state persists and is resumable.
15. Uncertainty is surfaced; it is not converted into blind retries.
16. A fresh natural-language release request reconciles persisted state before admission.
17. The same recoverable objective resumes automatically from its safe local stage.
18. An obsolete same-objective prerelease epoch is archived before replacement.
19. Remote push, tag or publication evidence is never silently discarded.
20. An unrelated active objective asks one bounded human-facing clarification.
21. Epoch IDs, internal controller states, ledger paths and manual resume/cancel
    commands are diagnostics, not normal user prerequisites.

---

## 36. Final Product Definition

The Release Recovery Controller succeeds when Agent Vesper behaves like a disciplined release engineer:

```text
prepare
→ verify locally
→ create exact candidate
→ run required CI
→ wait for complete evidence
→ if green, publish
→ if red, diagnose once
→ prove a focused repair
→ retry once
→ either ship or stop with exact evidence
```

If GitHub itself is degraded:

```text
prove repository-side readiness
→ verify external degradation
→ pause
→ preserve checkpoint
→ tell the user why
→ resume after recovery
```

It must never degrade into:

```text
red
→ guess
→ push
→ red
→ guess
→ push
→ red
→ repeat for hours
```

That behavior is the defect this PRD exists to eliminate.

A new ordinary-language release request also performs this admission reconciliation:

```text
load persisted epoch
→ same objective + safe local state: reconcile and continue automatically
→ same objective + obsolete source/worktree: archive old evidence and replace
→ remote push/tag/publication exists: preserve and ask only if a decision is needed
→ unrelated active objective: preserve and ask one bounded clarification
```

The primary checkout is not the reconciliation workspace and is never cleaned,
reset, stashed or mutated by this process.

---

## 37. Current Implementation Status

[Windows credential persistence and native RRC follow-up](foundation/2026-10-08-windows-credential-persistence-repair.md)
records the later post-login save defect, controlling-terminal fixture failure,
hidden repair dispatch error, stale boot-model repair configuration, falsely
running ownerless TUI header and managed-cache disappearance observation. The
first .11 matrix additionally found Windows acceptance-vector mutability and
macOS PTY request ABI compile errors; its automatic repair stopped after required
report edits invalidated its earlier check. The follow-up adds bounded same-history
proof continuation and fresh Windows cache metadata. These newly exposed gaps
passed 157 exact local cases, strict Clippy and eight Linux process checks;
the current candidate still requires
native exact-source gates, OS credential receipts and Complete/Idle publication.
Historical green releases do not certify these new behaviors.

[Native first-launch and RRC workspace repair](foundation/2026-10-08-native-first-launch-and-rrc-workspace-repair.md)
records a newly reproduced verification-scope defect: a shared cached `xtask`
validated its embedded old release worktree instead of the invoking candidate.
Both source and fixture roots now resolve from the invocation, with no build-path
fallback, and two real-process cases are mandatory acceptance. The five-target
native gate also exercises signed-out provider setup and TUI/ACP release controls
with every registered adapter selected. Natural-language native release admission
now bypasses pending account discovery through the shared classifier. Production
macOS/Windows governor backends replace the prior unavailable extension points;
current native platform, constrained-job and cancellable-observer evidence passed
for the corrected v0.24.10 candidate.
Earlier test counts alone do not establish this newly uncovered scope.
The first v0.24.10 exact matrix also exposed relative executable/Windows receipt
encoding faults and Unknown classification of typed Python runtime exceptions.
These are repaired with caller-path/UTF-8 fixture contracts and shared, priority-
preserving source diagnosis. Corrected commit `4426004d` passed all twelve exact-SHA
prerequisite jobs and all five native lanes, including the previously skipped
TUI/ACP RRC and real constrained-Windows-job checks. All 100 distinct mapped cases
plus eight recovery cases passed on every target. Native RRC published sixteen
assets, refreshed green main, updated Registry PR #539 in place and delivered its
final summary at Complete/Idle on 2026-10-08. This green release did not invoke a
live model repair; the provider-neutral repair contract retains its named
two-provider regression evidence and explicit unknown-failure refusal.


[Windows missing runtime package repair](foundation/2026-10-08-windows-missing-runtime-package-repair.md)
records the subsequent failed user-device install and unbundled runtime imports.
The corrective v0.24.9 epoch was cancelled before push/tag to incorporate that
repair. Prior green hosted checks did not certify a clean Windows laptop.

[Release v0.24.8 closeout](foundation/release-v0.24.8-closeout.md) binds the
integrated permission, promotion, resource admission, cancellation and Windows
installer changes to `9fada0cd06714ba1e754b332b5d44b39d3d9f373`: all eight native
local gates, twelve exact-SHA prerequisite jobs, producing publication and existing
Registry PR delivery passed. The native owner initially stopped on a read-only
publication request timeout and required an explicit recovery command.
[Publication observation recovery](foundation/2026-10-07-rrc-publication-observation-recovery.md)
tracks that new confirmed autonomy gap, matching tagged-request clarification
regression and formerly unbounded cumulative publication watch. These later fixes are not contained in immutable v0.24.8; current-source
verification and a separate corrective candidate remain required. Earlier completion
claims certify their recorded scope, not absence of subsequently observed defects.


[The further adversarial audit](foundation/2026-10-07-rrc-promotion-and-inventory-gap-audit.md) reproduced five additional defects after v0.24.7: proof-changing verification could promote different source, literal/new/renamed paths could block recovery, display-truncated status could admit an unowned staged file, an ignored new source input could pass proof yet be omitted from promotion, and whitespace validation checked the unstaged diff after staging. The shared controller repair shipped in v0.24.8; the published v0.24.7 receipts remain historical evidence for their exact source. Current-source verification and unexecuted hosted boundaries are recorded in that report.

[The expanded autonomy audit](foundation/2026-10-06-rrc-autonomy-and-prd-audit.md) tracks the subsequently authorized full requirement audit, new recovery/completion regressions and next patch release. The current v0.24.7 candidate passed all required hosted/producing gates and native closeout; the linked trace retains requirement-specific scope.

**The current 36-section/23-criterion requirement trace and acceptance evidence live in the expanded autonomy audit. Earlier v0.24.6 was published at
`a56f0ba76525bf4e7e288eefe2dff731cb120914`, with its assets verified, but the
controller initially stopped at Published before registry/report closeout and final user
notification. Native closeout now completed with an idle owner and final receipt;
[the v0.24.6 receipt](foundation/release-v0.24.6-closeout.md) records the unchanged
release and same registry PR update. [The automatic-closeout repair](foundation/2026-10-06-rrc-automatic-closeout-repair.md)
tracks the reproduced defects, current repair evidence and live closeout outcome.
The earlier [resource-recovery repair](foundation/2026-10-06-rrc-version-preparation-resource-repair.md)
remains evidence for the separate version-preparation defect. Published tags/assets
are immutable; a completed release does not establish all PRD requirements.**

[The production release report](foundation/2026-10-06-rrc-parity-production-release.md)
owns reconciliation with the newer production source, exact-source local verification,
all four complete hosted prerequisite matrices, actual publication and Registry delivery.
The binding sections above retain the current production bytes, including natural admission,
provenance reconciliation and explicit-version requirements. Their SHA-256 is
`e85011c696d769837c6ed5a83e4290e77eadc4ae27a9ea42cffe0966d37518db`.

[The completed public continuation](foundation/release-recovery-controller-parity-continuation.md)
certifies its frozen candidate `bd24bb48765d73c704fb81945ca87e33d093e121` against the
then-current binding scope. Its source manifest, requirement trace, controlled public
publication and archived receipts remain historical evidence; they cannot certify
changed production code. Earlier execution reports retain their original scope and
unexecuted items.

The release preserves the OpenAI device-sign-in URL/browser-launch repair together
with the later background startup discovery fix. Alex confirmed working OpenAI authentication in manual testing; that confirmation
does not establish deterministic release recovery acceptance.
Production publication used standard public GitHub Actions. Alex's installed
application is outside this release authorization.

[Historical requirement trace](foundation/2026-10-06-rrc-parity-production-release-final-requirements.json) binds the earlier production candidate; it cannot certify this changed source or the subsequently reproduced closeout defects.

[Post-release documentation/Windows repair](foundation/2026-10-06-documentation-ci-and-windows-readiness-repair.md) records the unnecessary separate documentation push, newly exposed command-fixture readiness/MCP startup failures and the same-commit documentation rule. This later main-health repair does not move the published tag or relabel cancelled checks as passed.

The expanded audit also repairs live host progress: same-epoch durable milestones
and gate settlement are projected while blocking commands run, and trusted exact
acceptance-case counts remain visible without certifying the outer gate early.
The [autonomy audit](foundation/2026-10-06-rrc-autonomy-and-prd-audit.md) retains
the interrupted native candidate and the regression evidence.

Repair workers preserve history across bounded ordinary iteration segments under
one persisted admission, with a role ceiling covering later native plans. Typed
terminal diagnostics distinguish exhaustion, interruption and acceptance failure.
The autonomy report preserves the settled red 0.24.7 candidate and focused UI
repairs; final-source hosted/publication evidence remains required.

The additional settled Apple Silicon scheduler failure has a red/green inherited
file-description regression and explicit lease unlock. Initial provider recovery
uses the adapter's typed retry decision under one bounded shared admission, never
replays output/tools, and retains safe terminal status. The autonomy audit records
127 exact local cases, stopped worker evidence and pending final-source CI.

Version-preparation source failures now enter ordinary autonomous local repair only
after verified baseline rollback and causal journal settlement. Promoted local
patches are digest-pinned before preparation; tampering refuses before version
writes. Default-feature and all-feature builds are checked independently. The
autonomy audit records the additional native discovery and executed mutation proof.

[Release v0.24.7 closeout evidence](foundation/release-v0.24.7-closeout.md) records this release; full PRD parity remains separately evidenced.

Current v0.24.7 evidence: 127 exact acceptance cases, eight native local gates,
11 exact-SHA prerequisite jobs and seven producing jobs passed. The native owner
automatically completed publication, current-main verification, the existing
Registry PR update, execution reporting and its final response; the ledger is
Complete/Idle with no in-flight operation. Live-model repair effectiveness remains
distinct from fixture coverage; no universal absence-of-bugs claim is made.

The [2026-10-07 reverification](foundation/2026-10-06-rrc-autonomy-and-prd-audit.md#requested-reverification-2026-10-07) found no remaining confirmed PRD implementation gap. All 100 named requirement cases passed in fresh execution, alongside 375 harness tests and 127 exact acceptance cases. This covers the PRD’s required bounded behavior; its explicit non-goals remain unchanged.

### 2026-10-07 authorization-stall follow-up

[Incident diagnosis and scoped repair](foundation/2026-10-07-rrc-authorization-stall-diagnosis.md) records an installed v0.24.7 ACP driver that does not answer permission requests and keeps polling a failed epoch. Shared typed authorization stops now preserve the actual cause and checkpoint without creating source failures. Focused local evidence does not certify installed-client recovery, full PRD parity or a new release.

### 2026-10-07 native permission and stop verification

[Native permission and stop repair](foundation/2026-10-07-rrc-native-permission-and-stop-repair.md) extends the diagnosis with real ACP protocol proofs of approval, rejection, cancellation, late approval isolation and exactly one terminal response. It also closes unoffered approval-option acceptance, old-request cancellation-hook cleanup races and stale TUI approval dialogs after their operation ends. The source includes the promotion/inventory repair; exact current-source local and hosted release receipts remain separately recorded.

### 2026-10-07 adaptive resource admission follow-up

[Adaptive resource admission repair](foundation/2026-10-07-rrc-adaptive-resource-admission-repair.md) reproduces a prolonged native wait caused by calculating a two-job near-full-swap envelope before reporting a one-job Cargo policy. Shared admission now selects a bounded lower concurrency only when every existing safety check passes for that same budget. Focused red/green and refusal tests are scoped evidence; final versioned native gates and hosted release receipts remain separately required.


[Native cancellation keybinding repair](foundation/2026-10-07-rrc-native-cancellation-keybinding-repair.md) closes the TUI gap where Ctrl+C ignored the registered native controller and approval dialogs swallowed cancellation. Configured Cancel uses the shared local controller cancellation route with ordinary foreground/voice priority, preserves pending cleanup and reports errors truthfully. Focused evidence and interrupted candidate receipts remain distinct from final exact-version release gates.

### 2026-10-08 settled CI diagnosis follow-up

[Settled-CI causal diagnosis repair](foundation/2026-10-08-rrc-ci-causal-diagnosis-repair.md) records the complete v0.24.9 red matrix and the native evidence stop. Shared diagnosis now retains runtime CLI assertions, first APT fetch failures and bounded same-step dependency timeout context; unproven causes remain uncertain. Windows CLI acceptance and CI acquisition inputs are repaired in the same candidate. The [v0.24.9 closeout](foundation/release-v0.24.9-closeout.md#corrective-implementation-and-observed-final-delivery) records eight fresh native gates, twelve exact-SHA prerequisite jobs, seven producing jobs, actual Windows installs, published-package audit and Complete/Idle final delivery as passed. Laptop acceptance and live-model coding repair effectiveness remain separately unexecuted.

[Release v0.24.9 closeout evidence](foundation/release-v0.24.9-closeout.md) records this release; full PRD parity remains separately evidenced.

The [final v0.24.9 requirement reconciliation](foundation/release-v0.24.9-closeout.md#requested-prd-evidence-reconciliation)
matched all 100 baseline named cases for all 36 binding sections/23 criteria,
plus eight publication/causal regressions, to observed passes in completed
exact-source canonical CI (108/108). No mapped acceptance case is uncovered.
This is current bounded-contract evidence, not universal absence of defects or
live-model/device acceptance beyond the recorded scope.

[Release v0.24.10 closeout evidence](foundation/release-v0.24.10-closeout.md) records this release; full PRD parity remains separately evidenced.
