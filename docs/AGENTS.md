# Documentation

## Purpose

Own durable project documentation and evidence-backed engineering records.

## Ownership

- `voice-control-prd.md` owns the requested persistent red microphone toggle,
  recorder state projection, responsive lifecycle and terminal acceptance.

- `Vesper bridge/` owns VB-PRD-001 (Vesper Bridge application-control
  subsystem): the frozen PRD, the Phase 0 reconnaissance evidence package
  with digest-pinned upstream sources, and later phase reports.

- `dependency-setup-prd.md` owns confirmed native runtime installation, reuse,
  managed-machine recovery, readiness checks and platform acceptance gates.
- `Agent_Vesper_Release_Recovery_Controller_PRD.md` owns deterministic `/release`,
  CI recovery and post-release main-health requirements. Current implementation
  status, controlled private-GitHub receipts, five-target native host lifecycle
  evidence, and the explicitly excluded/unexecuted production publication remain
  in section 37 and the linked `foundation/` execution reports.

- `README.md` is the documentation landing page, separating user guides from
  contributor references, specifications, and historical evidence.
- `installation.md` owns user prerequisites, dependency setup, installation,
  updates, paths, troubleshooting, and the actual uninstall/data-removal behavior.
- `skills.md` owns user-facing skill routing activation, per-project controls,
  explicit invocation, data-flow/usage explanations and preview limitations.
- `using-vesper.md` owns everyday workflows and a curated command reference;
  `zed.md` owns custom ACP registration, provider selection and chat persistence.
- Keep user guides task-oriented and link to detailed engineering records rather
  than repeating implementation diaries or unverified capability claims.
- `recon/` owns the frozen Python-harness reconnaissance and Rust migration design.
- `foundation/` owns blocker-resolution evidence, compatibility decisions, fixture contracts, and disposable-spike verdicts.
- `vro19/` owns the adopted VRO-19 Google subscription/native-API planning bundle, its current G0 scope decision, retained source excerpts and unapproved delegated-agent architecture proposal. VRO-19 is on hold and reference-only: no further research, PRD revision, implementation, live acceptance or API-only substitute is authorized unless Alex explicitly resumes it.
- `architecture/` owns read-only external-repository pattern reconnaissance mapped to Vesper primitives for future features.
- Root PRD `advanced-hive-governance-prd.md` (VRO-16) owns the ratified requirements for task-level HITL gates (`YieldToHost`/`HostCommand`/`Suspended`), PIVOT/REFINE decision loops with judge separation, and deterministic verification gates; its upstreams are referenced exclusively as `governance alpha`/`governance beta` (naming-guard tokens land with its PR-1).
- `adr/` owns accepted production architecture decisions.
- `stage1/` owns production-workspace foundation evidence and readiness reports.
- `stage2/` owns shared-contract completion, compatibility, fixture coverage,
  and Stage 3 readiness evidence.
- `stage3/` owns the production GLM adapter evidence, compatibility, coverage,
  and Stage 4 readiness.
- `stage4/` owns the ACP adapter, minimal runtime, process-transcript evidence,
  coverage, and Stage 5 readiness.
- `stage5/` owns read-only session persistence contracts, discovery/runtime/
  replay evidence, disk invariance, governance, and Stage 6 readiness.
- Root documentation files own current architecture, workspace, dependency,
  security, contribution, migration status, and full-harness parity evidence.
- `openai-provider.md` owns native OpenAI Settings setup, authentication modes,
  model controls, account prerequisites, and explicit capability limitations.
  `foundation/2026-09-30-openai-responses-reinspection.md` owns the bounded
  current-source decoder reinspection and expected-shape structured diagnostic
  repair. `foundation/2026-09-30-openai-responses-streaming-boundary-audit.md`
  owns the follow-up framing/boundary/no-replay matrix, semantic unknown-event
  classification, byte-measurement clarification and both-host safe-message
  separation. `foundation/2026-09-30-openai-responses-live-prerelease-candidate.md`
  owns the clean isolated debug candidate identity, secret-canary host proof,
  direct OpenAI runtime receipt and explicit no-release/no-install boundary.
- `vro18-native-xai-provider-prd.md` owns VRO-18 native xAI reasoning-provider
  requirements, phased acceptance, billing-path isolation, and adjacent-service
  exclusions. `architecture/recon_xai_native_provider.md` and
  `foundation/xai-provider-recon-execution.md` own its PR-0 evidence;
  `foundation/xai-provider-pr1-execution.md` through
  `foundation/xai-provider-pr7-execution.md` own phased implementation;
  `foundation/vro18-audit1-completeness-and-capability-truth.md` owns the
  independent post-v0.24.0 completeness/capability audit, F0 repair, invalid
  truncated-link correction, browser URL-integrity repair, live browser/device
  matrices and repaired Audit 1 acceptance boundary;
  `foundation/2026-09-26-v0.24.1-vro18-audit1-corrective-release.md` owns the
  exact-commit corrective release gates, publication/assets, Registry update,
  and explicit Audit 2/Audit 3-open boundary;
  `vro18-provider-authentication-settings-prd.md` owns the Settings
  authentication route, and `foundation/vro18.1-settings-authentication-execution.md`
  owns its implementation evidence;
  `foundation/xai-provider-pr8-live-acceptance.md` and
  `foundation/2026-09-25-v0.24.0-release-execution.md` own live and release
  acceptance. `xai-provider.md` is the bounded user guide.
- `web-tools.md` owns web-driver installation, immutable image identity,
  native Settings → Web tools and ACP `/web` activation, advanced opt-in
  configuration, operation bounds, and deployment troubleshooting.
- `skill-routing-quality-prd.md` owns the proposed descriptor/search/contract
  routing improvements and measured adoption gates. The primary goal is automatic
  selection of the correct skill from natural task descriptions, without requiring
  users to name skills. Preserve Alex's complete
  installed skill library: task shortlisting must not delete, automatically
  archive/disable or rewrite skill sources or user edits. It does not authorize
  full-body ranking or supersede ADR 0024 or the pending chunk-floor repair.
- `skill-routing-model-assistance-proposal.md` owns the approved bounded
  active-provider selection addition and its opt-in data-flow/latency boundary.
- `output-visual-upgrade-prd.md` owns semantic activity colors, truthful status
  dots, syntax-colored numbered diffs, and aligned responsive reports.
- `settings-and-update-prd.md` owns theme consistency, grouped Settings saves,
  automatic PRD enrollment and confirmed native update installation.
- `Agent_Vesper_Release_Recovery_Controller_PRD.md` owns the P0 deterministic
  release/CI-recovery lifecycle, complete-matrix rule, evidence/fingerprint and
  retry policy, external-pause semantics, durable checkpoint, and TUI/ACP parity.
  `foundation/release-recovery-controller-pr1-pr3-execution.md` records the
  original bounded implementation; `foundation/release-recovery-controller-pr4-pr7-execution.md`
  records the production executor and controlled GitHub checkpoint; and
  `foundation/release-recovery-controller-final-completion.md` records the
  process-tree repair, five-target lifecycle evidence and final non-publishing
  audit. `foundation/2026-09-30-release-intent-autonomy-repair.md` records
  ordinary-language admission and isolated-worktree repair;
  `foundation/2026-09-30-rrc-source-resolution-ux-repair.md` records the
  objective-provenance follow-up; `foundation/2026-09-30-rrc-task-lifecycle-repair.md`
  records registered worker ownership, real local-gate progression and truthful TUI
  activity; `foundation/2026-09-30-rrc-task-lifecycle-prerelease-candidate.md`
  records the exact fresh Linux TUI/ACP candidate identities for live acceptance;
  `foundation/2026-09-30-live-rrc-task-supervision.md` records read-only evidence
  that the candidate-owned controller and local verification were progressing;
  `foundation/2026-09-30-rrc-live-telemetry-ux-repair.md` records the earlier bounded
  live gate/process telemetry work and the candidate later rejected by live terminal
  acceptance; `foundation/2026-09-30-rrc-terminal-ownership-repair.md` records the
  corrective exclusive-writer path, full-PTY/mutation evidence and replacement
  non-publishing prerelease TUI candidate;
  `foundation/2026-09-30-rrc-ux-validation-repair.md` records the red-to-green
  follow-up validation: deterministic paired-stream PTY evidence, complete
  milestone-cursor initialization, strict-Clippy remediation and the 43-case
  Linux-local acceptance receipt;
  `foundation/2026-09-30-rrc-progress-ux-full-verification.md` records the
  all-features PTY bounded-tail repair, retained red receipt, twenty-repeat
  stability result and successful complete `cargo xtask verify` receipt without
  a candidate or release action;
  `foundation/2026-09-30-rrc-host-resource-governor.md` records the Linux host/cgroup
  memory and disk governor, constrained-host regression evidence, cross-host resource
  telemetry and its explicit no-release boundary;
  `foundation/2026-09-30-rrc-governor-acp-process-acceptance.md` records the ACP
  process-level status and every-Cargo-path inheritance proof;
  `foundation/2026-09-30-rrc-governor-acp-process-verification-rerun.md` records its
  independent current-worktree verification rerun, including the 44-case acceptance
  and complete verification receipts;
  `foundation/2026-09-30-rrc-governor-live-host-observation.md` records the
  timestamped read-only Linux capacity/default-policy observation and the unavailable
  requested TUI test-target receipt without creating RRC state or rebuilding; and
  `foundation/2026-09-30-rrc-progress-milestones.md` records the persisted typed
  phase/gate/job/milestone projection shared by TUI chat/RUN and ACP status, its scoped
  verification and the explicit no-release boundary.
  `foundation/2026-09-30-rrc-complete-version-mutation-repair.md`
  records the fail-closed complete workspace-version mutation repair;
  `foundation/2026-09-30-rrc-stale-epoch-autonomy-repair.md` records automatic
  persisted-epoch reconciliation, obsolete-evidence archival, bounded ambiguity
  handling, enrolled acceptance cases and the fresh non-publishing TUI candidate;
  `foundation/release-objective-provenance.json`
  is the versioned current completed-objective binding consumed by RRC;
  `foundation/2026-09-30-autonomous-rrc-prerelease-candidate.md` records the
  earlier non-publishing TUI/ACP candidate build for Alex-operated acceptance; and
  `foundation/2026-10-01-rrc-provenance-ambiguity-repair.md` records canonical
  final-source binding, equivalent-candidate collapse, the real retained-worktree
  probe, governed final gates and the current `0.24.4` replacement TUI;
  `foundation/2026-10-01-rrc-active-epoch-integration-repair.md` records canonical
  historical-epoch supersession; and
  `foundation/2026-10-01-rrc-active-epoch-final-audit.md` records the red-first
  known-version clarification correction, deterministic lifecycle acceptance,
  complete verification, later governor-admitted locked and release-profile TUI
  builds, the preserved final `0.24.4` Linux acceptance artifact, and unchanged
  no-release boundary; and
  `foundation/2026-10-01-rrc-resource-defer-zram-repair.md` records the later
  current-risk/zram physical-backing repair, typed passive resource defer and
  automatic recovery, truthful deferred TUI state, complete local gates,
  implementation commit, fresh Linux candidate identity, and unchanged
  no-release/no-install boundary; and
  `foundation/2026-10-01-v0.24.5-rrc-autonomy-and-ci-repair.md` records all four
  immutable failed v0.24.5 candidate matrices,
  missing/partial matrix-watch and complete-red multi-family repair corrections,
  all four rounds of observed CI failure-family repairs, complete
  50-case/canonical/MSRV/local-runtime verification, and the pending fresh exact-SHA
  hosted boundary with no tag or publication in scope.
- Root PRD files (`*-prd.md`) own accepted phased requirement documents
  (e.g. `agent-vesper-reasoning-orchestrator-prd.md`,
  `provider-capability-gating-prd.md`, `qm-extraction-prd.md`,
  `result-aware-loop-detection-prd.md`,
  `web-oracle-extraction-prd.md`),
  and `swarm-oracle-extraction-prd.md` (VRO-15 — accepted requirements;
  its original completion claim is disputed by `foundation/vro15-gap-audit.md`,
  with full repairs approved and tracked in `foundation/vro15-repair-execution.md`;
  original decision record `docs/adr/0025-provider-neutral-swarm-orchestration.md`
  and superseding repair/parity decision
  `docs/adr/0026-swarm-repair-and-cross-host-acceptance.md`; upstream is
  referenced only as the swarm oracle — no upstream brand names may
  appear in docs or source, enforced by `cargo xtask naming-guard`);
  implementation evidence for their
  phases lands in the owning stage/foundation/adapter directories.
- `advanced-context-paging-prd.md` owns the planning-stage requirements for
  bounded skill chunks and rich per-chunk routing metadata. Its upstream is
  referenced exclusively as `context upstream` (same naming-embargo rule as
  the swarm oracle; recon evidence in `architecture/recon_context_paging.md`).
  Binding constraints: no document-ingestion/conversion surface (explicitly
  rejected), chunks land in `vesper-memory`/`vesper-agent` shared paths for
  both hosts, and `summary`/`key_elements` routing metadata stays default-off
  in automatic routing until the PRD's D3 repeated-benefit eval gate records
  an `ADOPT` verdict in `docs/foundation/` (recorded: ADOPT,
  `foundation/context-paging-pr4-eval.md`).
- `ranker-hardening-prd.md` owns the planning-stage requirements for
  eliminating the chunk-tier alias cross-talk (recon:
  `architecture/recon_alias_crosstalk.md`). Binding constraints: scope is
  Option A (raw stemmed chunk pools, alias loop bypassed for the chunk tier
  only) + Option D (failing cross-talk fixture as anchor) — global skill-tier
  routing arithmetic is explicitly out of scope; the bounded prompt-side
  residual (520-pt literal match) is a recorded, accepted limitation, not a
  hidden one.
- `voice-oracle-extraction-prd.md` owns VRO-17 — the planned real-time
  bidirectional voice subsystem extracted (no port) from the voice oracle
  upstream: `vesper-voice` as a provider-neutral, agent-free core with
  interchangeable local/cloud STT and TTS ports, barge-in routed through
  the existing transactional cancellation path, sentence-gated streaming
  TTS with pre-cloud secret redaction, and the PR-0…PR-5 gated migration.
  VRO-17 v1 ships the local speech stack; concrete third-party cloud STT/TTS
  are future optional features behind the existing ports and security gates.
  Its upstream is referenced exclusively as the voice oracle; reconnaissance
  evidence and upstream pin live in
  `architecture/recon_voice_oracle.md`.
- `chunk-score-floor-prd.md` owns the completed (2026-09-13) requirements
  for eliminating the chunk-tier cosine noise floor (finding:
  `foundation/exemplar-migration-execution.md` §5.1). Landed contract:
  the eligibility gate is the conjunction `(overlap >= 1 || name_match) &&
  score >= MIN_CHUNK_ROUTING_SCORE (520)` in `rank_chunks` only — a bare
  score floor was explicitly rejected (the observed 678-pt noise outlier
  passes 520; PRD §1.1); chunk-name admission is delimiter-bounded
  (`chunk_name_matches`); skill-tier thresholds (`AUTO_ACTIVATION_SCORE`)
  were never touched; the exemplar G5 test asserts the literal zero-chunks
  form on a verified vocabulary-free fixture.

## Local Contracts

- Separate confirmed current behavior from inference and proposed architecture.
- Cite repository-relative source paths, symbols, and line ranges when practical.
- Preserve unresolved contradictions and test gaps instead of smoothing them over.

## Work Guidance

- Keep reports independently useful and maintain the evidence index incrementally.

## Verification

- Check changed Markdown links and anchors, parse documented JSON, and compare
  install/uninstall commands against the scripts. Do not run a user's installer
  or uninstaller just to validate documentation examples.
- Review reconnaissance documents against `recon/AGENTS.md` and the mission completeness audit.

## Child DOX Index

- `foundation/AGENTS.md` — Stage 0 decisions, fixture/oracle evidence, and technical-spike reports.
- `vro19/AGENTS.md` — adopted VRO-19 reference material and the explicit initiative hold contract.
- `architecture/AGENTS.md` — read-only external-repo pattern reconnaissance feeding feature PRDs.
- `recon/AGENTS.md` — Agent Vesper migration reconnaissance records and quality gates.
- `adr/AGENTS.md` — accepted production decisions and verification obligations.
- `stage1/AGENTS.md` — Stage 1 execution ledger, coverage, CI status, and final report.
- `stage2/AGENTS.md` — Stage 2 contract, oracle, compatibility, and readiness records.
- `stage3/AGENTS.md` — Stage 3 GLM adapter and Stage 4 readiness evidence.
- `stage4/AGENTS.md` — Stage 4 ACP/runtime evidence and Stage 5 readiness.
- `stage5/AGENTS.md` — Stage 5 read-only persistence evidence and scope.
