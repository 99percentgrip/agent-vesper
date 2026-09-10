# ADR 0026: Swarm repair and cross-host acceptance

## Status

Accepted within Alex's approved full F01–F18 repair scope. Implementation and
product acceptance remain open. Supersedes ADR 0025's single-stream worker
composition, TUI-only exclusion, and completion/verification claims. ADR 0025 is
preserved as the immutable original decision record.

## Context

`../foundation/vro15-gap-audit.md` reproduced safety, orchestration and verification
defects in the original delivery. Alex approved full repairs, both native hosts,
and the accepted immutable snapshot-reader design; authorization and execution
are recorded in `../foundation/vro15-repair-execution.md`. A passing historical
suite is not proof of native parallel execution or supervisor cleanup.

## Decision

- Retain provider-neutral coordination in `vesper-swarm`, its dependency direction,
  pure execution/embedding/isolation ports, ephemeral ledger and coherent immutable
  snapshots. No frontend, provider-wire, filesystem or process implementation enters
  that crate. Existing monotonic timing dependencies remain explicitly tracked
  against the clock-injection goal; this decision does not silently waive it.
- Native workers compose the existing `AgentLoop`, real restricted tool registry,
  permission and sandbox services, scoped configuration and independent transactional
  sessions. A worker task may require multiple provider/tool continuations. Do not
  add a separate text-only loop, fabricated production embeddings or credential flow.
- Both TUI and ACP must compose the shared capability, cancellation, progress and
  command semantics. Native Settings must persist explicit opt-in. No blanket ACP
  protocol exclusion is accepted; any narrow host-specific UX exception needs the
  root DOX's concrete oracle/primary-document evidence.
- Keep swarm build/runtime activation default-off until the actual composed gates
  pass. Unverified cleanup must not be reported as success or release reserved
  capacity. No tool-call replay is authorized by cleanup retries or goal recovery.
- Tests must prove actual independent execution, role/tool/permission enforcement,
  evidence-fed synthesis, bounded cancellation/teardown, retained partial state,
  snapshot integrity and default-off/no-user-state behavior. Name/count-only tests
  and skipped namespace tests cannot certify these contracts.
- Current status belongs in `../migration-status.md` and the F01–F18 repair ledger,
  not historical ADR test counts. Release and registry operations remain separately
  authorized and exact-commit gated.

## Compatibility, security and migration consequences

- Default single-agent hosts remain unchanged; no new provider or host control is
  advertised before its implementation and gates exist.
- Internal snapshot schema revisions fail closed on unsupported versions; retained
  in-memory readers remain coherent. Swarm state is not implicitly durable.
- Explicit backend cleanup failures now propagate to the existing shared sandbox
  route instead of disappearing in Drop. Conservative quarantine can refuse later
  work; that is preferable to claiming resources were reclaimed without evidence.
- Native UI activation, complete asynchronous lease composition and external target
  acceptance are still implementation obligations, not completed by this ADR.

## Verification

- `cargo xtask verify`, `cargo xtask msrv`, and `cargo test --workspace`.
- `cargo test -p vesper-harness --features swarm --test swarm_native_hive`:
  native AgentLoop/tool continuation, three-session barrier and grounded synthesis.
- `cargo test -p vesper-swarm`: bounded lifecycle, identity, snapshot and concurrency
  regressions. `cargo test -p vesper-sandbox --all-features -- --nocapture`: distinguish
  real cleanup fixtures from unavailable-platform skips and ignored Docker checks.
- `cargo xtask architecture` and `cargo xtask naming-guard`: default-feature
  exclusion and fail-closed counted file/content naming baseline.
- Final product acceptance additionally requires actual supervisor and both-host
  execution, complete requirement traceability, supply-chain and target CI evidence
  in the repair ledger. No finite suite proves absence of all defects.
