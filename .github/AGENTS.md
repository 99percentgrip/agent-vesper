# Migration validation workflows

## Purpose

Own CI definitions and guarded runner preparation used to validate production
migration gates and disposable platform assumptions on hosts unavailable locally.

## Local Contracts

- Canonical, MSRV, five-target and web-driver push/PR triggers exclude only
  README/DOX prose, documentation Markdown and named foundation report/evidence
  companions. Source, fixtures, seed skills, workflow/config/dependency changes
  and `release-objective-provenance.json` still trigger every required gate.
  Keep the same exclusion list in all four workflows. A missing filtered run
  never authorizes a tag; the exact-commit release gate stays fail-closed.

- `prepare-rust-runner.sh` refuses outside GitHub-hosted Linux. It removes only
  unused preinstalled Android/.NET/GHC SDK trees on disposable CI machines,
  records actual free space and preserves the Rust/Node/Python/Docker tool paths.
  Canonical/MSRV retain storage diagnostics on failure. The focused lifecycle
  gate proves exact acceptance linking after the workspace and default-doc graphs
  on both Linux architectures; it never reduces existing verification coverage.
- `prepare-linux-apt.py` is CI-only guarded runner preparation. Preserve mirror
  files and cached mirror+file paths, route architecture-correct Ubuntu archives over HTTPS (ARM64 keeps ports) and set
  IPv4/retry/acquisition bounds for both sandbox and browser dependency setup.
  `test_prepare_linux_apt.py` uses private fixtures and verifies refusal outside
  hosted Linux; no local system package configuration is changed by verification.

- ADR 0028 acceptance regressions are mandatory in canonical, MSRV and five-target
  foundation workflows. Canonical CI also kills two bounded evaluator mutations. The
  existing exact-commit release prerequisite therefore includes these gates; local
  evidence alone cannot authorize a release.

- Canonical CI has a separate offline Windows installer job invoking
  `scripts/test_install_windows.ps1` under native `powershell.exe` and `pwsh.exe`.
  The same pre-tag job also builds all four exact-candidate release executables
  with shipping features, audits normal/delay PE imports to reject external VC/CRT
  runtime dependencies, and installs the real candidate in private state under both
  PowerShell hosts. Publishing repeats the import audit on the staged Windows
  package. Its actual release-profile TUI must also pass signed-out ConPTY
  landing and all-provider setup before publication. Synthetic packages and developer-equipped runners never substitute
  for a real clean Windows release install.
- The five-target prerequisite builds shipping-feature TUI/ACP and runs actual
  signed-out landing, all-provider Settings authentication and cross-host RRC
  ledger controls on every native runner. Relative executable arguments and
  explicit CP1252 redirected Python output exercise the shared path/UTF-8 receipt
  contract on every lane. Windows uses pinned, hash-verified
  Python 3.13 ConPTY; Linux/macOS use stdlib PTYs. Version/help and seeded-key
  tests cannot substitute for this interactive first-launch gate. Native governor
  cases run on all targets; Windows additionally runs a real constrained Job Object
  fixture against the production policy, proving unavailable/unsafe capacity is
  withheld without substituting synthetic lifecycle telemetry.
- Keep the five release-target families explicit in the matrix: linux-x86_64,
  linux-arm64, macos-intel, macos-apple-silicon, windows-x86_64.
- Five-target foundation explicitly invokes ADR 0032 native credential
  persistence acceptance on disposable runners: UUID namespaced synthetic
  entries only, production store, fresh-process reload, rotation and sign-out.
  Windows must prove native storage with no plaintext vault. Default tests keep
  this lane ignored; real credentials and provider calls remain prohibited.
  `verify_native_credentials.py` requires one named passing Rust receipt for
  each parent case; the core fixture likewise requires one exact child receipt.
  Missing, renamed, ignored and zero-match cases fail even when Cargo exits zero.
  Its offline parser/guard tests never invoke Cargo or native credential access.
- Validation workflows must not call live providers or require credentials.
  `release-recovery-acceptance.yml` is a manually dispatched, read-only controlled
  fixture for deterministic complete-matrix red, repeated-fingerprint, focused-green
  and post-publication-main-red evidence. Its selectable synthetic macOS/Windows
  lane is Ubuntu-hosted diagnosis evidence, never native platform proof. Its
  non-failing lanes wait 90 seconds so 20-second controller polling plus native
  API inventory latency can capture the actual partial matrix. `release-recovery-lifecycle-acceptance.yml`
  is the manually dispatched, read-only five-native-runner gate for an exact SHA in
  the controlled public acceptance repository; it proves RRC process-tree
  cancellation, persisted restart identity, controller/adapter/policy regressions
  and fixture-backed repair/retry without publication authority. It also runs
  the isolated synthetic ACP lifecycle, focused native command settlement,
  three additional Windows cancellation
  repetitions, and hermetic voice-pack fixtures before
  the complete production gate retry. These
  fixtures must never replace production exact-commit release gates or intentionally
  break Agent Vesper `main`.
  The tag-triggered `release.yml` workflow is the sole publishing workflow and
  may publish only compiled, checksummed release archives and exact-commit
  tested browser image archives; it must not make
  provider calls.
- Release builds are fail-closed behind exact-commit CI: the tagged commit must
  already have successful `push` runs for `ci.yml`, `msrv.yml`, and
  `platform-foundation.yml`, and `web-driver.yml`. Push the version commit to
  `main`, wait for all four workflows to pass, then create/push its release tag.
- Release archives include `vesper-web-fetch` and the `sandbox_init` supervisor
  beside ACP and TUI, plus the
  matching Linux-architecture CI-tested image under `web-driver/` as
  `image.tar.gz`, `image.sha256`, and `image-id`. Each matrix job downloads
  the successful exact-commit driver artifact selected by the release gate,
  verifies its hash before packaging, and never rebuilds it. Both hosts build
  with Docker, Swarm and Bridge features. The TUI also builds with the VRO-17
  `voice-kokoro` and `voice-flm` features so the shipped binary exposes the
  accepted local voice stack; optional model/runtime assets remain user-installed
  through native Settings and are not bundled. Swarm activation remains default-off in native
  Settings and requires configured embeddings and a permitted backend. Docker/Podman
  supplies the host runtime; native guided setup offers separate confirmed preparation,
  while installer preflight remains import-only and never enables web access.
- Release uploads compare existing asset bytes and fail on mismatch; no
  `--clobber` replacement is permitted. Notes describe the tagged version and
  generated commit changes rather than a fixed historical version narrative.
- VRO-17 release notes state that the release ships the local speech stack;
  third-party cloud STT/TTS are planned future optional integrations, ACP has
  no microphone/audio capability, and no NPU TTS claim is made.
- The default toolchain is pinned to Rust 1.95.0 via `rust-toolchain.toml`
  (with `clippy` and `rustfmt` components); MSRV 1.88.0 is enforced
  independently in `msrv.yml` and the spike workflows.
- `ci.yml` runs the canonical verification gate via `cargo xtask verify`
  (fmt, clippy `-D warnings`, architecture, fixtures per stage, contract
  conformance, GLM/runtime/ACP/sessions verify, and the full workspace test
  suite) plus a documentation-structure check.
- Canonical CI runs the offline POSIX installer upgrade regression to ensure
  bundle replacement preserves co-located user state and database inodes.
- The supply-chain job runs `cargo audit` and `cargo deny --all-features check`
  with pinned tool versions (`cargo-audit 0.22.2`, `cargo-deny 0.20.2`).
- `cargo deny` permits the OSI-approved Boost Software License (`BSL-1.0`),
  required by the native Windows clipboard backend.
- `deny.toml` enforces allowed licenses, bans SQLite crates and wildcard
  dependencies, and treats duplicate versions as warnings (the documented
  Stage 5 baseline) rather than failures.
- Optional compiler-cache installation may fail; the existing health probe then
  selects direct compilation. Every test, fixture and architecture gate remains
  required; cache outages never count as test passes.
- The five-target matrix runs the Stage 4.1 real-process blocker suite with
  bounded timeouts; Linux-only RSS evidence must not be generalized. The
  matrix also runs `vesper-agent`'s shared `command_settlement` behavior on
  every target family so Windows/macOS output draining, timeout/cancellation,
  descendant cleanup, caller-drop cleanup and recovery cannot pass by
  cross-compilation or an empty platform-filtered test binary. The explicit
  RRC host-lifecycle case must likewise run on every family and prove local
  cancellation reaps a real descendant plus a fresh process reloads the same
  persisted epoch/run/job identity. Its exact invocation uses `--lib --all-features`
  to reuse the eligible workspace unit-test build, without relinking unrelated
  default-feature integration binaries; the selected case must still execute.
  The
  Linux sandbox step is stall-proofed in layers: it skips `apt` entirely when
  the runner image already ships `bwrap`; when it must install it REWRITES
  (never deletes) `/etc/apt/apt-mirrors.txt` to the canonical
  HTTPS `archive.ubuntu.com` / `security.ubuntu.com` mirrors and rewrites known
  Ubuntu HTTP source URLs — the runner's pre-populated apt lists fetch
  through the `mirror+file:` scheme, so deleting the file breaks even cached
  `.deb` downloads — forces IPv4, hard-kills every apt call with coreutils
  `timeout` (apt's own `Acquire::Retries`/`Timeout` options did NOT bound a
  stalled azure-mirror transfer — observed 2026-08-19: 56 silent minutes →
  60-minute job-timeout cancellation), and caps the step at
  `timeout-minutes: 10`. A missing bwrap fails the bwrap tests truthfully;
  they are never silently skipped.
- Canonical, MSRV, platform and web-driver Cargo checks disable incremental
  artifacts and use line-table debug symbols. Preserve every test and gate;
  hosted-runner disk exhaustion must not be bypassed by reducing coverage.
- CI validates Stage 5 coverage, read-only session/testkit conformance, and
  writer/SQLite architecture gates on all five target families.
- `release.yml` checks exact-commit CI through the Actions API using only the
  repository `GITHUB_TOKEN` with `actions: read`; no build or publication job
  starts when any required latest main-push run or its attempt-specific jobs
  are missing, skipped, running or unsuccessful. `scripts/release_gate.py`
  checks pagination/provenance and changed attempts; publication rechecks
  prerequisites and uses the original admitted driver artifact run. It also
  declares a single concurrency group `release-pipeline` with
  `cancel-in-progress: true` so a new tag-push (or `workflow_dispatch` with
  a `tag` input) cancels any stuck prior run (e.g. a phantom-queued run
  left behind by a runner-image `Service Unavailable` failure). Without
  this guard, GitHub Actions blocks all subsequent release runs behind the
  stuck one indefinitely. `workflow_dispatch` is the manual recovery path
  when a tag is already pushed but no run fired: `gh workflow run
  release.yml --ref <tag> -f tag=<tag>`.

- Canonical CI also runs native voice lifecycle acceptance using private temporary
  state, fake recorder/inference helpers and pinned test-only numpy 2.5.3. Production
  PCM slicing and TUI input/rendering remain real; no microphone/provider is used.
  The timed case exceeds 90 seconds; a six-minute step bound and forty-minute
  quality-job bound include this additional acceptance work.

## Verification

- `.github/verify_native_voice.py` requires a disposable native runner, installs
  pronunciation prerequisites, isolates application state, and runs production
  speech-dependency setup plus actual Kokoro pack verification and both real PCM
  signal cases. All five prerequisite lanes require these named receipts before
  tagging. Its dependencies use fixed integrity pins; no microphone, speakers,
  provider or user credentials are exercised. OS prompt/device acceptance remains
  separate. The 90-minute lane budget includes the 20-minute bounded speech step.

- Validate YAML syntax locally where tooling exists.
- Run `cargo xtask verify` and the MSRV workflow; record exact-SHA terminal
  conclusions rather than inferring success from workflow registration.
- Require all five native target jobs to settle; focused lifecycle evidence
  cannot replace complete production prerequisite or publishing gates.

## Child DOX Index

- `verify_native_credentials.py` and `test_verify_native_credentials.py` —
  explicitly hosted native credential acceptance and offline exact-receipt guards.

- `workflows/ci.yml` — pull-request canonical gate (`cargo xtask verify`),
  documentation-structure check, and the supply-chain job (`cargo audit` +
  `cargo deny check`).
- `workflows/msrv.yml` — dedicated Rust 1.88.0 foundational verification with
  per-stage fixture coverage.
- `workflows/web-driver.yml` — native x86_64/arm64 image builds and gated
  real pipe-browser tests, native dependency browser/cleanup readiness, and real scoped Hive lifecycle acceptance (three
  independent workers, permissioned command continuations, scale/replacement
  and verified cleanup) using the same immutable built image. The explicit Hive
  gate fails on unavailable isolation; it never counts a skipped body as passing.
  The shared service gate also checks disabled/missing-embedding/cancelled
  admission, project copies, captured cognition, native history and cancelled
  cleanup. The real ACP process gate exercises Settings activation,
  configured loopback embeddings and provider transport, three overlapping
  native workers, grounded synthesis and final artifact-report delivery.
  The TUI gate covers both scope modes through native command/task/history
  composition. Shared-scope gates verify a single real provision/teardown,
  denied sibling writes, detached-child reaping and active service shutdown.
  Pinned Playwright 1.51.1 drives real browser submission through the
  shared Lens executor into the native worker's captured provider continuation
  on both Linux architectures. Browser setup failure fails the gate.
  A separate Ubuntu 22.04 job executes the real namespace supervisor Hive gate
  plus direct private-root/capability/pipe-timeout acceptance, without changing
  host security policy; unavailable isolation fails the job.
  Preserves the exact tested image archives and
  immutable image IDs. Release downloads those artifacts, never rebuilds
  untested images after tagging. Public release assets avoid requiring a
  separate container-registry credential or visibility change.
- `workflows/platform-foundation.yml` — five-target production-foundation and
  eligible spike matrix.
- `workflows/foundation-spikes.yml` — five-target disposable spike test matrix.
- `workflows/release-recovery-acceptance.yml` — manually dispatched, non-publishing
  RRC fixture matrix with stable synthetic causal output and a secret canary.
- `workflows/release-recovery-lifecycle-acceptance.yml` — manually dispatched,
  read-only exact-SHA native five-target cancellation/restart matrix for the
  controlled public acceptance repository.
- `workflows/release.yml` — tag-triggered ACP+TUI archive packaging and GitHub
  Release publication for the registry and installers; archives also bundle
  the repo `skills/` seed library seeded by the installers into
  `~/.agent-vesper/memory/`; the registry continues
  to launch only the ACP binary from the shared bundle.
