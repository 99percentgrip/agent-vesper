# RRC parity production release — 2026-10-06

## Objective and status

Publish the requested patch release with the RRC parity and native OpenAI sign-in
repairs while preserving the newer changes already committed to production.
**In progress: no production push, tag, publication or installation performed.**
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

## Deviations and unresolved work

Production advanced beyond the original parity candidate, so a reconciliation and
fresh verification are required before release. No acceptance scope is reduced.
Current local rerun, full hosted prerequisites, actual publication/checksums/native
closeout and the existing Registry PR update are outstanding. Live OpenAI account
completion and replacement of Alex's local installation are unexecuted.

## Readiness effect and DOX

No current production release-readiness claim yet. Nearest operational contracts are
reconciled for the combined behavior; parent indexes retain their existing boundaries.
Detailed final receipts and delivery summary will distinguish tested source, immutable
published source and any subsequent documentation-only main commit.
