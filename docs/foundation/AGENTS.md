# Migration Foundation Evidence

## Purpose

Own evidence and decisions that close the pre-workspace blockers identified by reconnaissance.

## Ownership

- `evidence-index.md` is the durable execution ledger and command record.
- ADRs under `adr/` record Stage 0 compatibility and product choices.
- `memory-oracle-cognitive-memory-blueprint.md` is the reconnaissance record for the
  external the memory oracle (`29fa4155`) oracle and the evidence base for ADR 0015
  (Stage 16 — `vesper-cognition`). The the memory oracle oracle is independent of the
  frozen Python harness; this is the only place where the memory oracle is cited.
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
