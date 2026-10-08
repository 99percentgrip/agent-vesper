# Windows missing runtime package repair

## Objective and status

Repair Alex's reported public Windows installer failure, preserve a working
installation on failed downloaded payloads, and require real package evidence
before the integrated corrective v0.24.9 release. Implementation and scoped local
checks are complete; native Windows compiled-package checks and user-device retest
remain open. The publication-observation repair remains part of this candidate.

## Incident and method

The supplied screenshot shows seeding followed by `installed ACP version check
failed (exit -1073741515)`. That exit is 0xC0000135, STATUS_DLL_NOT_FOUND
([Microsoft deployment troubleshooting](https://github.com/MicrosoftDocs/cpp-docs/blob/main/docs/build/troubleshooting-c-cpp-isolated-applications-and-side-by-side-assemblies.md)).
It is an executable loader failure after download, not a successful installation.
The screenshot does not identify the exact missing DLL or Windows build.

Downloaded the immutable v0.24.8 Windows archive and checksum for read-only
inspection, without running an installer or changing Alex's installation.
SHA-256 matched. GNU objdump and the new PE reader independently agree on all
four import sets: ACP has 19 imports, TUI 20, sandbox supervisor 9 and web-fetch
16. Every executable imports VCRUNTIME140.dll and CRT API libraries; the archive
contains no DLLs. This confirms an unhandled package dependency and is consistent
with the user's missing-DLL failure. The exact missing DLL on that computer is
not independently observed.

The earlier PowerShell fixture used compiled C# stand-ins. A real v0.24.7 probe
also ran on a developer-equipped Windows Server host. Those receipts did not
prove the newly shipped package could start on a clean user computer. Their
recorded successes remain valid only for those scopes; the user result is failed.

## Corrections

- Configure only x86_64-pc-windows-msvc builds with `+crt-static`, preserving
  rust-lld and Linux/macOS settings. Rust documents this CRT linkage mode in
  [the Rust Reference](https://doc.rust-lang.org/reference/linkage.html).
  Optional voice runtimes remain dynamically loaded through their setup ports.
- Audit normal and delay PE imports for all four shipped executables. Missing,
  malformed or runtime-dependent executable inputs fail closed. No publication
  can bypass the staged import check.
- Extend the canonical pre-tag Windows job to build the exact candidate with
  shipping features, audit its dependencies, and package/install the actual
  binaries under PowerShell 5.1 and 7 in private state. Both native hosts must
  pass --version and --help. No provider or optional runtime is contacted.
- Validate downloaded ACP/TUI executables before replacing installed payloads,
  launchers, PATH or memory. Diagnose 0xC0000135 explicitly. Add a native fixture
  returning that exact NTSTATUS and prove the prior executable remains intact.

## Commands and exact evidence

- `gh release download v0.24.8 --repo 99percentgrip/agent-vesper` for the named
  Windows archive/checksum: downloaded; checksum verified.
- `objdump -p` on all four extracted executables: imports recorded in the
  [package evidence](2026-10-08-windows-missing-runtime-package-repair-package-evidence.json).
- `python3 scripts/windows_package_audit.py <published-executables>`: exited 1
  as expected, naming VCRUNTIME140.dll and CRT imports; this is red package proof.
- `python3 scripts/test_windows_package_audit.py`: 7 passed, zero failed.
- `python3 scripts/test_release_gate.py`: 10 passed, zero failed.
- Both changed workflow YAML files parsed; rustc target cfg reports crt-static;
  git whitespace validation passed. These checks do not compile Windows binaries.
- PowerShell is unavailable on this Linux host. The changed native preflight and
  real-package installer regressions have not been executed locally and require
  the new hosted gate. No skipped body is represented as a pass.

[Evidence manifest](2026-10-08-windows-missing-runtime-package-repair-evidence.json)
and [raw receipts](2026-10-08-windows-missing-runtime-package-repair-receipts.tar.gz)
retain the failed package and exact scoped checks.

## Native RRC operation and deviations

The prior corrective epoch accepted /release 0.24.9 through the native TUI.
Resource pressure deferred version preparation. Four inactive task-owned binary
copies were hash-verified and relocated off tmpfs, preserving the copies and
freeing about 1.4 GiB of RAM. The governor autonomously resumed without another
release request. Version preparation passed; workspace verification began.
After the new Windows report, configured Ctrl+C cancelled the native controller
and settled its mutation journal. No candidate push or tag existed. The temporary
host exited via /quit. The original user TUI and installation were untouched.
This was deliberate integration of a new blocking defect, not a failed CI retry
or automatic repair proof. The next native request must reconcile the new source
and perform fresh exact-version gates before publication.

## Files and DOX

Changed target build policy, canonical/publishing workflows, Windows installer,
installer fixture, new package audit/parser tests and real-package acceptance.
Updated their nearest `.cargo/`, `.github/` and `scripts/` contracts, foundation
ownership/index, Settings/update PRD, RRC status/report and canonical objective
provenance in the same code candidate. Root/docs parent ownership and indexes
were reviewed unchanged: no new subtree or global permission is introduced.

## Unresolved items and readiness effect

Native Windows static-link compilation, actual package import audit, both
PowerShell real-package installs, all complete exact-SHA matrices, producing
publication and registry/report closeout are required before release completion.
Alex's device retest remains separate and cannot be fabricated by a hosted runner.
The v0.24.8 immutable assets are not replaced. No local installation is authorized.
Neither this repair nor old green checks establish universal bug-free RRC parity.

## Subsequent exact-candidate hosted result

The first v0.24.9 candidate c076ebc7 completed all eight native gates. Hosted Windows shipping compilation and the CRT import audit passed; both offline PowerShell host regressions passed. Actual Desktop 5.1 installation succeeded, then the real-package check incorrectly required stdout although ACP metadata uses stderr. The real PowerShell 7 body was skipped. The complete prerequisite matrix settled nine passed / three failed, including two Linux dependency acquisition failures; no tag was created. The [causal diagnosis follow-up](2026-10-08-rrc-ci-causal-diagnosis-repair.md) preserves the native stop and integrates the check/setup corrections before any release. The laptop retest remains open.

## Final hosted and publication follow-up

The [v0.24.9 closeout](release-v0.24.9-closeout.md#corrective-implementation-and-observed-final-delivery)
supersedes this report's prerelease hosted/publication-pending boundary for final
source `82b5e4183fa74e012ee4c3f36b7a9715d22d02b8`: all eight native gates,
all twelve exact-SHA prerequisite jobs and all seven producing jobs passed.
Actual Windows package installation passed in PowerShell 5.1 and 7; the published
ZIP checksum/import audit passed. Native RRC delivered its final summary and
reached Complete/Idle with Registry/report readback verified. Earlier failed
candidate evidence remains preserved. Alex's Windows10 laptop retest and
live-model coding repair effectiveness remain outside this acceptance.
