# Native Settings and update usability repair

Status: implemented and Linux-verified; native Windows updater acceptance remains
open. Owner: native host composition.
User directive: 2026-09-12 screenshot and workflow audit.

## Required behavior

| ID | Requirement | Acceptance evidence |
| --- | --- | --- |
| S0 | Every fresh native launch reaches the welcome screen without credentials. Providers is the first Settings category; every registered adapter's authentication is reachable, and Back keeps the host open. Linux, macOS Intel/Apple Silicon and Windows run the same private signed-out process acceptance. | `first_launch_pty.py`, authentication process fixture and five-target native gate. Current hosted results belong in the repair report. |
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

[Native first-launch and RRC workspace repair](foundation/2026-10-08-native-first-launch-and-rrc-workspace-repair.md)
tracks the user-reported Windows landing/authentication trap and the cached
verification binary selecting an old release worktree. Earlier seeded-key or
version/help checks do not certify S0. Current evidence must match the exact source
and native platform, including signed-out startup.
The first v0.24.10 matrix passed twenty signed-out native welcome/provider cases
and Windows release-profile/private-install checks; later fixture transport/receipt
failures blocked that candidate. The corrected commit `4426004d` passed all five
native lanes, including every signed-out provider/authentication route, and was
released as v0.24.10. Original Windows 10 laptop retesting remains unobserved;
hosted native acceptance does not replace that device observation.


[Windows missing runtime package repair](foundation/2026-10-08-windows-missing-runtime-package-repair.md)
records the failed user installation after v0.24.8. Its unbundled VC runtime
imports were not covered by synthetic installer fixtures or a developer-equipped
host. Static CRT linkage, preflight before replacement and real exact-candidate
package checks are implemented. The [v0.24.9 closeout](foundation/release-v0.24.9-closeout.md#windows-package-acceptance)
records both real-package PowerShell 5.1/7 installs, published ZIP import/checksum
audit, complete exact-source matrices and publication as passed. The original
Windows10 device retest remains required; earlier receipts retain their recorded scope.

[Windows installer compatibility report](foundation/windows-powershell-installer-repair.md)
tracks the public installation/S5 installer repair; native Windows Server 2025
PowerShell 5.1/7+ CI and real v0.24.7 installer smoke passed, but clean Windows 10
acceptance and the changed-source release gates remain open, not completion proof.
[Patch-release execution](foundation/windows-installer-v0.24.8-release-execution.md)
retains the failed PR foundation gate and pending exact-main publication checks.

[Execution report](foundation/settings-and-update-execution.md) owns exact commands,
results, deviations and unexecuted platform acceptance. A requirement is not marked
complete by the existence of code or this table.

[Release execution](foundation/v0.22.2-release-execution.md) tracks the authorized
0.22.2 publication separately from the user-reserved installation test.

[Front-page refresh](foundation/readme-refresh-execution.md) records user-facing
publication of the shipped controls and the subsequent user-supplied Linux updater
acceptance in the v0.22.3 release report.


### Corrective Windows authentication persistence boundary

The v0.24.10 original-laptop device-login save failure exposes a separate gap
from successful first-launch/provider navigation. [Windows credential persistence
repair](foundation/2026-10-08-windows-credential-persistence-repair.md) owns the
ADR 0032 fix and required native save/restart/rotation/sign-out acceptance.
The first-launch receipts do not certify this behavior; verification is pending.
