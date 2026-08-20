# Documentation

## Purpose

Own durable project documentation and evidence-backed engineering records.

## Ownership

- `recon/` owns the frozen Python-harness reconnaissance and Rust migration design.
- `foundation/` owns blocker-resolution evidence, compatibility decisions, fixture contracts, and disposable-spike verdicts.
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
- Root PRD files (`*-prd.md`) own accepted phased requirement documents
  (e.g. `agent-vesper-reasoning-orchestrator-prd.md`,
  `provider-capability-gating-prd.md`); implementation evidence for their
  phases lands in the owning stage/foundation/adapter directories.

## Local Contracts

- Separate confirmed current behavior from inference and proposed architecture.
- Cite repository-relative source paths, symbols, and line ranges when practical.
- Preserve unresolved contradictions and test gaps instead of smoothing them over.

## Work Guidance

- Keep reports independently useful and maintain the evidence index incrementally.

## Verification

- Review reconnaissance documents against `recon/AGENTS.md` and the mission completeness audit.

## Child DOX Index

- `foundation/AGENTS.md` — Stage 0 decisions, fixture/oracle evidence, and technical-spike reports.
- `recon/AGENTS.md` — Agent Vesper migration reconnaissance records and quality gates.
- `adr/AGENTS.md` — accepted production decisions and verification obligations.
- `stage1/AGENTS.md` — Stage 1 execution ledger, coverage, CI status, and final report.
- `stage2/AGENTS.md` — Stage 2 contract, oracle, compatibility, and readiness records.
- `stage3/AGENTS.md` — Stage 3 GLM adapter and Stage 4 readiness evidence.
- `stage4/AGENTS.md` — Stage 4 ACP/runtime evidence and Stage 5 readiness.
- `stage5/AGENTS.md` — Stage 5 read-only persistence evidence and scope.
