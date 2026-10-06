# RRC version-preparation resource recovery

## Objective and status

Repair the stalled native 0.24.6 attempt without resetting its budget, replaying
uncertain operations, weakening resource safety, or changing Alex's installation.
**Reproduced and locally repaired; integration and production publication pending.**
The earlier full-PRD completion claim is withdrawn for current behavior.

## Observations and cause

- Source parent: `7a8de2f57d3199f9cf1858b4dabfcd9dbc083161`.
- Active epoch: `20261006T112521.786854617Z-7a8de2f57d31`.
- The ACP process remained alive while its registered controller worker exited.
  Repeated status queries reported LocalVerification, version-preparation Running,
  0/8 gates, no source failure, and an unsettled LocalVerification journal.
  No candidate push, tag or publication was recorded.
- `prepare_version_bump` transactionally restores manifests and Cargo.lock on
  governor rejection, then returns ResourceConstrained. The controller handled
  this error only for later local gates, so preparation escaped its worker.
  `persist_worker_failure` discarded ResourceConstrained entirely. Status polling
  could neither recover the worker nor explain the stop.
- At diagnosis the target filesystem had about 96–97 GiB available. Default policy
  reserves 10% of the roughly 951 GiB filesystem plus 30 GiB gate growth, requiring
  about 125.13 GiB. This remains a real capacity block. The original error text was
  discarded; the current filesystem calculation is a reproducible explanation,
  not a recovered original error receipt.

## Changes and contracts

- `release_executor.rs`: share ordinary-gate ResourceDeferred handling with
  version preparation. Preserve the pending gate, measured telemetry/reason and
  retry budget. Settle the preparation journal only after the transactional port
  reports rollback; resume after the existing three safe Normal observations.
- Preserve redacted escaped resource-worker diagnostics and a recoverable owner
  state without creating source failures or clearing an uncertain journal.
- Avoid reporting a previous successful gate when preparation is the first gate.
- Three new regressions cover preparation defer/reload/recovery, actual native
  version-byte rollback before a forced filesystem rejection, and preservation of
  escaped resource diagnostics with an uncertain journal retained.
- Both hosts use this shared controller. No host-specific dispatch or presentation
  workaround was added. Harness DOX, owning PRD status, evidence index and release
  objective report links are updated with the repair.

## Methods and exact evidence

Read-only process, worktree, status-output, ledger and statvfs observations preceded
edits. Verification used an isolated worktree/branch with one Cargo job and one
Rust test thread. The finished repair is integrated by fast-forward into the clean
OpenAI candidate branch; installed binaries remain untouched. The stalled status-
polling helper can be stopped after confirming no controller worker is active. Retained logs and source digests are in the linked receipts.

Commands:

```text
cargo test -p vesper-harness --lib --all-features release_executor::tests::version_preparation_resource_defer_recovers_without_replaying_uncertain_mutation -- --exact --nocapture
cargo test -p vesper-harness --lib --all-features release_executor::tests -- --test-threads=1
cargo clippy -p vesper-harness --lib --tests --all-features -- -D warnings
cargo fmt --all -- --check
git diff --check
```

- Before production changes, the exact new regression executed and failed:
  `ResourceConstrained("fixture version admission pressure")`; 0 passed, 1 failed,
  exit 101. After repair: 1 passed, 0 failed, exit 0.
- Final release-executor suite: **62 passed, 0 failed**, including native rollback,
  resource watch, registered-worker lifecycle, restart no-replay, cancellation,
  version graph and fixture publication coverage. These are offline fixtures,
  not actual release publication evidence.
- Scoped all-feature Clippy passed with `-D warnings` (exit 0). Formatting and
  whitespace checks passed. Added report links and JSON documents were reviewed.

## Deviations and unresolved items

- The first new-test compilation referenced a fixture type outside its scope
  (E0425). It was corrected before the executed red regression; it is not counted
  as defect reproduction. Its full log was overwritten by the corrected run.
- A documentation-edit helper initially selected a nonexistent text anchor. It
  made no documentation change; the exact anchor was subsequently corrected.
- A whole-index link scan encountered the pre-existing missing
  `2026-10-02-agent-process-status-check.md` report. Added repair links were checked
  separately and passed; the unrelated index baseline is not relabeled as valid.
- Verification reused the candidate's existing target directory while no competing
  Cargo process was active, with a different source worktree path forcing rebuilt
  packages. No red/green branch alternation or stale pre-change binary was used.
- The live epoch is preserved. Its old unresolved journal is not manually cleared.
  A fresh built native host must reconcile the strictly newer canonical source and
  archive the old record under the existing admission rules before progressing.
- The disk safety threshold remains unchanged. No caches, unrelated work or user
  state were deleted to manufacture readiness. Capacity must satisfy native policy
  before actual expensive gates can run.
- No full workspace, new cross-host process matrix, hosted CI, registry update,
  tag or production publication was executed by this repair. Existing v0.24.5 is
  unaffected. No installer or live provider/authentication request was run.

## Readiness effect and DOX closeout

The reproduced version-preparation escape is repaired locally. This does not
certify the full RRC PRD or make the stuck older process progress automatically.
The closest harness and foundation contracts are updated. Root/apps/docs parent
contracts and Child DOX Index entries stay unchanged because ownership boundaries,
provider composition and global release policy did not change.

[Observation and source evidence](2026-10-06-rrc-version-preparation-resource-repair-evidence.json)
and [retained receipts](2026-10-06-rrc-version-preparation-resource-repair-receipts.tar.gz).
