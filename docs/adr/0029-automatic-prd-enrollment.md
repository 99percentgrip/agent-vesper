# ADR 0029: Automatic PRD enrollment after native opt-in

Status: accepted. Date: 2026-09-12.

## Decision

Refine only the enrollment workflow of [ADR 0028](0028-native-implementation-acceptance.md)
to implement the user's explicit request to remove mandatory Settings path entry.

- Native Enforced completion ON (or ACP `/settings acceptance on`) may save an
  empty PRD path. This means enrollment pending, never acceptance disabled.
- Both hosts capture the originating user request outside provider-editable
  history. The opt-in completion port advertises `acceptance_enroll(prd)` and
  instructs the agent to recognize an existing PRD or write the complete requested
  specification before implementation. Ambiguous scope requires clarification.
- The enrollment tool is a scoped mutation under the ordinary permission gate.
  It confines the path, captures original sources, and requires a separate
  read-only reviewer to compare the candidate against the captured user request.
  The native reviewer must actually read source. Refusal, malformed output,
  missing user request, changed PRD, or cancellation cannot admit scope.
- On admission, the original PRD contract is prepared and its path is remembered
  in workspace acceptance preferences. Enrollment may happen once. The model
  cannot replace scope, turn off enforcement, submit receipts, or certify success.
- Pending enrollment blocks completion. Once enrolled, all original ADR 0028
  evidence, review, bounded continuation, cancellation, cross-host and snapshot
  constraints remain active. Restart reopens the remembered PRD without receipts.
- Direct, VRO, ReAct and Swarm retain the same parent completion port. Both hosts
  use `activate_for_prompt`; native status/control commands do not invoke models.

## Compatibility and security

Existing explicit PRD settings and `/acceptance start|revise|resume|stop` remain
supported. Existing saved JSON needs no migration. Automatic enrollment changes
scope discovery, not evidence trust. Model selection and independent semantic
review remain fallible; fixture tests do not establish live-model reliability.
Opt-in explicitly authorizes remembering the admitted path, while the tool still
honors execution permissions. Default loading creates no workspace files.

## Verification

The exact named `cargo xtask acceptance` gate includes automatic enrollment with
real Rust verification and invalid enrollment without state writes. Native ACP
controls and TUI Settings are tested with isolated roots. See the
[repair PRD](../settings-and-update-prd.md) and
[execution report](../foundation/settings-and-update-execution.md) for current
results and platform limitations. No provider-specific runtime is introduced;
MSRV remains 1.88 and production remains safe Rust.
