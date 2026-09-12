# GitHub front-page refresh execution

Date: 2026-09-13. Status: documentation complete.

## Objective

Refresh the repository front page for the released native UI and updater while
keeping installation, first use, capabilities, dependencies, uninstall and guide
links easy to find. Owning scopes: [output](../output-visual-upgrade-prd.md) and
[Settings/update](../settings-and-update-prd.md).

## Methods and files

- Read root, documentation and foundation DOX contracts, README, owning PRDs,
  installation/usage guides and updater controls. Inspect installer checksum and
  preservation behavior with `rg`; no installer execution.
- Update `README.md` with native update steps, Settings save choices, automatic
  requirements enrollment, activity presentation and an expandable existing Nord
  renderer example. Preserve provider prerequisites, install/uninstall commands,
  dependencies and user-guide links. The image is labeled sample renderer output.
- Append user-supplied Linux update/restart evidence to the v0.22.3 release report.
- Replace the expired root 0.22.2 preservation instruction with the continuing
  rule that release authorization does not authorize local installation.
- Link this report from the evidence index and both owning PRDs.

## Verification and evidence

- All 32 checked README/report/PRD relative links and heading anchors resolve; all four documented
  installer/uninstaller URLs map to existing scripts. Existing setup instructions
  agree with `docs/installation.md`, `docs/using-vesper.md` and native updater labels.
- `git diff --check` passes. Documentation-only changes require no Rust test run.
- The existing renderer PNG was visually inspected before embedding. It is a
  renderer fixture, not a claim of a live provider session or pixel identity.
- Alex’s five screenshots show consent, download, installation progress, completion
  and a reopened 0.22.3 welcome screen. Attribution and scope are preserved in the
  release report; no new independent local installation was performed.

## Deviations, unresolved items and readiness

No code, version bump, release tag or local installation changes. Windows/macOS
interactive acceptance and Alex’s coding-output comparison remain unverified.
The front page now describes the shipped update workflow and terminal output.

## DOX closeout

Root preference updated to remove its expired version-specific restriction.
Documentation and foundation AGENTS remain unchanged: their existing front-page,
report and evidence-index ownership covers these edits; no child boundary or
index entry changed. PRDs and the evidence index link the report.
