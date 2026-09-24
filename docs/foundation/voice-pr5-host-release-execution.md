# VRO-17 PR-5 — Host parity and release execution

## Objective

Close the final VRO-17 phase without adding a cloud speech provider or extending
ACP. The release must ship the accepted local TUI voice stack, preserve generic
reasoning-provider dispatch and cancellation, document ACP's actual protocol
boundary, and pass the repository's exact-commit release sequence.

Cloud STT/TTS are future optional features. No concrete third-party cloud speech
provider ships in VRO-17 v1. The existing `VoiceStt` and `VoiceTts` contracts,
credential seam, egress classes, failover rules and mandatory pre-cloud TTS
hygiene/redaction remain the entry gates for later adapters.

## Source state and methods

- Starting branch/revision: `main` at
  `8f258ba28f4ea2f749526fb32b5b180ecc7dcead`.
- Published baseline: `v0.23.3` at `94ed16d`; workspace version before this unit
  was `0.23.3`.
- The working tree contained the uncommitted VRO-17 implementation and unrelated
  watcher/provider incident records. Unrelated records and `.flm-loop-*` model,
  audio, environment and cache trees are excluded from the VRO-17 commit.
- Inspected the TUI and ACP composition, ACP capability mapping and real-process
  transcript, release workflow, installers, registry manifest, user guides,
  owning DOX chain and the R1–R20 completion evidence.

## Host and release findings

### ACP capability decision

The production ACP initializer returns
`agentCapabilities.promptCapabilities.audio = false`. ACP owns no microphone
capture, speaker playback, live voice status, Voice Settings, speech-provider
picker or F5/F9/Stop gesture. Adding PCM or terminal playback status to stdout
would violate its JSON-RPC transport contract.

PR-5 therefore records a **documented exclusion**. ACP retains parity at the
shared provider-neutral runtime and cancellation seams. TUI voice transcripts
enter the same generic reasoning-provider path as typed input; a future
reasoning provider inherits voice without a provider-name branch. Future speech
adapters enter `VoiceStt`/`VoiceTts` without changing ACP, `VoiceSession`,
capture, playback or reasoning-provider integration.

### Release defect found

The pre-PR-5 release workflow built Docker, Swarm and Bridge features but omitted
`agent-vesper-tui/voice-kokoro` and `agent-vesper-tui/voice-flm`. A release from
that workflow would compile the TUI without the accepted VRO-17 conversation,
Kokoro and accelerated-recognition surfaces even though user documentation
described them.

The workflow now compiles both voice features in every release target. Optional
Kokoro/FLM model weights and runtimes remain user-installed through Settings;
they are not bundled. ACP remains voice-free.

The first default-feature workspace run also exposed that feature-only voice
evidence examples were being auto-discovered without manifest feature gates.
They referenced Kokoro, conversation and FLM modules that are intentionally
absent from a default build. Each example now declares its owning `ort`,
`voice-kokoro` or `voice-flm` requirement in `Cargo.toml`; default builds skip
those evidence binaries while all-feature builds continue compiling them.

The first exact-commit web-driver workflow then rejected the candidate before
container tests because the Dockerfile's exact
`chromium-headless-shell=152.0.7977.82-1~deb12u1` package had left the current
Debian Bookworm repositories. Official `bookworm-security` package metadata
listed `153.0.8010.52-1~deb12u1` for both amd64 and arm64. The Dockerfile now
pins that shared available version; the immutable base-image digest and the
two-architecture image/test contract are unchanged.

The replacement exact-commit five-target workflow exposed a separate macOS
Apple Silicon R20 portability defect: capture free-space checks invoked GNU-only
`df -B1 --output=avail`, while lease liveness consulted Linux `/proc` on every
platform. Seven real TUI library tests failed with `UnknownFreeSpace` or treated
the current process as dead. Capture space now uses `fs2`'s cross-platform
filesystem query. Linux retains its process-start identity check; Unix and
Windows use direct process probes for PID liveness, and an unavailable start
marker is conservatively live until the PID is proved absent.

The focused repair run then reproduced a same-process collision that the prior
`df` subprocess latency had masked: two captures opened within one millisecond
shared `cap-<pid>-<timestamp>`, and cleaning the first removed the second.
Capture directories now use the crate's existing UUID facility, with an
explicit back-to-back ownership regression.

The first canonical rerun also exposed a pre-existing verifier-test isolation
race: four real-Cargo tests created different temporary workspaces with the
same package identity while inheriting one shared `CARGO_TARGET_DIR`. Parallel
valid and deliberately broken fixtures could reuse each other's artifacts,
making two negative tests report `Passed`. Every fixture now receives a UUID
package identity, retaining parallel execution while separating Cargo caches.

The next exact-commit five-target run exposed another pre-existing test
isolation defect in the PR-4 F9 readiness regression. Its `PATH` fixtures owned
the speech-engine and player files, but the test still probed Alex's real
harness voice interpreter. Clean Linux, macOS and Windows runners therefore
reported the interpreter as the first blocker in four tests. Readiness now has
a narrow path-injection seam; production still supplies the same harness path,
while the regression supplies all three prerequisite files from its fixture.
The test also no longer imports Unix-only permission APIs.

## Files

- `.github/workflows/release.yml`, `.github/AGENTS.md` — ship and document the
  TUI voice feature matrix.
- `apps/agent-vesper-acp/AGENTS.md`,
  `apps/agent-vesper-acp/tests/process_transcript.rs` — explicit host exclusion
  plus real-process capability/catalog evidence.
- `docs/using-vesper.md`, `docs/installation.md` — F5/F9, final-only preview,
  FLM Verify, Kokoro, interruption, privacy, capture bounds, ACP and cloud
  roadmap documentation.
- `docs/voice-oracle-extraction-prd.md` — cloud future-feature contract and ACP
  decision.
- Workspace manifests, lockfile and `registry/agent.json` — v0.23.4 release
  identity and matching archive URLs.
- TUI and Kokoro manifests — gate feature-only voice evidence examples so the
  default workspace remains buildable.
- VRO-17 production, regression and evidence files accumulated through PR-0 to
  PR-4/R1–R20 — committed as the source being released.
- `crates/vesper-web-fetch/Dockerfile` — refresh the exact Debian Bookworm
  headless-shell package pin required by the release-blocking contained-driver
  workflow.
- `apps/agent-vesper-tui/src/voice_capture_store.rs`, its manifest and owning
  `AGENTS.md` — make R20 free-space and conservative lease-liveness checks
  portable across the release matrix.
- `crates/vesper-agent/src/vro/verifiers.rs` — isolate parallel real-Cargo
  verifier fixtures so a shared release target directory cannot cross-contaminate
  pass/fail evidence.
- `apps/agent-vesper-tui/src/voice_readiness.rs`,
  `apps/agent-vesper-tui/tests/pr4_f9_gate.rs` — make PR-4 readiness evidence
  hermetic and portable without changing the production prerequisite path.
- `apps/agent-vesper-tui/src/voice_speech_worker.rs`,
  `apps/agent-vesper-tui/tests/voice_multiturn_playback.rs`, and the test DOX
  record — inject deterministic synthesis into sustained playback tests while
  retaining the real worker and playback pipeline.
- `crates/vesper-voice-kokoro/src/setup.rs` and its owning `AGENTS.md` — make
  pack-removal lease liveness portable and fail closed when a platform probe is
  unavailable.

## Exact evidence

Pre-commit focused checks:

- ACP real-process composition:
  `cargo test --locked -p agent-vesper-acp --all-features --test
  process_transcript
  stdio_transcript_reaches_real_glm_adapter_with_protocol_pure_stdout --
  --exact --nocapture` — **1 passed**. It pins `audio=false`, no advertised
  voice/audio controls, protocol-pure stdout and the ordinary provider path.
- Two-provider neutrality and capability-aware Settings:
  `cargo test --locked -p agent-vesper-tui --features
  voice-kokoro,voice-flm --test voice_provider_neutrality` — **8 passed**.
- Release-profile composition, using the exact feature list in `release.yml`:
  `cargo build --locked --release --package agent-vesper-acp --package
  agent-vesper-tui --package vesper-web-fetch --package vesper-sandbox
  --features <docker,swarm,bridge for both hosts + TUI voice-kokoro,voice-flm>`
  — **passed**. Both binaries report `0.23.4`; the TUI binary contains the
  Voice conversation, Natural Voice pack, FLM recognition and live-preview
  presentation strings. The ACP binary contains none of those host surfaces.
- `cargo fmt --all -- --check`, `git diff --check`, and the registry assertion
  (`version == 0.23.4`; all five archive URLs contain `/v0.23.4/`) — **passed**.
- Default workspace red → green: the first `cargo test --workspace` failed to
  compile feature-only voice examples because their modules were absent from
  the default feature set; after adding manifest `required-features`, the same
  command completed with **0 failures**.
- Remote contained-driver red evidence: exact-commit web-driver run
  `35959319156` failed on both x86_64 and arm64 at `apt-get install` with
  `Version '152.0.7977.82-1~deb12u1' ... was not found`. Debian's current
  `bookworm-security` indexes identify `153.0.8010.52-1~deb12u1` for both
  architectures. Green replacement-run evidence is recorded after the updated
  exact commit completes.
- Remote five-target red evidence: run `35961360242`, job `107510423888`,
  failed seven `voice_capture_store` tests on macOS Apple Silicon. The log
  records GNU-only free-space probing as `UnknownFreeSpace` and current-process
  lease misclassification without Linux `/proc`. Focused red-to-green and the
  replacement five-target run are recorded after the repair completes.
- Remote five-target red evidence: run `35964811060` failed the same four
  `pr4_f9_gate` cases on clean Linux, macOS and Windows runners because the
  fixture omitted the harness voice interpreter. After injecting that path,
  `cargo test -p agent-vesper-tui --test pr4_f9_gate --features
  voice-conversation -- --test-threads=8` is green: **6 passed, 0 failed**.
  Production `voice_readiness_in` still derives the interpreter from
  `voice_venv_root`; only the controlled test route supplies a different path.
- Remote five-target red evidence: exact-commit run `35968227802` reached the
  repaired F9 gate but failed `presentation_separates_selection_from_availability_and_receipt`
  on clean Linux and macOS runners. The test incorrectly required the runner's
  physical FLM readiness before recording the verification receipt it actually
  exercises, while two parallel tests could reset the same process-global
  receipt. The regression now serializes every receipt mutation in that test
  binary and establishes the required state directly; it performs no hardware
  discovery or NPU work. The formerly failing test binary then passed five
  consecutive all-feature runs with 12 parallel test threads: **60 passed,
  0 failed**.
- Replacement exact-commit MSRV run `35971352460` and five-target run
  `35971352466` exposed the same machine-assumption defect in the separate
  `voice_flm_route` binary: its own header promised no device was required, but
  `readiness_is_pending_until_real_verification` required a fully provisioned
  machine and panicked on the correct clean-runner `DeviceAbsent` state. That
  panic poisoned the shared seam lock and produced three secondary failures.
  The test now asserts the provider contract rather than Alex's machine state:
  reset verification is never `Ready`, while an explicit verification receipt
  promotes the route to `Ready`; poisoned test locks recover without cascading.
  The repaired all-feature test binary passed five consecutive runs with 12
  parallel threads: **65 passed, 0 failed**.
- Exact-commit canonical run `35974282356`, MSRV run `35974282453`, and
  five-target run `35974282350` all reached the same two failures in
  `voice_multiturn_playback`: the tests selected the real system TTS adapter,
  so clean runners without `espeak-ng` failed on turn 1 before playback. The
  workstation's installed engine had masked this machine dependency. The real
  worker now has a narrow integration-test synthesis injection seam; production
  `spawn` and selection replacement still construct the selected real adapter.
  Both sustained-worker cases inject the provider-neutral `FakeTts` while
  retaining the production queue, generation, playback and recovery paths.
  With `PATH` restricted to a directory containing only `python3` (no speech
  engine or audio player command), the two formerly failing tests passed five
  consecutive parallel runs: **10 passed, 0 failed**. The complete all-feature
  test binary then passed: **11 passed, 0 failed**.
- The first Rust 1.88 audit compile found the internal stale-generation unit
  test's direct `run_worker` call missing the new optional injection argument.
  Adding explicit `None` preserved its production-engine behavior. A cleaned
  all-feature TUI library rebuild passed **285 tests**, and the full Rust 1.88
  workspace then passed with all features.
- Exact-commit five-target run `35980320616` then exposed a separate Windows
  compile defect in the older PR-1/PR-2 voice adapter regressions. Their
  controlled subprocess fixtures are POSIX shell wrappers, but the files
  unconditionally imported `std::os::unix::fs::PermissionsExt`; Windows failed
  before running any test. Only the shell-backed helpers and their dependent
  cases are now `cfg(unix)`. Platform-neutral configuration, HTTP, failover,
  partial-gate, cancellation and contract tests remain enabled on Windows. A
  focused Windows-target all-feature test check reproduced both files' E0433/
  E0599 errors and passed after the correction. The Unix PR-1/PR-2 adapter
  binaries then passed five consecutive runs: **245 tests passed, 0 failed**.
  A whole-workspace Windows cross-check cannot link Windows C dependencies from
  this Linux host (`lib.exe`/MSVC are absent); the native Windows matrix remains
  the authoritative whole-workspace execution gate.
- The next exact local canonical run exposed a parallel fixture-name collision
  in `pr4_f9_gate`: workspace roots used process id plus wall-clock nanoseconds,
  but the clock can return the same value to concurrent tests. An enabled-scope
  case could overwrite the disabled scope before its Settings assertion. Both
  fixture classes now use one process-local atomic identity, which is unique
  regardless of clock resolution.
- Replacement five-target run `35983279272` compiled beyond the repaired voice
  adapter tests on Windows and exposed the same portability class in a Kokoro
  measurement example: POSIX permissions and shell launchers were compiled
  unconditionally. A repository-wide Unix-API audit found four such VRO-17
  receipt examples plus POSIX-only playback/swarm test fixtures. The examples
  now compile truthful Windows notice stubs, and only the dependent test cases
  are Unix-gated; platform-neutral playback validation remains cross-platform.
- The completed `35983279272` failure set contained two causes, not three:
  Windows job `107579968410` failed the unconditional Unix example compile;
  macOS Apple Silicon job `107579968274` and macOS Intel job `107579968446`
  both failed `removal_is_refused_with_a_live_foreign_lease`. That regression
  assumed Linux PID 1 and production lease liveness used Linux `/proc` on every
  platform. Linux x86_64 and ARM64 passed; exact-commit quality, supply-chain,
  MSRV and web-driver runs also passed. Kokoro removal now uses `/proc` on
  Linux, `kill -0` plus `ps` on other Unix platforms, and `tasklist` on Windows;
  unavailable probes conservatively preserve the pack. The regression uses the
  current live process through a narrow exclusion seam instead of a platform
  PID assumption. The repaired all-feature Kokoro suite passed **42 tests**;
  strict Clippy passed; locked all-feature checks passed for
  `x86_64-apple-darwin`, `aarch64-apple-darwin`, and
  `x86_64-pc-windows-msvc`.
- Exact-commit five-target run `35989669001`, Windows job `107600481144`,
  compiled beyond the repaired examples and exposed one remaining Unix-only
  symlink call in the R20 capture store's internal unit tests. The separate R20
  integration test correctly gated the symlink operation but still declared
  its path on Windows. The unit case is now Unix-gated and the integration
  fixture declares that path only on Unix. The platform-neutral R20 recovery,
  ownership and bounds cases remain enabled on Windows.

The final commit SHA, complete local gate results, exact-commit workflow run IDs,
tag, release assets/checksums and registry PR receipt are appended only after
each step succeeds.

## Deviations

- ACP voice UI/audio is excluded because ACP v1 truthfully advertises no audio
  capability. No protocol extension was invented.
- No physical microphone, speaker, FLM/NPU workload, live reasoning-provider
  call or local installer replacement is part of PR-5 verification. Existing
  user acceptance supplies the tested-device evidence; CI/local regressions
  verify the release source and package.
- The already-committed `8f258ba` MCP lifecycle repair precedes this VRO-17
  commit on local `main` and will also reach `origin/main`; it is not claimed as
  VRO-17 work.

## Unresolved items

- Exact-commit local and remote release gates, publication, public asset
  verification and registry update remain open until recorded below.
- User acceptance remains bounded to Alex's tested setup. No universal acoustic,
  platform, provider, NPU TTS or cloud speech claim is added.

## Readiness effect

PR-5 is ready for the isolated VRO-17 commit and exact-commit release sequence.
VRO-17 remains open until publication and the final audit update.
