# Native dependency setup reconnaissance

Date: 2026-09-13. Status: reconnaissance COMPLETE; implementation PROPOSED.
Objective: determine how Vesper can prepare dependencies for beginners without
manual package commands or configuration-file editing. No feature PRD owns this
standalone investigation yet.

## Current behavior and evidence

Inspected `scripts/install.sh`, `scripts/install.ps1`,
`crates/vesper-harness/src/web_settings.rs`, `src/sandbox_backend.rs` in that crate,
`docs/installation.md` and the installation DOX contract.

- Ordinary hosted-provider coding does not require Docker. The release already
  includes ACP, TUI, the fetch helper, sandbox supervisor, skills and browser image.
- Both installers invoke `--setup-web-driver`. The native helper verifies the
  bundled archive and runs the container CLI's `load`, then checks image identity.
- Missing/stopped engines still produce instructions to install/start Docker or
  Podman manually. Neither installer provisions an engine or initializes its VM.
- `container_cli()` respects an explicit override, otherwise chooses the first
  installed Docker/Podman executable. It does not first select a healthy engine;
  a stopped Docker installation can mask an available Podman installation.
- Container-backed web tools need a compatible Linux-container runtime. Not all
  sandbox execution requires Docker: the shared default backend is native Linux
  namespaces, subject to real capability checks.
- Voice partly bootstraps itself through bundled uv, but recording tools and OS
  microphone access remain separate prerequisites. Local models need a model
  server and chosen model; Swarm also needs configured embeddings. Project build
  tools depend on the repository. Installing an engine does not satisfy these.

The declared frozen Python checkout was not available at the documented path;
this report makes no new oracle-parity or impossibility claim. Findings above
come from the current Rust tree. No packages or services were installed or started.

## Feasibility and primary sources

The existing import helper and shared host settings provide a useful starting
point. A deterministic native setup service can install prerequisites, manage a
Vesper-owned VM where needed, verify readiness and resume interrupted setup.
This is a design inference, not an already implemented capability.

[Podman installation](https://podman.io/docs/installation) documents Linux
package-manager installation and official macOS/Windows packages. Its macOS
installer is preferred over the community Homebrew package. Linux distro/package
support must be an explicit tested matrix, not arbitrary guessed shell commands.

[Podman machine](https://docs.podman.io/en/latest/markdown/podman-machine.1.html)
documents required VMs on macOS/Windows and commands to initialize, inspect, start
and stop them. Automation removes setup work, not the VM's resource requirements.

[Podman's Windows setup](https://podman-desktop.io/docs/installation/windows-install)
documents WSL2/Hyper-V, administrator-controlled feature activation and restart
steps. Vesper must resume after these OS interactions rather than promise silent
setup on every machine. A Desktop app install alone is not engine readiness.

[Docker's Windows installation guide](https://docs.docker.com/desktop/setup/install/windows-install/)
documents installer flags, engine startup, WSL prerequisites and user acceptance
of its terms. Reuse a working Docker installation; do not silently accept terms
or change its global configuration.

## Proposed user flow

1. After installation or in native Settings, show a **Set up features** page:
   coding readiness, web/browser/isolation readiness, and separately optional
   voice, local-model and worker prerequisites.
2. **Set up web and isolated tools** explains the selected runtime, estimated
   download/disk requirements and any expected administrator/restart step.
3. Detect OS, architecture, supported distro, permissions, virtualization and
   existing engine health. Respect explicit choices and local/remote context;
   do not silently send workspace data to a remote engine.
4. Reuse a healthy compatible engine. If absent, propose Podman through supported
   package sources/installers, using rootless operation where supported. Verify
   package provenance and supported versions. Elevate only the fixed installation
   action, not the whole TUI or coding agent.
5. On macOS/Windows, create/start a named Vesper-owned machine when needed. Persist
   progress and resume after reboot, interruption or lost network; never recreate
   or reset an existing user machine. Starting/stopping a shared engine requires
   respecting its other workloads.
6. Import the already bundled, checksum-verified browser image. Verify engine
   capability, actual isolated execution, browser operation and cleanup using
   controlled probes. Display **Ready** only after those checks pass.
7. Keep feature activation in the existing permission-aware Save changes flow.
   Offer **Retry**, **Continue coding**, and specific OS guidance on failure.
   Subsequent sessions can restart a Vesper-owned stopped runtime under the saved
   setup choice; sleep/wake and engine restarts must recheck readiness.

Normal supported setup should require clicks and unavoidable OS prompts, not
copying terminal commands. Core coding stays usable if optional setup is declined.
Hardware virtualization restrictions, managed-device policies, provider sign-in
and unavailable network access remain real boundaries with explicit recovery.

## Implementation and acceptance scope

Use one shared harness dependency/setup service with native Settings presentation
and equivalent ACP control outcomes. Installers hand off to it rather than keeping
separate dependency logic. Use fixed typed installation plans, bounded subprocesses,
redacted progress and an ownership ledger; no model-generated administrator shell.

Runtime setup must preserve existing engines, containers, credentials, project
state and the skill library. Cleanup may remove only verified Vesper-owned setup
artifacts; uninstallation must not remove a shared engine or unrelated data.

Acceptance requires clean supported Linux/macOS/Windows hosts, including absent
engine, installed-but-stopped engine, working alternative engine, unavailable
virtualization, elevation denied, reboot/resume, failed download, wrong checksum,
wrong architecture, full disk, cancel/retry, sleep/wake and repeated setup. Real
browser/isolation/cleanup probes must pass; mocked package commands alone do not
prove a beginner can finish setup. No such installation acceptance ran in this recon.

A universal installer for every project's toolchain or every skill integration
is not established here. Detect and offer the dependencies of selected features
and the current project; obtain actual provider credentials from the user.

## Verification and DOX closeout

Methods: source reads and targeted `rg` searches, current official documentation,
manual review, relative-link checks and `git diff --check`. No program tests,
provider calls, installer execution, version bump or release. Existing user guides
remain unchanged because the wizard is proposed, not shipped. The foundation
index links this report; owning DOX records its scope and requested beginner UX.
Readiness effect: identified a concrete install/start/health/resume gap and a
bounded implementation direction; no claim of dependency automation delivered.
