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
