# vesper-bridge

## Purpose

Own the provider-neutral application-control core contracts for VB-PRD-001
(Vesper Bridge): generation-bound identities, the versioned capability
manifest, typed operation envelopes and result classification, session and
operation state machines, fenced exclusive mutation leases, the intent
journal port, bounded observations, and the provider-neutral error
taxonomy.

## Ownership

- Pure crate: no I/O, no process spawning, no capture, no network, no
  clock dependence. Time, observations and storage arrive as arguments.
- Depends only on `vesper-domain` and `vesper-security` plus
  `serde`/`serde_json`/`thiserror`.
- Application, driver, provider and transport behavior composes in the
  hosted layer (`vesper-harness`) behind the ports defined here; nothing
  in this crate names an application or transport.
- The deterministic tests in `tests/session_gate.rs` and module `#[cfg(test)]`
  blocks are the Phase 1 evidence class: they prove contract behavior only
  and must never be cited as application-control capability.

## Local Contracts

- Unknown implementation status is never dispatchable and never reportable
  as supported (BR-03).
- The host mints request/session/lease/approval references; models only
  propose `OperationSpec` (capability id + JSON arguments).
- `authorize()` is the single pre-dispatch gate: admission, session
  readiness, quarantine, mode enforcement, capability availability,
  generation freshness, observation freshness, lease fencing and duplicate
  suppression all run before any adapter is consulted, and intent is
  journaled before dispatch (fail-closed).
- Driver acknowledgments settle to `Applied`, never `Verified`;
  `verify()` is the only promotion path and requires independent evidence.
- Stop closes admission, requests emergency input release (which survives
  ordinary lease revocation) and keeps outstanding jobs visible.
- Denial (`PermissionDenied`, target ambiguity/change) blocks route
  fallback; only missing-capability and transport errors may fall back
  within policy.
- Errors carry retry eligibility; at most two safe transport retries and
  no automatic ambiguous-mutation retries (NF-10).

## Work Guidance

- Keep modules small and orthogonal: identity, capability, operation,
  observation, lease, journal, error, session.
- New state transitions need deterministic tests first; no test may depend
  on wall-clock time or external processes.
- Extend `CapabilityRecord` fields (idempotency, cancellation semantics)
  only with manifest-generation bump semantics in mind (BR-28).

## Verification

- `cargo test -p vesper-bridge` (46 deterministic tests at Phase 1).
- `cargo clippy -p vesper-bridge --all-targets --all-features -- -D warnings`.
- `cargo xtask architecture` validates the dependency allowlist entry.
- Fake/deterministic fixtures stay in tests; production has no fake
  drivers (no stubs that report successful connection or execution).

## Child DOX Index

- none
