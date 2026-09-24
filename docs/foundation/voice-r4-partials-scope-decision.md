# VRO-17 R4 — Partial-Transcript Scope Decision + Provider-Neutrality Audit

Decision record (analysis + documentation only): 2026-09-23, evening
session. Baseline `main` @ `8f258ba` + 172 dirty/untracked paths (the
VRO-17 voice tree; unrelated work preserved). Feature set audited:
`voice-conversation`, `voice-kokoro`, `voice-flm`. **No production code,
tests, adapters, Settings, workflows, or providers were modified.**

---

## 1. Baseline registries (source-verified)

- **Reasoning providers** (registered in `register_default_providers`,
  `main.rs:813`): OpenAI (`vesper-provider-openai`), Z.ai/GLM
  (`vesper-provider-glm`), LM Studio (TUI-host adapter), plus
  test-only `vesper-provider-synthetic` (cfg(test), never
  user-selectable).
- **STT providers**: `SharedSidecarStt`/`SidecarStt` (CPU Whisper
  sidecar), `FlmNpuStt` (NPU, feature-gated), `HttpStt` (self-hosted,
  library-only). **TTS providers**: `KokoroTts`, `SubprocessTts`
  (baseline engine, not selected by conversation).

## 2. Reasoning-provider neutrality — call-path proof

Production path, actual owners:

| Boundary | Production owner | Provider-neutral? |
|---|---|---|
| F9 capture | `voice.rs::Worker::start/stop` → managed capture store (R20) | ✓ no provider knowledge |
| Selected STT | `SelectedStt::build()` from saved scope + `voice_accel` resolution | ✓ speech-provider only |
| Final transcript → submission | `ConversationController` executes `VoiceEffect::SubmitTurn` → **`dispatch_voice_submission`** (`main.rs:4950`) → **`spawn_submitted_prompt`** — *the identical typed-Enter path* | ✓ |
| Runtime/provider | `AgentLoop` + `ProviderRegistry` (`vesper-runtime`, heterogeneous factories, no concrete provider enum) | ✓ generic |
| Provider-specific mapping | ends inside each provider adapter; voice receives `AgentProgressEvent::ContentDelta` / `AgentEvent::Completed/Failed` | ✓ provider-neutral events |
| Assistant text → speech | `voice_conversation.rs` translates deltas to `HostEvent::AgentText`; PR-2 hygiene; `SpeechWorker`/Kokoro | ✓ TTS never knows the source |
| Playback | `PlaybackOwner` | ✓ |

**Grep sweep of provider names** (openai/zai/lmstudio/glm/xai/grok,
case-insensitive, non-comment/non-test) across every voice-owned
production module (`vesper-voice/src/**`, `voice*.rs` in the TUI):
**zero reasoning-provider hits**. The only "GLM" occurrences are
`GLM_ACP_WHISPER_MODEL` (sidecar env var for the *speech* model name) and
`GLM_VENV_PATH` (a legacy *speech*-venv env fallback) — classified
**speech-provider-specific naming legacy**, not reasoning coupling: they
configure the STT interpreter/model, never branch on or import a
reasoning provider. Recorded as a cosmetic naming note, not a defect.

Verified additionally: no voice module imports `OpenAiFactory`/
`GlmFactory`/`vesper-provider-*`; `vesper-voice` depends only on
`vesper-domain`/`vesper-security` (arch gate, 30 pkgs); no provider crate
references `VoiceStt`/`VoiceTts`; F9's gate branches only on scope +
readiness + *speech*-stage policy, never provider identity;
`voice_turn_instruction` is provider-agnostic text.

**A future conforming xAI reasoning adapter** registers through
`ProviderRegistry::register_with_all` and is immediately usable by F9:
the voice path submits through the generic turn seam and consumes
provider-neutral events. **Voice-specific production changes required:
NO.**

### Neutrality regression proof — current state

Existing evidence is **single-provider-shaped**: `pr4_wiring.rs` uses
`FakeStt` (a speech double) and asserts submission semantics;
`r3_loop_pty.py` drives the *real* LM Studio adapter against a loopback
HTTP double through the true F9 loop — strong generic-path proof, but for
one provider. Nothing registers a *distinct fake reasoning provider* and
proves typed **and** F9 input reach it identically. **Verdict: PASS —
test gap.**

**Missing proof (spec; do not implement in this unit):** a new
integration test (suggested home: `apps/agent-vesper-tui/tests/
voice_provider_neutrality.rs`, `required-features =
["voice-conversation"]`, or extend `pr4_wiring.rs`) that (a) registers
two distinct fake conforming provider factories under different
`ProviderId`s through the normal registry seam, (b) drives the
production conversation composition boundary with typed input and then
with an F9-origin `SubmitTurn`, (c) asserts both reach the *currently
selected* fake provider through the same `spawn_submitted_prompt` seam
with no provider-conditional voice code executed, and (d) asserts the
assistant text flows back through `AgentProgressEvent::ContentDelta`
into the same `HostEvent::AgentText` → synthesis path for both
providers. It must fail on any `if provider == "x"` voice logic — the
two-fake symmetry makes name-branching visible (whichever name is
special-cased, the other fake starves). No xAI account or network needed.

## 3. R4 current reality (source-verified)

| Fact | Evidence |
|---|---|
| Adapter partial modes: **all three production STT adapters declare `partials: None` (final-only)** | `stt_sidecar.rs:235`, `stt_http.rs:149`, `voice_flm.rs:687` |
| `PartialGate` is library-complete (buffered repass, whole-capture budget, coalescing, finals-first, stale-generation rejection, no submission API) but **constructed by zero production hosts** | only non-test references: its own module + `pr1_adapters.rs` |
| Not reachable from F9 or F5 capture | grep: no construction sites |
| FLM NPU: final-only, declared truthfully (`partials: None` + module doc "No partials") | `voice_flm.rs:24,687` |
| `[voice] partials` toggle exists in Settings and persists; **`scope.partials` is consumed by no runtime code** — a dormant setting | `settings_host.rs:787,831`; `settings_voice_save.rs:19`; zero consumers |
| No interim-text UI exists: `VoicePhase` has no interim state; F5 shows only "Transcribed N seconds" progress; no rendering path distinguishes interim from final text | `ui.rs:180-187`, `voice.rs:824` |
| Stale/generation guards: exist and are sound (`PartialGeneration{id, finalized}`, finals-first, coalesced) — **sufficient by design** | `partials.rs:50-62,95-190` |
| Partials are **not required** by barge-in (Stop uses urgent lane + acked playback), Stop, final submission, telemetry (`VoiceTurnReport` fields), interruption notes (acked-playback only), or R20 (capture store) — each has its own owner | respective modules |
| Cost if wired: repass = **full re-transcription of the buffered capture** on the same engine (`partial_eval` → `inner.transcribe(snapshot)`) → repeated CPU inference during capture, or NPU round-trips contending with the single owned FLM server for the final | `partials.rs:140-185`; FLM single-owned-serial design |

**Alex's saved config has `partials = true`** — currently a no-op. No
accepted user experience has ever rendered a partial (no user record
mentions them; no UI exists).

## 4. Option A — keep R4 mandatory, implement production partials

Smallest honest scope: wire `scope.partials` into ONE capture host (F9
conversation: wrap the selected `VoiceStt` in `PartialGate` when the
descriptor declares `BufferedRepass` support); render interim text in a
capture-status line distinct from final; capability-aware Settings (hide/
explain when the active STT declares `None` — today that is **all three
adapters**, so the toggle would explain-only everywhere until an adapter
opts in); repass cadence bound; CPU/NPU contention budget (FLM: repasses
queue on the single owned server, delaying the final); stale-supersession
tests; two-fake provider parity. **Critical honesty constraint:** FLM and
both current STT adapters are final-only — Option A mandates either
fabricating nothing (toggle explains "not supported by the current
engine" for every current engine) **or** adding genuine incremental
support to an adapter first. Nothing in today's stack can produce a
partial without new engine work.

## 5. Option B — amend R4 to an optional STT capability

Contract as specified in the directive: providers declare
`BufferedRepass`/`Incremental`/none; when enabled **and** supported,
partials are display-only, best-effort, bounded, never submitted,
stale-safe, always superseded by the final; a final-only provider remains
fully conformant. Concretely today this means: `PartialGate` stays as
reusable infrastructure (already built and tested); descriptors stay
honest (already are); the Settings toggle becomes capability-aware
(small follow-up when any adapter opts in; today it would show "not
supported by the current engine" — one Settings-panel change, separately
authorized); `VoiceSession` unchanged (gate wraps the adapter, not the
session); no fake partials.

## 6. Comparison against the criteria

| Criterion | A (mandatory) | B (optional) |
|---|---|---|
| Alex's accepted F9 experience | unchanged either way (partials never rendered) | unchanged |
| Latency | repass adds inference during capture; final may queue behind repasses | none |
| CPU/NPU contention | real: CPU repass competes with the final; FLM repasses contend with the single owned server | none |
| Correctness | new stale/supersession surface in the hot path | zero new surface |
| Usefulness in short push-to-talk | low: F9 captures are seconds; the final arrives before a repass cadence matters; Alex never saw or asked for interim text | same utility today |
| Complexity this late | highest remaining R-item | documentation-only + later optional toggle |
| Regression risk | moderate (capture path, cancellation races) | minimal |
| Provider neutrality | neutral either way | neutral |
| Capability honesty | requires per-engine work to be non-vacuous | preserves truth: all current engines final-only |
| Future extensibility | same | same (gate reusable) |
| Required for real-time bidirectional voice to function | **no** — streaming STT→turn→streaming TTS works final-only (accepted experience proves it) | no |
| Justifies a mandatory v1 gate | no evidence it does | — |

## 7. Recommendation

**Option B.** Evidence: (1) partials are not needed by any mechanism that
exists — barge-in/Stop/notes/telemetry/R20 each own their inputs; (2) no
current STT adapter can produce one, so a mandatory R4 either stays
unimplementable without new engine work or pressures dishonest
fabrication; (3) the accepted experience (2026-09-23) never rendered a
partial and Alex never requested them; (4) repass costs real CPU/NPU
contention exactly where the NPU route is weakest (single owned server).
`PartialGate` is preserved as ready infrastructure for any future
incremental-capable adapter.

**Status: DECISION REQUIRED — Alex must approve Option B (or choose A)
before R4 is amended.** Per the directive, R4 is NOT amended in this
unit.

## 8. Documentation actions taken

- This record created.
- Final audit updated: R4 marked **DECISION REQUIRED** with a link here;
  no verdict changed; ledger untouched (R4 stays IMPLEMENTED —
  acceptance open until the decision).

## 9. Verification performed

Read-only: git baseline; grep sweeps (provider names in voice modules;
`scope.partials` consumers; `PartialGate` construction sites; adapter
descriptor fields); source reads of the F9 gate, `dispatch_voice_submission`,
`spawn_submitted_prompt`, `register_default_providers`, `PartialGate`,
`VoicePhase`; existing-test survey. Links/whitespace checked for the
touched docs. No builds, tests, devices, providers, or workloads run.
