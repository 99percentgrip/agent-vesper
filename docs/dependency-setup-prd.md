# Guided dependency setup

Status: implemented and released in v0.22.5 as preview; clean-platform installation acceptance incomplete.
Baseline: `89bdc07`.

## Objective

A beginner can prepare web/browser and container isolation from native Settings,
without editing configuration or finding a separate browser image. Core coding
remains usable when optional setup is declined. Preserve the complete skill library,
existing runtimes, workloads, credentials and Alex's installed Vesper binaries.

## Required behavior

1. Read-only discovery checks actual engine health, platform and local endpoint.
   An unavailable Docker must not hide working Podman. Explicit overrides remain
   authoritative; remote endpoints are never silently selected.
2. Settings offers a separate confirmed setup operation, with actual progress,
   retry and return-to-coding. Explain package downloads, VM resources and OS prompts.
   Setup does not implicitly enable web permissions or save the Settings draft.
3. Missing engines use fixed supported package plans: distro packages on Linux,
   checksum-pinned official Podman installers on macOS/Windows. Never execute a
   model-generated installer command or elevate the agent itself.
4. VM setup owns a named machine, leaves other machines/connections unchanged,
   and resumes by inspecting current state. OS-required restarts are explicit.
   Retry must preserve existing progress and never reset a machine.
5. Verify the bundled image, run real contained browser and isolation probes, and
   verify cleanup before reporting ready. Failure remains failure.
6. Persist the selected runtime separately from workspace settings. Setup and actual
   web/swarm/sandbox execution use the same connection. Saved managed runtime
   startup is bounded; health is checked again after sleep/restart.
7. Both native hosts expose the shared setup service; ACP stdout stays protocol-only.
   Installers point to native setup without performing unapproved OS installation.
8. Explain separate optional voice, local-model, embeddings and project dependencies;
   container setup must not claim to satisfy those requirements.

## Acceptance

Offline tests exercise selection, overrides, remote refusal, fixed plans, malformed
state, failed commands, checksum refusal, retry/idempotence and cleanup failure.
Native UI checks cover confirmation/decline, progress, errors and unchanged drafts.
Actual clean Linux/macOS/Windows installation, elevation denial, reboot/resume,
network/disk failures and sleep/wake require platform evidence; mocks do not close
those gates. All missing or failed items remain visible in the execution report.

## Evidence

- [Reconnaissance](foundation/dependency-setup-recon.md)
- [Implementation and acceptance](foundation/dependency-setup-execution.md)

- [Release status](foundation/v0.22.5-release-execution.md)

## MCP session repair evidence

[Persistent MCP execution report](foundation/mcp-session-lifecycle-repair.md)
records source-level browser continuity and host wiring. It does not close native
Settings setup, clean-platform installation or release acceptance gates.
