# Migration Foundation Evidence

## Purpose

Own evidence and decisions that close the pre-workspace blockers identified by reconnaissance.

## Ownership

- `settings-and-update-execution.md` owns native Settings, automatic enrollment,
  updater repair evidence and unexecuted platform acceptance.
- `v0.22.2-release-execution.md` owns the Settings repair release receipts and
  initial installation evidence and pending user update test.
- `v0.22.3-release-execution.md` owns visual-upgrade release receipts and proof
  that the installed 0.22.2 payload was preserved for user updater testing.
- `output-visual-upgrade-execution.md` and `output-reference-*.png` own native
  output upgrade verification and actual renderer reference captures.
  `output-reference-render.py` rasterizes captured cells with Linux Noto fonts; it
  is an optional Pillow-based evidence helper, never production rendering.
- `evidence-index.md` is the durable execution ledger and command record.
- `completion-assurance-proposal.md` owns the researched proposal for native
  requirement coverage, execution receipts, and enforced completion decisions.
  It records inspected sources and the approved acceptance criteria.
- `completion-assurance-execution.md` owns ADR 0028 implementation evidence,
  executable coverage, measured limits and release readiness.
- `vro14-gap-audit.md` owns current web-extraction gaps, repair evidence,
  deployment prerequisites, and outstanding release acceptance.
- `vro15-gap-audit.md` owns independent swarm-extraction acceptance findings,
  verification limits, and the original repair proposal.
  `vro15-repair-execution.md` tracks Alex's approved full repair scope, gates and
  execution evidence, including VesperLens provider-neutral feedback delivery.
  `vro15-codex-repair-prompt.md` is the external coding-agent handoff for remaining
  repairs; the PRD, accepted ADRs and current execution matrix remain authoritative.
  `vro15-audit-probes.rs` is standalone non-production evidence: its assertions
  reproduce defects, not desired behavior; compile/run as documented in the report.
- ADRs under `adr/` record Stage 0 compatibility and product choices.
- `memory-oracle-cognitive-memory-blueprint.md` is the reconnaissance record for the
  external the memory oracle (`29fa4155`) oracle and the evidence base for ADR 0015
  (Stage 16 — `vesper-cognition`). The the memory oracle oracle is independent of the
  frozen Python harness; this is the only place where the memory oracle is cited.
- `vro13-pr8-closeout-evidence.md` is the VRO-13 cross-feature closeout record:
  the end-to-end fixture (`crates/vesper-harness/tests/vro13_e2e.rs`), the
  pipeline coverage (watcher/cron fire → composed firewall → sandbox route →
  scope-keyed transcript), and the PR-1..PR-8 verification trail per
  `docs/qm-extraction-prd.md`.
- `vesperlens-end-to-end-acceptance-postmortem.md` is the owner-directed
  VesperLens audit (v0.20.29 → v0.20.44): the binding end-to-end completion
  standard, the ten audited gaps, honest-scope list, and the open task of
  wiring the real-browser scripts into verification.
- The remaining reports document source-baseline diagnosis, fixture/oracle results, disposable Rust spikes, and readiness.

## Local Contracts

- The Python source repository is immutable and pinned to `bf4d4287e2e3320aa3f09015f678e6169d520045`.
- Distinguish reproduced or locally validated results from CI-pending and product-pending claims.
- Every report records objective, methods, commands, inspected/created files, exact evidence, tests, unresolved issues, platform scope, readiness effect, and status.
- Spike code is evidence only and must not be described as production Agent Vesper implementation.

## Work Guidance

- Update `evidence-index.md` after each bounded phase.
- Use language-neutral fixtures, deterministic local services, isolated state, and secret canaries.

## Verification

- Validate fixture manifests and results against their JSON Schemas.
- Re-run deterministic captures and compare canonical hashes.
- Confirm the source commit and status are invariant at closeout.

## Child DOX Index

No children.
