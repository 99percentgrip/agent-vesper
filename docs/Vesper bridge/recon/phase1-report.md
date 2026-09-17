# Vesper Bridge — Phase 1 execution report (pure contracts + deterministic tests)

## Objective

Implement VB-PRD-001 Phase 1: the `vesper-bridge` pure-contract crate
(types, state transitions, capability/identity model, result
classification, leases, deadlines, journal port, error contract) with
deterministic tests for denial, stale identity, duplicate requests,
unknown outcomes and shutdown — **no production app-control side
effects**, per the phase gate.

## Methods and commands

- New crate `crates/vesper-bridge` (workspace member; deps:
  `vesper-domain`, `vesper-security`, serde/serde_json/thiserror only).
- Architecture allowlist entry added in `xtask/src/main.rs`
  (`allowed_dependencies()`).
- Deterministic evidence: `cargo test -p vesper-bridge` — 46 tests
  (29 lib unit + 17 integration in `tests/session_gate.rs`).
- Gates: `cargo clippy -p vesper-bridge -p xtask --all-targets
  --all-features -- -D warnings` (clean), `cargo fmt` (clean),
  `cargo xtask architecture` (**28 packages validated** — was 27),
  `cargo xtask naming-guard` (clean, 33 frozen hits).

## Files changed

- `Cargo.toml` — workspace member `crates/vesper-bridge`.
- `crates/vesper-bridge/Cargo.toml`, `AGENTS.md` — crate + local contract.
- `crates/vesper-bridge/src/{lib,identity,capability,operation,
  observation,lease,journal,error,session}.rs` — the core.
- `crates/vesper-bridge/tests/session_gate.rs` — integration evidence.
- `xtask/src/main.rs` — allowlist entry with rationale comment.
- `crates/AGENTS.md` — contract bullet + child index entry.
- Earlier in this work unit (enrollment mechanics): `crates/vesper-domain/
  src/acceptance.rs` (MAX_REQUIREMENTS 256→512), `crates/vesper-harness/
  src/acceptance.rs` (honest error + test module), new
  `acceptance_enrollment_bounds_tests.rs`, `xtask/naming-guard-baseline.json`
  regeneration, `docs/Vesper bridge/` evidence package, `docs/foundation/
  {enrollment-ceiling-repair.md, evidence-index.md}`.

## Exact evidence (invariants proven deterministically)

| Requirement | Test | Result |
|---|---|---|
| BR-03 unknown≠supported | `unknown_capability_is_refused_not_supported`; `unknown_is_never_dispatchable_or_supported` | PASS |
| BR-07 plan/read-only enforcement | `plan_mode_blocks_mutations_entirely`; `read_only_permission_blocks_mutations` | PASS |
| BR-02/15 stale generation | `restarted_application_generation_refuses_old_binding` | PASS |
| BR-15 stale/degraded observation | `superseded_observation_refuses_dispatch`; `degraded_capture_observation_refuses_visual_grounding` | PASS |
| BR-14 lease exclusivity + fencing | `lease_exclusivity_two_writers_one_document`; `one_exclusive_writer_per_resource`; `stale_fence_never_wins` | PASS |
| BR-16 duplicates/uncertainty | `duplicate_request_ids_are_suppressed_before_dispatch`; `uncertain_outcome_requires_reconcile_before_retry`; journal tests | PASS |
| BR-12/NF-13 ack≠verified | `driver_ack_never_verifies_and_unknown_outcome_never_succeeds`; `only_verified_counts_as_success` | PASS |
| BR-17/NF-02 stop semantics | `stop_closes_admission_and_requires_fresh_observation_on_resume`; `stop_with_held_input_reports_release_targets_and_unconfirmed_state` | PASS |
| BR-13 outstanding jobs | `outstanding_jobs_survive_cancellation_and_remain_visible` | PASS |
| BR-18 quarantine | `quarantined_session_blocks_new_writes` | PASS |
| BR-28 manifest revision | `manifest_revision_blocks_previously_available_operation`; `manifest_revision_bumps_generation_for_approval_invalidation` | PASS |
| §6.6 denial blocks fallback | `denial_blocks_route_fallback` | PASS |
| NF-10 retry bound | `transport_failures_allow_at_most_two_safe_retries` | PASS |
| NF-09 default deadline | `default_timeout_is_15s_ordinary_actions` | PASS |
| NF-07 pixel budget | `pixel_budget_bounds_the_package` | PASS |
| §6.1 capture identity | `capture_identity_requires_serial_for_durable_across_handle_reuse` | PASS |
| BR-04 readiness | `not_ready_session_refuses_everything` | PASS |

Full receipt: `test result: ok. 29 passed` (lib) + `17 passed`
(`tests/session_gate.rs`), zero failures, zero ignored.

## Honest status against the PRD acceptance matrix

- These tests are **deterministic contract evidence (unit/integration
  class)**. They do not certify any application-control capability; no
  real driver, application or host integration exists yet (Phase 2+).
- All 44 AT scenarios remain **NOT TESTED**; the Resolve lane remains
  **BLOCKED** (no local installation). Phase 1's gate — "mandatory
  contract tests and current architecture/MSRV gates pass; no production
  app-control side effects exist" — is met: no sidecar, capture, network
  or process spawn was added anywhere.
- MSRV check on 1.88 toolchain was **not run this session** (only 1.95
  present locally); the crate uses no features beyond the workspace
  baseline, but this remains an unexecuted gate item, not a pass.

## Deviations

- `session_gate.rs` mints request IDs from session record count
  (`bs-1/N`); this is the host-minted id policy's deterministic stand-in.
  Real random/ULID minting composes at the hosted layer without changing
  the contract.
- The enrollment copy `Vesper_Bridge_PRD.enroll.md` (paragraph-coalesced,
  whitespace-normalization-identical to the original; generator asserts
  equality) exists solely because the session's hosting binary predates
  the ceiling repair. The original PRD file is untouched and remains the
  reference document; both carry identical normative text.
- Native enrollment still could not complete in-session: the running host
  process refused with the pre-repair string, then the repaired path hit
  the enrollment window during contract preparation. No reduced scope was
  enrolled; no completion is claimed.

## Unresolved items

1. Native enrollment freeze + verification (needs hosting-process restart
   on the repaired binary; then contract ladder within the 300 s window).
2. MSRV 1.88 confirmation for the new crate.
3. Phase 2 (harness composition, tool surface, both hosts) not started.
4. Phase 3/4 lanes remain blocked per Phase 0 (Resolve install; Cua
   authorization).

## Readiness effect

Phase 1 gate is satisfied in source with deterministic evidence and all
applicable repository gates green. The architecture enforcement grew by
exactly one allowlisted pure crate — no exemptions broadened. Phase 2 may
proceed; platform lanes stay explicitly blocked.
