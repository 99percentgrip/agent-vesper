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
