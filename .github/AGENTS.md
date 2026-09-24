# Migration validation workflows

## Purpose

Own CI definitions used to validate production migration gates and disposable
platform assumptions on hosts unavailable locally.

## Local Contracts

- ADR 0028 acceptance regressions are mandatory in canonical, MSRV and five-target
  foundation workflows. Canonical CI also kills two bounded evaluator mutations. The
  existing exact-commit release prerequisite therefore includes these gates; local
  evidence alone cannot authorize a release.

- Keep the five release-target families explicit in the matrix: linux-x86_64,
  linux-arm64, macos-intel, macos-apple-silicon, windows-x86_64.
- Validation workflows must not call live providers or require credentials.
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
  cross-compilation or an empty platform-filtered test binary.
  The
  Linux sandbox step is stall-proofed in layers: it skips `apt` entirely when
  the runner image already ships `bwrap`; when it must install it REWRITES
  (never deletes) `/etc/apt/apt-mirrors.txt` to the canonical
  `archive.ubuntu.com` mirror — the runner's pre-populated apt lists fetch
  through the `mirror+file:` scheme, so deleting the file breaks even cached
  `.deb` downloads — forces IPv4, hard-kills every apt call with coreutils
  `timeout` (apt's own `Acquire::Retries`/`Timeout` options did NOT bound a
  stalled azure-mirror transfer — observed 2026-08-19: 56 silent minutes →
  60-minute job-timeout cancellation), and caps the step at
  `timeout-minutes: 10`. A missing bwrap fails the bwrap tests truthfully;
  they are never silently skipped.
- CI validates Stage 5 coverage, read-only session/testkit conformance, and
  writer/SQLite architecture gates on all five target families.
- `release.yml` checks exact-commit CI through the Actions API using only the
  repository `GITHUB_TOKEN` with `actions: read`; no build or publication job
  starts when any required push workflow lacks a successful run. It also
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

- Validate YAML syntax locally where tooling exists.
- The canonical gate (`cargo xtask verify`) and MSRV 1.88 verification pass on
  every push. Linux (x86_64, arm64) and macOS (intel, apple-silicon) targets
  are verified green on the five-target matrix. Windows build-time is being
  optimized via Cargo artifact caching.

## Child DOX Index

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
- `workflows/release.yml` — tag-triggered ACP+TUI archive packaging and GitHub
  Release publication for the registry and installers; archives also bundle
  the repo `skills/` seed library seeded by the installers into
  `~/.agent-vesper/memory/`; the registry continues
  to launch only the ACP binary from the shared bundle.
