# Guided dependency setup execution

Date: 2026-09-13. Status: implemented and locally verified; platform acceptance
incomplete; NOT released. Owning [PRD](../dependency-setup-prd.md),
[reconnaissance](dependency-setup-recon.md), [verification receipt](dependency-setup-verification.json).
Baseline: `89bdc075cb725e5982408521dfb390ce27896b22`; separate
`feat/guided-dependency-setup` checkout.

## Objective and result

Prepare optional web/browser and container dependencies from native Settings,
while keeping ordinary coding available and preserving existing user state.

- Settings → Web tools → **Set up features / repair** offers separate confirmation,
  actual phase progress, cooperative stop and retry. Setup does not save web toggles.
- The shared service detects healthy local engines, starts an existing local
  runtime when possible, or installs Podman using fixed package plans. A stopped
  Docker executable no longer masks a working Podman engine.
- Fresh desktop Podman setup can create a named `agent-vesper` machine. Saved
  intent supports retry without reset. Only a previously verified owned machine
  can auto-start on later tool use; runtime paths never install packages/create VMs.
- Setup checks the complete browser bundle before OS installation, imports its
  verified image and checks contained browser execution and cleanup before Ready.
- Executable and connection are loaded consistently. Sandbox probes, execution,
  pipes and recorded teardown preserve the connection; explicit runtime/image
  overrides retain precedence. Automatic setup refuses remote contexts.
- ACP `/web prepare` previews; `/web prepare confirm` streams progress and terminal
  outcomes and observes cancellation between OS transactions. Both binaries expose
  pre-provider `--setup-features --confirm`. ACP stdout remains protocol-only.
- Installers retain import-only preflight and direct users to native guided setup.
  README/user guides distinguish unreleased source behavior from released behavior.

## Files and ownership

- `crates/vesper-harness/src/dependency_setup.rs`, its tests and CLI fixture own
  setup, OS plans, serialization, preferences, health, recovery and readiness.
  `web_settings.rs`, `web_runtime.rs`, `swarm_service.rs`, `sandbox_backend.rs`
  compose that service. `Cargo.toml`/lock add the already-used pinned `fs2` for
  cross-process setup locking.
- `crates/vesper-sandbox/src/docker.rs` preserves explicit Podman connections.
- TUI `main.rs`, `settings_host.rs`, `web_hub.rs` and terminal tests own presentation.
  ACP `lib.rs` and `dependency_setup_controls.rs` own protocol progress/cancellation.
- `scripts/install.sh`, `install.ps1`, README and user guides update the setup route.
  `.github/workflows/web-driver.yml` adds real dependency readiness to both native
  image jobs; that new CI step has not yet run remotely.
- Root and affected crate/app/scripts/docs/workflow DOX contracts record ownership
  and the explicit setup-only exception to ordinary harness workspace/network I/O.
  No skill, provider adapter or permission policy was changed.

## Verification

All Cargo commands used `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
CARGO_INCREMENTAL=0`.

- **`cargo xtask verify` passed:** 2,315 workspace tests, zero failures, 36 explicitly
  ignored tests; doctests, Clippy with warnings denied, architecture, naming,
  fixtures/contracts/provider/runtime/ACP/session gates, and all 23 acceptance cases.
- **Rust 1.88 check passed:** `cargo +1.88.0 check --locked -p vesper-harness
  -p vesper-sandbox -p agent-vesper-tui -p agent-vesper-acp --all-features`.
- **Both binaries built:** `cargo build -p agent-vesper-tui -p agent-vesper-acp --all-features`.
- **Native terminal tests passed:** `dependency_setup_pty.py` checks confirmation,
  decline, failure and retry; `settings_pty.py` checks grouped saves/discards,
  permissions and restart persistence. Both use isolated roots and no provider prompt.
- **ACP process controls passed:** previews and repeated failures preserve workspace
  state, dispatch no provider request, and stream both terminal errors after progress.
- **Real Linux readiness passed:** Fedora 44 x86_64, Podman, isolated HOME/config/
  vfs storage under a dedicated `/var/tmp/vesper-dependency-*` directory. Used the
  v0.22.4 bundle image
  `sha256:36380fe2427daf19a431991a85a521c266f67a145cbb2211d76c0372a6f14d2e`.
  The actual Chromium DOM was returned from a read-only, network-disabled container
  without a workspace mount; subsequent container listing was empty.
- **Real native setup passed twice:** `agent-vesper-acp --setup-features --confirm`
  verified/imported the bundle, saved verified runtime preferences, left the workspace
  untouched, produced no stdout and left no containers. Repetition remained successful.
- **Real Settings success passed:** the TUI reached browser readiness using the same
  isolated runtime; Discard changes retained separately confirmed runtime preparation
  while leaving web preferences unsaved.
- **Installer preservation passed:** `sh scripts/test_install_upgrade.sh` retained
  fixture user state and database inode. Changed links, YAML syntax and whitespace passed.

The receipt records source hashes and command-log hashes. Real-runtime receipts:
`/tmp/vesper-dependency-real.log`, `/tmp/vesper-dependency-native.log`, and
`/tmp/vesper-dependency-real-pty.log`. These are runtime/browser checks, not proof
that a clean OS can install its dependencies. No live provider evaluation ran.

## Defects found during verification

Missing-bundle preflight originally ran after engine discovery; it now runs first.
ACP's final-body fallback could suppress the terminal setup error after progress;
the outcome now uses the same stream and tests assert both repeated errors.
A linker crashed during an initial acceptance run on the size-limited temporary
filesystem. Moving only this checkout's build cache to disk allowed all 23 cases
and the complete verification rerun to pass. Source compiler errors during initial
implementation were corrected; failed attempts are not counted as passing evidence.

## Package provenance

GitHub API metadata inspected on 2026-09-13:
[Podman v6.1.1](https://github.com/podman-container-tools/podman/releases/tag/v6.1.1)
(macOS arm64, Windows amd64) and
[Podman v5.8.3](https://github.com/podman-container-tools/podman/releases/tag/v5.8.3)
(macOS amd64). Exact asset names and SHA-256 digests are pinned in the service.
Linux uses existing distro package repositories. No OS package installer ran on
Alex's machine.

## Remaining acceptance and readiness

- Clean Debian/Ubuntu/Fedora package installation and graphical authorization denial
  remain unexecuted. Linux authorization currently needs pkexec and its agent.
- macOS/Windows installer execution, virtualization restrictions, reboot/resume,
  stopped desktop runtimes, real sleep/wake, full-disk and interrupted-download
  acceptance remain unexecuted. Source implementations and fixtures do not close them.
- Multiple existing Podman machines without an identifiable default require choosing
  a connection in Podman; none is reset or silently selected.
- An OS installer descendant may outlive a timed-out launcher. Readiness is refused;
  no forced rollback/reset is attempted. Cancellation waits for the current transaction.
- Voice recording, local models, embeddings and arbitrary project toolchains retain
  separate requirements. Container readiness does not claim to install those.

Ready for review and clean-platform validation; not a universal dependency-automation
completion claim. No version bump, release tag, registry update or local Vesper update.

## Preservation and DOX closeout

The original checkout remains at `5cf5835f78d8e779443188c3490172d7736a589a`.
All ten pre-existing dirty file hashes, five protected GLM files and 488 skill-library
files match their baselines. Installed executables report v0.22.4 and differ from
the older pre-release baseline; this task ran no installer or copy against them.
Affected owning docs and parent contracts were updated; existing child boundaries
remain unchanged. Skill/provider/policy DOX files were intentionally unchanged
because their sources and contracts were not modified.
