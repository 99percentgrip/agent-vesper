# Completion assurance implementation and evidence

Date: 2026-09-11. Baseline: `8083f9f131939d14d1f8465f056ffaf071a91cdc`.
Decision: [ADR 0028](../adr/0028-native-implementation-acceptance.md).
Scope: approved native PRD completion gate, verification and regression prevention.
Status: implemented and locally verified; v0.21.7 release preparation is authorized.
Publication and installation require the exact-commit gates below.

Windows release validation exposed an opt-in discovery regression for ACP sessions
with unresolved/virtual workspace paths. Saved activation now defaults to inactive
only for unresolved or absent roots; explicit enrollment/saves remain strict and
unreadable/malformed existing settings still refuse. A cross-platform regression
checks absent-root reads, rejected saves and non-directory refusal.

## User workflow

- One objective: `/acceptance start docs/my-prd.md`.
- Persistent activation: TUI Settings → Implementation acceptance, select ON and
  the PRD path, then Save. ACP: `/settings acceptance on docs/my-prd.md`.
- `/acceptance status` reports gaps; `/acceptance resume` continues repairs.
- `/acceptance revise <PRD>` explicitly replaces scope while retaining the old
  incomplete objective in lineage. Changing requirements never earns completion.
- `/acceptance export <new-workspace-file>` explicitly saves an audit bundle;
  `/acceptance resume <audit-file>` restores scope with fresh review/verification.
- `/acceptance stop` or `/settings acceptance off` ends enforcement explicitly,
  without declaring completion. Existing clear task authorization is sufficient
  to prepare a contract; no additional routine approval step is introduced.

The implementer proposes checks through `acceptance_configure`, runs them through
permission-gated `acceptance_verify`, and can request independent re-examination
through `acceptance_review`. The model cannot upload receipts, clear findings or
edit the in-memory contract. Required absent, failed, stale or inconclusive
criteria remain in the authoritative report.

## Evidence mapping

| Obligation | Executable coverage |
|---|---|
| Plan omission/replacement/checkmarks cannot certify | `delegated_finish_and_empty_deleted_or_replaced_plans_cannot_certify_parent`; `repeated_false_completion_stops_incomplete_at_the_bound` |
| Both hosts and native classes need evidence | `every_required_host_needs_its_own_evidence`; `unit_tests_cannot_substitute_for_native_tests`; `zero_matches_and_ignored_tests_are_not_passing_receipts` |
| Fresh source, definitions and environment | `changed_source_contract_checks_platform_or_test_invalidates`; `real_failing_check_repairs_then_source_edit_invalidates_success`; snapshot/configuration tests |
| No receipt upload or silent scope weakening | `forged_receipts_scope_changes_and_cross_workspace_use_refuse`; strict DTO/policy validation |
| Legitimate evidence reuse and scope lineage | real repair/reuse case; `explicit_scope_revision_and_resume_preserve_history_but_never_import_verification` |
| Reviewer failure/findings and resolution | `reviewer_unavailable_or_without_source_inspection_cannot_approve`; `unresolved_independent_findings_block_even_real_green_tests`; independent re-examination case |
| Publication/cancellation/budget truth | actual AgentLoop repair/withheld-delta test; `cancelled_and_exhausted_turns_publish_incomplete_history`; TUI acceptance event test |
| Parent authority across modes | shared delegated-finalization test; VRO/ReAct/Swarm native composition returns through that boundary; existing mode suites in full workspace verification |
| Native controls/no default durable evidence | real ACP subprocess `acceptance_controls`; native TUI settings render test; shared settings roundtrip/default-no-write test |
| Defective and known-good replay | real Rust defect 0→42→0; controlled historical-shaped missing-host/class cases; two actual evaluator mutations |

Core test locations:
`crates/vesper-agent/tests/acceptance_policy.rs`,
`crates/vesper-harness/src/acceptance_tests.rs`,
`apps/agent-vesper-acp/tests/acceptance_controls.rs`,
`apps/agent-vesper-tui/src/acceptance_host.rs`, and native TUI event tests.

## Executed checks

| Command / check | Result and scope |
|---|---|
| `cargo xtask verify` | Final run passed canonical workspace, strict Clippy, all-feature tests, doctests, fixtures, architecture, naming and all 20 exact acceptance cases (22.825 seconds for the exact-case gate) |
| `cargo test --workspace --offline` | Passed default-feature workspace and doctests |
| `cargo +1.88.0 test --workspace --all-features --locked --offline` | Passed full MSRV workspace and doctests |
| `cargo +1.88.0 xtask acceptance` | Passed all 20 exact cases again after the final receipt provenance, report and Settings adjustments; 42.135 seconds including Cargo invocation/build overhead |
| `cargo test -p vesper-harness --all-features acceptance --offline` | 19 runtime/settings cases passed; 1.03 seconds test execution in the recorded run, compilation excluded |
| Native ACP `acceptance_controls` | Real isolated subprocess passed; loopback binding required the approved test execution outside the tool sandbox |
| Native TUI acceptance tests | Settings render, authoritative incomplete event and shared Settings/command routing passed |
| `cargo xtask acceptance-mutations` | Both deliberate evaluator defects were killed by the expected assertions; unmodified controls passed |
| `cargo clippy --workspace --all-targets --all-features --offline -- -D warnings` | Passed after receipt provenance and reporting adjustments |
| `cargo build -p agent-vesper-acp -p agent-vesper-tui --all-features --offline` | Both native binaries built after the final TUI configuration adjustment |
| Workflow YAML and `git diff --check` | Passed |

The default-feature and full MSRV workspace runs preceded the final small
reporting/parser/provenance and native Settings adjustments. Focused MSRV gates
covered those adjustments before the last TUI enrollment configuration fix;
the final canonical run and binary build covered that fix. No remote platform
result is inferred from these Linux executions.

The controlled repair makes five scripted implementation-provider requests:
unsupported stop, real file repair, check admission, actual verification, final
stop. Reviewer replies in that scenario are fixtures. The final claim is generated
from evidence and does not reproduce the model's unsupported “bug free” prose.
These observations measure deterministic regression behavior; they do not estimate
live-model accuracy, real provider token cost, or an improvement percentage.
No live provider calls were used in foundation verification.

## Limits and readiness

The scope and trust boundaries in ADR 0028 are material. This is not proof that an
agent always writes correct software, and not a claim that every external chat
completion is intercepted. The gate is active only for enrolled objectives or
saved native activation. Review can still miss semantic gaps; new defects need
new regression cases. Required unknown/unsupported evidence fails closed.

Local checks do not establish Windows/macOS/ARM CI success, publication or local
installation. Workspace and registry manifests are prepared for v0.21.7. Before
tagging, its exact main commit must pass canonical, MSRV, five-target foundation
and web-driver workflows. Published release assets and the local installation must
then be verified separately; preparation is not publication evidence.

## DOX closeout

- Root reporting preferences, all affected crate/app owners, ADR ownership,
  foundation evidence, migration status and CI/xtask contracts are updated.
- `crates/AGENTS.md`, `apps/AGENTS.md` and `docs/AGENTS.md` remain unchanged: no
  crate boundary, child DOX boundary, dependency direction or parent index changed.
- No new production dependency, external code copy or live-provider test was added.
  `tempfile` was already in the workspace and is additionally used by non-production
  xtask mutation isolation.
- Release preparation changes package versions and registry asset URLs only;
  existing manifest-owner DOX contracts and child indexes remain applicable.
