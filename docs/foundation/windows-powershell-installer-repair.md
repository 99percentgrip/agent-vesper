# Windows PowerShell installer compatibility repair

## Objective and status

Repair the Windows x86_64 installation path for stock Windows PowerShell 5.1 without requiring PowerShell 7 or relaxing checksum or execution-policy controls. **Implementation and offline regression are prepared; native Windows CI and the user's clean Windows 10/public-main smoke test are NOT YET RUN.** No release or installed user payload is claimed.

## Investigation and root cause

- Inspected complete `scripts/install.ps1`, both invocation forms in `README.md` and `docs/installation.md`, the release matrix/packaging in `.github/workflows/release.yml`, canonical CI, installer and upgrade tests, and Windows command references in documentation. Existing code called `[System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()` without checking for null. The reproduced Windows PowerShell 5.1 Desktop host evaluated that architecture expression to null, and calling `ToString()` produced the observed null-method exception. The type/API's availability and behavior under .NET Framework/Windows PowerShell 5.1 are not a safe prerequisite here; the precise .NET binding reason on the reporter's machine cannot be independently measured on this Linux host. `[Environment]::Is64BitOperatingSystem` and `PROCESSOR_ARCHITECTURE=AMD64` independently establish that this was not unsupported hardware.
- `release.yml` has five explicit packages: `linux-x86_64`, `linux-aarch64`, `darwin-x86_64`, `darwin-aarch64`, `windows-x86_64` (`x86_64-pc-windows-msvc`, `agent-vesper-acp-windows-x86_64.zip`). No Windows ARM64 package exists. The installer always uses the Windows x86_64 zip; unsupported platforms must fail rather than map ARM64 to x86_64.
- Full installer audit: `param` defaults, `.StartsWith`, `[guid]`, temp directory, `Invoke-WebRequest`, checksum read and `Get-FileHash`, `Expand-Archive`, literal-path checks, payload moves preserving co-located state, `Set-Content -NoNewline` CMD launchers, skill manifest, `$HOME`, user/process PATH, native `--version`, bundled driver setup and `finally` cleanup. All constructs are available in Windows PowerShell 5.1. Potential follow-on hazards were IE first-run DOM initialization during `Invoke-WebRequest`, legacy TLS negotiation, malformed/empty checksums, missing ACP binary, and native stderr under `$ErrorActionPreference='Stop'`. Applied `-UseBasicParsing`, TLS 1.2 session negotiation, checksum-format and both-executable preflight, and version-exit checks with the existing driver stderr handling pattern. No code was changed in the Unix installer. `irm | iex` has no downloaded-file policy check; downloaded `.ps1` may be blocked by execution policy. Initial fetch of the raw script happens before installer TLS setup; troubleshooting documents the session-local TLS preflight if required.

## Implementation / files

- `scripts/install.ps1`: Windows/64-bit preflight, native architecture from `PROCESSOR_ARCHITEW6432` under WOW64 or `PROCESSOR_ARCHITECTURE` otherwise; accept AMD64 only. Explicit missing/unsupported errors; original HTTPS release paths and archive SHA-256 comparison retained. WinPS-compatible web fetch, bundle preflight, installed native process status handling.
- `scripts/test_install_windows.ps1`: new network-free synthetic archive with both version-capable executables; mocked downloads, real SHA-256, extraction, launcher execution and PATH behavior; direct file and local script-text-through-`Invoke-Expression` forms; simulated WOW64 and missing/unsupported arch; tampered/malformed checksum failure before install. Restores process and user PATH and removes private temp files.
- `.github/workflows/ci.yml`: separate Windows runner job invokes `powershell.exe` (Desktop 5.1) and `pwsh.exe` (7+) independently. This job is an offline compatibility test, not public-release acceptance.
- `README.md` and `docs/installation.md`: consistent Windows shell/architecture, one-liner, reviewed-file and process-local execution-policy options, TLS and architecture troubleshooting; unchanged Unix command and verified five-target support list.
- `scripts/AGENTS.md`, `.github/AGENTS.md`, `docs/AGENTS.md`, `docs/foundation/AGENTS.md`, `docs/foundation/evidence-index.md`, `docs/settings-and-update-prd.md`: owning contracts and evidence links for the changed installer/CI and S5 updater-install boundary.

## Methods and exact local receipts

Commands executed on Linux in this worktree (which already contained extensive unrelated modifications; those were not reset):

```text
python3 scripts/test_release_gate.py
.......
----------------------------------------------------------------------
Ran 7 tests in 0.001s
OK
sh scripts/test_install_upgrade.sh
Installer upgrade preserves user state and database inode: PASS
sh -n scripts/install.sh scripts/uninstall.sh
# exit 0, no output
python3 - <<'PY'  # PyYAML safe_load of .github/workflows/ci.yml
# ci jobs: ['quality', 'windows-installer', 'supply-chain']
PY
git diff --check
# exit 0, no output
```

An initial broad Markdown link probe failed on an existing percent-encoded path in `docs/foundation/evidence-index.md` (`../Vesper%20bridge/...`); the corrected probe decoded link paths, verified all directly changed user/PRD/report documents and the new index links, and printed `CI YAML, report/index links and installer static guards OK`. This was a checker limitation, not an installer test result. The final `git diff --check` exited 0.

The offline Windows test and public one-liner were **not executable here**: neither `powershell.exe` nor `pwsh.exe` is installed on this Linux machine; no CI run for these changes has settled. The synthetic `--version` checks establish installer/launcher mechanics only, not that a published Agent Vesper binary works on Windows 10. Full `cargo xtask verify`, native Windows CI, real GitHub release download and live app launch are **not passed/claimed**. The source inspection establishes that no new .NET Core-only API, PS7-only syntax, unsupported cmdlet parameter, global policy change or Unix runtime edit was introduced; it does not replace native execution.

## Security, deviations and remaining gates

- The actual release zip is fetched over the existing HTTPS URL and its SHA-256 is compared to the published `.sha256` **before extraction or installation**; absent, malformed and mismatched checksums fail. `AGENT_VESPER_RELEASE_BASE_URL` is an existing test/development override and is not used by the documented command. The shell's TLS 1.2 selection changes only the current process's protocol flags; it does not change execution policy. No `Unrestricted` setting or automatic bypass was added; the optional documented `-ExecutionPolicy Bypass` is process-local for a reviewed file.
- Windows ARM64 intentionally remains unsupported: release matrix does not publish a native package. WOW64 native-arch handling is simulated in CI fixture metadata; an actual 32-bit host process and Windows ARM64 machine remain untested. Windows 10 PowerShell 5.1 real release install, the public raw-main one-liner **after merge**, direct file execution, both real binaries' `--version` and `--help`, and TUI interactive launch require the reporter's clean laptop. CI will test native Desktop and PowerShell 7+ on the hosted Windows image once pushed; until green, compatibility is a target, not a validated result. `main` cannot exercise this unmerged source; release packaging and asset publication are unchanged.
- Readiness effect: focused patch prepared for Windows CI and owner-operated final regression, not yet accepted as successful on the reproduced clean device. Keep S5 Windows acceptance open until exact CI and real-machine receipts are recorded.
