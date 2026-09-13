# Security policy refresh

Date: 2026-09-13. Status: COMPLETE. Scope: documentation correction requested by Alex.

## Objective and findings

Update the GitHub Security page. Its Stage 1 warning and unfinished Python
migration language were obsolete relative to the published native TUI/ACP release.
The policy-denial rule remains valid in `crates/vesper-policy/src/lib.rs` and
`firewall/mod.rs`. The old linked reconnaissance describes the frozen Python
source, so it is not presented as the current user-facing security policy.

## Changes and verification

- `SECURITY.md`: replace obsolete status and migration tasks with current product
  scope, private reporting guidance, permissions, provider data flow and update links.
  Do not invent a security email, support commitment or security certification.
- `AGENTS.md`: record the user's lightweight verification preference for prose fixes.
- `evidence-index.md`: link this record. There is no owning feature PRD for this
  standalone editorial correction.
- Read the root/documentation/foundation contracts, current README, security policy,
  reconnaissance and policy definitions. Review the diff, local link targets and
  `git diff --check`. No program tests, provider calls, version bump or installer.
- Commit with `[skip ci]` to honor the explicit no-test request for this text-only
  update. This does not waive the exact-commit gates for future software releases.

## DOX and readiness

Documentation and foundation ownership remain unchanged; their AGENTS files need
no edits. Historical reconnaissance remains intact as historical evidence.
The public Security page now reflects the shipped product. This is not a new
security audit or a change to program behavior. No unresolved editorial item.
