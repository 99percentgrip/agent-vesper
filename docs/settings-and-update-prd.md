# Native Settings and update usability repair

Status: implemented and Linux-verified; native Windows updater acceptance remains
open. Owner: native host composition.
User directive: 2026-09-12 screenshot and workflow audit.

## Required behavior

| ID | Requirement | Acceptance evidence |
| --- | --- | --- |
| S1 | Acceptance, Swarm and Providers use the active theme and shared native menu geometry. | Theme buffer tests; isolated native Settings PTY. |
| S2 | Ordinary Settings form one draft. Leaving offers Save changes, Discard changes and Keep editing; submenu navigation never saves. Providers retain their explicit confirmation. | Native PTY exercises all three outcomes; grouped-save failure regression. |
| S3 | Primary/auxiliary model, reasoning, generation, mixture, permission and session mode survive an explicit save and restart, and reach execution configuration. Provider values are validated against the current adapter. | Saved-choice restore tests, turn-configuration tests, PTY, both-host LM Studio HTTP-body regression. |
| S4 | Enforced completion can be enabled without entering a path. The agent recognizes the task's PRD, independently checks it against the captured user request, enrolls it, and remembers the path. Scope replacement and verification remain protected. | Shared AgentLoop enrollment/repair/test regression, rejection tests, native ACP controls, `cargo xtask acceptance`. |
| S5 | Check for updates reports the release and offers installation consent. Declining does nothing; confirmation invokes the shipped checksum-verifying installer with the selected version. Installation progress/failure and restart guidance are visible. Windows waits for the running host to exit. | Release/command tests, POSIX installer preservation test; platform execution limits in the report. |

## Boundaries

- The Settings draft and welcome screen are terminal-specific. ACP retains its
  native controls and shares enrollment and provider execution behavior.
- Theme and ordinary saved choices are user-wide; provider choices are partitioned
  by provider. Web, Swarm and acceptance activation remain workspace-scoped.
- Driver import and provider authentication are explicit setup actions. Discarding
  ordinary settings cannot undo an already imported image or a saved credential.
- Automatic PRD selection receives independent review; neither selection nor
  saved settings is a completion receipt. Existing Rust collector/platform limits
  still apply. [ADR 0029](adr/0029-automatic-prd-enrollment.md) refines enrollment.
- No release, installation into the developer's home, live provider call, or public
  update download is required for foundation verification.

## Execution

[Execution report](foundation/settings-and-update-execution.md) owns exact commands,
results, deviations and unexecuted platform acceptance. A requirement is not marked
complete by the existence of code or this table.

[Release execution](foundation/v0.22.2-release-execution.md) tracks the authorized
0.22.2 publication separately from the user-reserved installation test.

[Front-page refresh](foundation/readme-refresh-execution.md) records user-facing
publication of the shipped controls and the subsequent user-supplied Linux updater
acceptance in the v0.22.3 release report.
