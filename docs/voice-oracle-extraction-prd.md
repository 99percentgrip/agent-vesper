# PRD — VRO-17: The Voice Oracle Extraction (Real-Time Bidirectional Voice Interface)

Status: **OPEN — but see the [final completion audit](foundation/voice-vro17-final-completion-audit.md)
for the current requirement matrix and the 2026-09-23 approved scope
amendments.** The historical staged gates landed (PR-0…PR-4); the
remaining open item is PR-5 (documented ACP exclusion + exact-commit release
gates). R1–R20 are closed at their approved scopes: the §2.4 production
binding repair is recorded in
[`foundation/voice-r6-binding-repair.md`](foundation/voice-r6-binding-repair.md),
and Alex's passing real-device interruption/recovery result is closed in
[`foundation/voice-r6-device-acceptance-closeout.md`](foundation/voice-r6-device-acceptance-closeout.md).
R4 is amended to an optional capability and closed; R20 is closed. Optional
integrations Alex may add later remain outside the required closure set. The remaining
short-reply quality and Voice Settings presentation defects are repaired and
verified in
[`foundation/voice-short-reply-quality-repair.md`](foundation/voice-short-reply-quality-repair.md);
Alex subsequently accepted that candidate as “Looks like smooth” on the tested
setup, and the pronunciation repair remains accepted. Concrete third-party
cloud STT/TTS providers are deferred roadmap features rather than VRO-17 v1
release blockers; D28 preserves their binding adapter and security contract.
Earlier status text below is preserved
as history: the 20/40/20-second latency observations describe the
superseded pre-repair build (the lineage ended at the
[2026-09-23 NPU-conversation user acceptance](foundation/voice-npu-user-acceptance.md)),
and "NPU STT/TTS … remain open" was
true when written — NPU STT is now production-implemented and
user-accepted at its documented scope. PR-0 contracts live in
`docs/foundation/voice-oracle-pr0-execution.md`. Port existence is not capability
(ADR 0028). The latest NPU STT continuation is recorded in
[`foundation/voice-npu-stt-implementation-progress.md`](foundation/voice-npu-stt-implementation-progress.md):
the shared installed-Silero worker is locally probed on both silence and
speech-positive paths, the authorized Whisper asset is installed and
hash-verified, and the 2026-09-22 launch-gate correction established that FLM
1.0.5 standalone ASR (`flm serve --asr 1`, no positional chat model) serves
real NPU transcription on loopback — the earlier failure was chat-model-slot
misuse plus a port-conflict confounder, not a runtime limitation. The
production route is now **implemented behind the default-off `voice-flm`
feature** (owned loopback supervisor + VAD-composed final-only adapter +
`voice_accel` registration behind real per-process verification + native
Settings Verify/compute selection + F9 wiring), with real-model no-device
receipts (silence short-circuit 0.28 s, cold surrogate 3.7 s, warm 3.2 s).
Open acceptance: natural-speech accuracy, the full PTY F9 round-trip
receipt, and Alex's live microphone acceptance. The backend's own
hallucination on unfiltered silence re-confirms that only a VAD-protected
composition may ever be registered.

> **2026-09-23 approved scope amendments (Alex):** R2 concrete cloud STT
> and R3 concrete cloud TTS are optional gated integrations, not
> mandatory v1 completion gates; R16b NPU TTS is capability-gated and CPU
> synthesis is first-class/conformant. Amended contracts live inline at
> R2/R3/R16 and in the final completion audit. All other requirements
> keep their original force.

Reference upstream: **the voice oracle** (alias assigned by Alex in the
mission directive). Its brand names, repository/owner names, and its agent
runtime's brand must never appear in any PRD, AGENTS.md, README, source
comment, fixture, or commit message produced by this work. Reconnaissance
evidence: `docs/architecture/recon_voice_oracle.md` (upstream pinned
`88998de8369e9d36f6d434b5e01feb93fcf1c33f`, MIT — upstream attribution
and license text are preserved in that report and in the external clone;
the naming rule governs Vesper-produced artifacts, not the deletion of
upstream attribution). This PRD is informed by
that reconnaissance under the no-port rule: mechanisms and contracts are
extracted, source code is not.

Related: `voice-control-prd.md` and
`docs/foundation/voice-control-execution.md` (existing terminal dictation
surface that this subsystem generalizes);
`docs/voice-trailing-silence-vad-prd.md` (binding VAD evidence);
ADR 0006 (provider-neutral core and adapters), ADR 0007 (session actor and
hierarchical cancellation), ADR 0016 (provider-independent embedding layer
— precedent for an independent capability layer), ADR 0028 (native
implementation acceptance).

PR-5 host/release evidence:
[`foundation/voice-pr5-host-release-execution.md`](foundation/voice-pr5-host-release-execution.md).

---

## 0. Executive summary

The voice oracle demonstrates a complete real-time bidirectional voice
interface — streaming STT with live partials and failover, an agent turn
streamed over SSE, sentence-gated streaming TTS, barge-in with
last-heard-sentence context, and per-stage latency telemetry — as a
**translation shim with zero agent logic**. Vesper extracts that shape as
a native Rust subsystem, `vesper-voice`, that is:

- **Provider-neutral by construction.** STT and TTS are trait ports with
  interchangeable registered adapters (local and cloud), exactly as
  `vesper-provider` treats model providers. No vendor name is compiled
  into the core.
- **Agent-free.** The crate contains no reasoning, no tool execution, no
  policy. It consumes the existing runtime's turn/cancellation surface and
  emits typed events. Integration happens at the composition boundary in
  the hosts.
- **Opt-in and non-degrading.** Absent configuration, both hosts are
  byte-identical to today. Voice is a layered capability like the web
  tools, never a hidden cost on the coding loop.

The migration is staged PR-0 (contracts) → PR-1 (STT adapters) → PR-2 (TTS
adapters + hygiene) → PR-3 (voice session orchestration + barge-in) → PR-4
(TUI host integration) → PR-5 (ACP + release gates). Every PR proves zero
regression on the existing floor before proceeding.

## 1. Goals and non-goals

### Goals

1. Real-time bidirectional voice conversation with the **existing**
   Agent Vesper runtime: speak → transcript → normal agent turn → spoken
   reply, streaming both directions.
2. Interchangeable STT providers behind one provider-neutral port, with
   at least one production-capable local implementation selected and
   configured like model providers. *(Amended 2026-09-23, approved by
   Alex: concrete self-hosted/cloud STT integrations are optional
   follow-on integrations behind their own gates — see R2.)*
3. Interchangeable TTS providers behind one provider-neutral port, with
   at least one production-capable local implementation. *(Amended
   2026-09-23, approved by Alex: concrete cloud TTS is an optional
   follow-on integration behind its own gates — see R3.)*
4. Barge-in: user speech during assistant playback cancels the spoken
   output, cancels the agent turn **through Vesper's existing transactional
   cancellation**, and gives the next turn honest context about what was
   heard.
5. Live partial transcripts during capture (opt-in), reusing the same
   provider path as finals. *(Amended 2026-09-23 with R4: an optional
   STT-provider capability — see R4 — not a mandatory v1 goal.)*
6. Deterministic TTS text hygiene with secret redaction before any cloud
   synthesis.
7. Per-stage latency telemetry for every turn outcome (success, error,
   interrupted).
8. The existing terminal dictation surface (`apps/agent-vesper-tui/src/voice.rs`)
   is preserved and eventually re-hosted on the shared subsystem with no
   user-visible regression.

### Non-goals (v1)

- Wake-word detection, always-on listening, VAD-gated auto-capture.
- Multi-client LAN server, browser HUD, TLS surface, media panels,
  dashboard proxies, machine-stats panels — host-product concerns of the
  upstream that Vesper's two hosts do not need.
- Voice biometrics, speaker identification, per-user voice profiles.
- Telephony, PSTN, SIP integrations.
- Real-time bidirectional **protocol** audio codecs (Opus/WebRTC);
  v1 uses raw PCM on loopback/in-process transports only.
- Any agent reasoning, tool choice, or policy inside the voice subsystem.
- Cloud-only deployments; the local paths are first-class, not fallbacks.

## 2. Architecture

### 2.1 Crate placement and dependency rules

New production crate `crates/vesper-voice`:

- Depends only on `vesper-domain` and `vesper-security` among workspace
  crates (same class as `vesper-provider`), plus bounded utility crates
  (`serde`, `thiserror`). This is the **pure-core boundary**; it is what
  `cargo xtask architecture` enforces for the crate in all builds.
- **No dependency on `vesper-runtime`, `vesper-agent`, `vesper-harness`,
  or any provider adapter crate — including `vesper-provider`.** The core
  knows nothing about agents and nothing about model-provider streaming
  types. Hosts translate existing runtime/provider events into
  voice-owned, agent-neutral inputs at the composition boundary (§2.3);
  the core never sees `ProviderStreamEvent` or `HarnessEventPayload`.
- **Concrete adapters do not live in `vesper-voice`'s core.** Any future
  adapter modules (local engines, cloud transports) are feature-gated
  additions evaluated per-PR against the repository's dependency rules,
  and heavy backends remain outside the pure core. Default-off features
  do not grant dependency ownership: a feature that would pull a forbidden
  dependency into the core must instead live at the composition boundary
  or in a dedicated crate (the `vesper-web` → `vesper-web-fetch` split is
  the precedent). PR-0 ships **no adapter code at all** — types, ports,
  configuration contracts, and in-memory fakes only.
- The crate compiles cleanly with no features (core types + fakes) and
  contains no I/O, no clock reads, no process spawning, and no device
  access. `unsafe` is forbidden at the crate root like every foundation
  crate.
- Hosts (`agent-vesper-tui`, `agent-vesper-acp`) compose adapters at their
  boundary, the same seam `WebService` uses in `vesper-harness`.

Rationale: `vesper-bridge`'s precedent — pure provider-neutral contract
crate with no I/O and no vendor names, adapters composed by the harness.
The voice problem has the identical shape.

### 2.2 Core ports (the stable interfaces)

PR-0 implements exactly this surface in `crates/vesper-voice` (§8 of the
PR-0 execution report lists the landed modules). The Rust source of truth
is the crate; this section records the binding contract.

**Audio framing — validated frames vs raw transport bytes.** The canonical
v1 audio contract is unchanged: i16 little-endian, 16 000 Hz, mono.
`AudioFormat` carries and validates exactly those parameters. The core
type is `PcmFrame`: a *validated, sample-aligned* unit built only through
`PcmReassembler`, never from arbitrary transport chunks. A reassembler
carries an unmatched odd byte to the next chunk (the voice-oracle
playback lesson); an odd byte that is still unmatched when capture ends
is reported as `VoiceError::Truncated` — truncation is observable, never
silently dropped or invented into a sample. Frames carry no resampling:
if a device or model produces a different rate, the **host** resamples at
its capture boundary before frames enter the core, and adapters that
require other formats state so in their descriptors. Resampling is a
documented host/adapter responsibility, not an assumed property.

**Cancellation token ownership (decided at PR-0).** `vesper-voice` defines
its own minimal `VoiceCancel` token (idempotent request + poll + async
wait), mirroring `vesper-runtime`'s `RuntimeCancellation` shape. This is a
locally owned contract, *not* a dependency-free re-export: hosts construct
a `VoiceCancel` child per voice turn and cancel it alongside — never
instead of — the runtime's own cancellation. It is deliberately
structurally small so an adapter is trivial. The alternative (promoting a
shared token into `vesper-domain`) was rejected as a larger cross-crate
change with no PR-0-scoped benefit; revisitable if a second consumer
appears. All PR-0 futures take `&VoiceCancel`.

**STT port.**

```rust
pub trait VoiceStt: Send + Sync {
    fn transcribe<'a>(&'a self, audio: &'a [PcmFrame], cancel: &'a VoiceCancel)
        -> VoiceFuture<'a, Result<SttTranscript, VoiceError>>;
    /// Opt-in live partials. None = this provider has no partial mode.
    fn partial(&self) -> Option<&dyn SttPartial> { None }
    fn descriptor(&self) -> &'a SttDescriptor; // id, locality, model, langs,
                                               // partials: PartialsKind
}
```

`SttPartial` is *repeated transcription of the buffered utterance so far*
(`PartialsKind::BufferedRepass`), which is what the reference implements
(periodic re-transcription of the capture buffer, bounded window) — **not**
incremental streaming inference. A future adapter that maintains
incremental state declares `PartialsKind::Incremental`; the descriptor
makes the difference observable, and hosts/PRD text must not label one as
the other. Partials are best-effort by contract: a `partial()` call
neither blocks capture callbacks (the host invokes it on its own worker;
the core never spawns work) nor has any effect on the final transcript —
the final `transcribe()` result always supersedes obsolete partial work.
Partial text is interim display data only; it is **never** submitted as a
turn input and never treated as an executable command.

**TTS port — full stream lifecycle.**

```rust
pub enum TtsChunk {                    // one stream's items
    Audio(PcmFrame),
    Finished,                          // clean end of synthesized audio
}

pub trait VoiceTts: Send + Sync {
    /// Open one synthesis stream. Err before any audio = open failure
    /// (auth, quota, unavailable, invalid input). After the first audio
    /// frame, failures surface as MidStream on the stream itself.
    fn synthesize<'a>(&'a self, text: &'a str, voice: &'a VoiceProfile,
                      cancel: &'a VoiceCancel)
        -> VoiceFuture<'a, Result<BoxStream<'a, Result<TtsChunk, TtsMidStreamError>>, VoiceError>>;
    fn descriptor(&self) -> &'a TtsDescriptor;
    fn voices(&self) -> Result<Vec<VoiceProfile>, VoiceError>;
}
```

`TtsMidStreamError::AudioFailed` (audio began, then the provider failed)
is distinct from a clean `Finished` and from `VoiceError::Cancelled`.
`TtsChunk::Finished` proves the provider completed synthesis — never that
the user heard it (§2.3 playback acknowledgments).

**Error taxonomy (v1, exhaustive for PR-0).**
`Unavailable { provider, reason }` — provider not reachable/configured;
failover-eligible. `NoSpeech` — VAD found no speech; a **nonfatal
outcome**, not a provider fault: the turn ends politely and no retry
chain runs (the reference's conflation of "unavailable" with "empty" is
corrected here; VAD stays **enabled** per Vesper's shipped evidence,
`docs/voice-trailing-silence-vad-prd.md`). `Auth`, `Quota` — provider
account states, never silently retried. `Cancelled` — caller requested
stop; not an error. `InvalidInput` — malformed text/audio parameters,
`Truncated` — unmatched trailing byte (above), `ResourceExhausted` — a
bounded queue/budget was hit, `Inference` — the engine itself failed on
valid input. No arbitrary exception is ever converted into `NoSpeech`:
engine failures map to `Inference`/`Unavailable` and remain visible.

**Failover is a policy, not a core dependency.** PR-0 defines
`FailoverStt`/`FailoverTts` *contracts* (ordered adapter lists;
`Unavailable` advances to the next adapter, `NoSpeech`/`Auth`/`Quota`
stop the chain — silence is an answer; auth state is not an outage). The
implementation lands in PR-1 with the first real adapters; PR-0 pins the
semantics so adapters are written against a frozen rule.

### 2.3 Turn orchestration and the host contract

The orchestration core is host-agnostic and pure: the host feeds events;
the core emits client events and turn requests. **All agent/provider
types stay in the host.** The host translates the existing runtime
surface (`HarnessCommandPayload::SubmitPrompt`/`CancelTurn`,
`HarnessEventPayload::{ContentDelta, ToolCallStarted, TurnCompleted,
TurnCancelled, …}` via `RuntimeSupervisor`) into the voice-owned inputs
below. Nothing in `vesper-voice` imports or names those types.

**Dimensions, not a linear phase.** A voice turn has independently
tracked dimensions, because generation and playback overlap:

- capture: `Idle | Capturing` (user-controlled; explicit start/stop)
- stt: `Waiting | Running | Settled`
- agent: `NotSubmitted | Dispatched(turn identity) | Completed(FinishOutcome) | Failed`
- synthesis: per-speech-segment `Pending | Streaming | Finished | Failed | Cancelled`
- playback: `Draining | Acked(through segment) | Stopped | Unknown`
  (acknowledgments are host-supplied estimates — see below)

`VoiceTurnPhase` remains as the *projection* for UI (e.g.
`Capturing/Transcribing/Thinking/Working/Speaking/Interrupted/Done`),
derived from the dimensions; PR-0 defines the projection function.

**Host → core (complete; PR-0 types):**

`CaptureStarted`, `CapturedAudio(PcmFrame)`, `CaptureStopped` — the
capture lifecycle; bounded by `CaptureBudget`. `AgentText { text }` —
**approved user-visible assistant text only**: the host selects the
correct stream channel (visible content deltas, not reasoning/tool
channels) before this call; regex stripping of reasoning markers inside
hygiene is defense-in-depth, never channel selection. `AgentTurnIdentity
{ turn_id }` — the runtime's own `TurnId`, once assigned; correlation for
stale-output rejection uses this plus a voice-owned monotone
`SpeechSegmentId` and `CancelGeneration` counter that the core stamps on
every outstanding synthesis/playback request. `AgentTurnSettled {
outcome }` — terminal runtime completion/failure/cancellation (the
`FinishOutcome` translation, voice-owned enum). `PlaybackAck {
segment, through_bytes }` — host acknowledgment that audio up to an
offset reached the output device. `PlaybackUnavailable` — the host
cannot observe playback (no acks will arrive; progress stays `Unknown`).
`DeviceOrProviderFailure { class }` — capture/playback device failure or
provider failure surfaced by the host. `BargeIn`, `StopRequested` — the
two distinct interruption triggers.

**Core → host:** `PartialTranscript`, `Transcript`,
`AgentActivity` (forwarded tool/approval *status* for display only),
`Speak { segment, text }` (a hygiene-passed unit; host drives TTS or uses
the core pipeline in PR-3), `SpeechAudio { segment, frame }`,
`PlaybackStop`, `Interrupted { heard_through }`, `TurnDone(report)`,
`Failed(error)`.

**Voice never approves tools.** Approval decisions flow through the
existing host/runtime permission mechanisms only; the voice surface can
*display* a request and *relay a user decision the host already owns*,
nothing more. Interim transcripts are never commands.

**One terminal outcome per accepted voice turn.** `TurnDone(report)`
is emitted exactly once; a turn rejected before submission (empty
transcript) ends in `NoSpeech`-classified `TurnDone`, not `Failed`.
`Failed` is reserved for infrastructure failures. Late text/audio after
a terminal event is rejected by generation stamp.

**Playback acknowledgments and honest "heard" claims.** A `PlaybackAck`
proves *the host handed those bytes to the output device at an estimated
position* — nothing stronger. When the host cannot observe playback,
progress is `Unknown` and any interruption note must say what was
*queued/synthesized*, never claim it was heard. The upstream's
"last spoken sentence" heuristic is downgraded to exactly this honesty.

### 2.4 Barge-in, Stop, and interruption contract (binding)

Both `BargeIn` and `StopRequested` produce two separable effects —
(1) stop speech output, (2) request transactional runtime cancellation —
and differ only in that barge-in immediately opens a new capture while
Stop does not. v1 behavior is unchanged; PR-0 adds no UI mode.

On interruption the core/host must, in order: flush the playback queue
(host stops active sources); cancel in-flight synthesis via its
`CancelGeneration` stamp (stale text/audio arriving after is rejected,
not appended); capture the runtime turn identity if assigned
(cancellation before identity assignment is legal — the request is
recorded and the host cancels when/if `AgentTurnIdentity` arrives, or
observes the turn settle without us); route cancellation through the
**existing runtime contract** (`HarnessCommandPayload::CancelTurn`),
never a bespoke stream drop. Already-completed tool effects remain
completed; unresolved effects are governed by the runtime's
`StreamInterrupted{tool_call_started}` classification. The voice layer
never replays a possibly side-effecting tool operation.

The next turn's user input carries a bounded interruption note built from
**acknowledged playback information only**: clearly labeled as
Vesper-generated context (bracketed, prefixed as assistant-response
context — not as user words), containing no hidden reasoning, no
secret-bearing text, and no text that was never queued for playback. It
is ordinary message content with no elevated instruction status. When
playback progress is `Unknown`, the note says playback status was
unknown, not that a sentence was heard.

New turns are submitted only when the existing session contract permits
(one active turn; the previous turn settled or was cancelled) — the voice
layer never force-submits over a live turn. A barge-in during STT
cancels STT and discards the buffer; during a pending approval it stops
speech and requests cancellation but the approval decision remains the
user's through the existing mechanism; repeated interruption before
cleanup finishes is idempotent — the second request is a no-op recorded
as such.

### 2.5 Hygiene-and-segmentation pipeline (one stateful pass)

Ordering is now explicit: **segmentation and hygiene are one stateful,
bounded pipeline — hygiene runs on gated segments, not before gating.**
Reason: redaction must see protected spans that close across sentence
boundaries and chunk boundaries; a pre-gate pass would either redact
half-spans or duplicate state. The pipeline accepts host-approved
assistant text chunks; maintains a bounded pending buffer (8 KiB);
detects protected spans (reasoning blocks, code fences, PEM blocks,
credential-shaped strings) *across* chunk and sentence boundaries; emits
`Speak` units that are fully validated — an unterminated protected span
at turn end is finalized as truncated-with-marker, and an overflowing
pending buffer never flushes an unvalidated fragment to TTS: it emits the
last fully validated prefix and records a `TruncatedByBudget` marker in
the report. Skipped/redacted/truncated speech is marked truthfully in the
report; the canonical assistant answer in history is never modified.
Punctuation/abbreviation/decimal handling and the unterminated-tail case
are fixture-pinned (PR-2 implements the engine; PR-0 fixes the contract
and the fixture corpus shape). Heuristic redaction is documented as a
mitigation, not proof that arbitrary text contains no secrets.

### 2.6 STT provider set (v1, planned)

| Adapter | Locality | Engine | Status |
|---|---|---|---|
| `stt-sidecar` | on-device | Rust adapter driving the **shipped Python sidecar** (embedded script, warm shared child, deadlines, bounded IO) | **Implemented PR-1** (feature `stt-sidecar`, default-off). Label: *Rust adapter with existing sidecar inference* — compatibility, not native Rust inference. `vad_filter=True` binding preserved; real-model probes recorded |
| `stt-http` | configured self-hosted remote | raw-PCM-POST/JSON-text (the pinned worker contract, verified) | **Implemented PR-1** (feature `stt-http`, default-off). Legacy-empty responses carry `LegacyEmptyResponse` provenance (D21) — never labeled confirmed silence. Redirects refused; TLS N/A for `http` self-hosted contract |
| Cloud STT | third-party cloud | real vendor | **Future optional feature; no concrete provider ships in v1.** Registered and advertised only after provider-specific auth, transport, credentials, egress/privacy, error/failover, fixtures and acceptance pass |

*(2026-09-23 amendment, approved by Alex)* v1's binding shipping
requirement is the production-capable **local** route (met). The
self-hosted remote adapter is an additional deployment shape; concrete
cloud STT is a **future optional integration** behind `VoiceStt` and its own
auth, transport, credential, privacy/egress, error/failover, fixture and
acceptance gates — never registered or advertised until a real adapter passes
them. Adding one must not change `VoiceSession`, capture, playback or the
reasoning-provider integration. The original "local + cloud
commitment" wording above is retained as the pre-amendment contract this
row supersedes.

### 2.7 TTS provider set (v1 — **local neural TTS selected, implemented, and user-accepted**; the header's earlier "open candidate" status is historical)

The previous "Piper (MIT)" entry was **incorrect as dependency approval**
and is corrected by primary-source verification (PR-0 execution report
§4): the original upstream engine is MIT but **archived**; the actively
maintained successor is licensed **GPL-3.0**, which the workspace supply
chain (`deny.toml`) does not permit for distribution, and it embeds an
espeak-ng phonemizer (GPL family) — the "MIT" label never applied to the
maintained line, and FFI/process isolation does not change license
obligations. **Resolution (supersedes the gated-candidate status this
section described at PRD-writing time):** Alex selected the process-
boundary option — Kokoro-82M via the ONNX-community export, per-user
opt-in Natural Voice pack, implemented and user-accepted
(`docs/foundation/voice-oracle-kokoro-implementation.md` and the
listening acceptance records). The historical gated-candidate text is
preserved above as the decision context. R3 as amended (2026-09-23)
binds v1 to the production-capable local route — satisfied — with cloud
TTS a future optional `VoiceTts` integration behind provider-specific auth,
transport, credential, privacy/egress, mandatory hygiene/redaction, fixture and
acceptance gates. It is not registered or advertised before those pass and must
not require changes to `VoiceSession`, capture, playback or the reasoning-provider
integration. A cloud-only preview
never satisfied either the old or the amended gate.

### 2.8 Telemetry and privacy (R12 hardened)

`VoiceTurnReport` carries stage timestamps (capture start/end, STT
start/final, agent dispatch/first token, first sentence, TTS open/first
audio byte, playback acks, turn end), provider/model identities,
interruption flags, tools observed, bounded error list, and
budget/truncation markers. **No audio bytes and no transcript text** are
carried in reports, error strings, interruption diagnostics, provider
failure reasons, or timing records: operational metadata only, from an
allowlist (stage names, counts, durations, class enums, provider/model
identifiers). Connection-failure/fallback events are observable without
leaking content. Debugging must not persist audio or extra transcript
copies as a side effect.

### 2.9 Configuration, speech egress, and activation

- Native Settings panel in the TUI (`/settings → Voice`): provider selection,
  voice profile and capability-aware partial-preview presentation. ACP v1
  advertises `promptCapabilities.audio = false` and has no microphone,
  playback, voice-status, speech-provider or Voice Settings surface; this is a
  documented host exclusion, not an invented protocol extension. Manual
  config-file editing is never the normal activation path.
- Credentials via the existing OS credential-manager port per provider.
- Local-model dependency setup joins the existing native dependency-setup
  surface; no parallel bootstrap.
- **Speech-egress policy (binding).** Three classes: `OnDevice`,
  `SelfHostedRemote` (configured endpoint), `ThirdPartyCloud`. Local-only
  speech (STT or TTS declared `OnDevice`) must never send microphone
  audio or speech text to either remote class; a provider failure must
  not silently change the policy — failover across egress classes
  requires the selected provider and data flow to be permitted by
  configuration (and cloud fallback additionally requires the existing
  credential/permission mechanisms). The configuration validator
  enforces consistency (e.g. a cloud TTS selected while speech is
  declared local-only is a validation error, not a runtime surprise).
- **Redaction is not a settings switch.** Pre-cloud hygiene (§2.5) is
  mandatory for any `ThirdPartyCloud` synthesis target and cannot be
  disabled by a generic setting. A separate, clearly named
  *local presentation* option may exist later for on-device TTS only.
- Absent voice configuration: no voice tools, no adapters constructed, no
  startup side effects — R9's baseline (§3) holds.
- **Local speech ≠ local assistant.** The reasoning provider keeps its
  separate configuration and privacy consequences; on-device STT/TTS does
  not make the assistant local, and docs must not imply it.

## 3. Requirements (acceptance-derived)

R1–R12 identifiers are preserved verbatim in intent; corrections of
wording noted inline. New traceable requirements carry fresh IDs.

R1 The subsystem contains no agent reasoning or tool-execution logic;
   `cargo xtask architecture` enforces the crate's dependency allowlist
   (`vesper-domain`, `vesper-security` only) and the crate contains no
   tool-dispatch or runtime/provider imports. *(Tested: arch gate +
   PR-0 dependency test.)*
R2 *(amended 2026-09-23, approved by Alex)* STT providers are
   interchangeable behind one provider-neutral port. VRO-17 v1 must ship
   **at least one production-capable local STT route**. Self-hosted and
   third-party cloud STT adapters are **future optional features**
   that may be added independently through `VoiceStt` after their own auth,
   transport, credentials, privacy/egress, error/failover, fixtures, and
   acceptance gates. The architecture,
   configuration, credential seams, failover contracts, and egress policy
   must permit such adapters. **No cloud provider is considered supported
   until a real adapter passes its gate.** Selection is configuration,
   not code. *(Supersedes the original "at least one local and one cloud
   adapter ship" shipping mandate; the self-hosted HTTP adapter remains
   an additional deployment shape, never a cloud substitute. Historical
   wording preserved in §7 D12/D14.)*
R3 *(amended 2026-09-23, approved by Alex)* TTS providers are
   interchangeable behind one provider-neutral port. VRO-17 v1 must ship
   **at least one production-capable local TTS route**. Third-party cloud
   TTS is a **future optional feature** through `VoiceTts`. Any future cloud TTS
   adapter must pass provider-specific auth, transport, credentials, privacy/egress,
   mandatory pre-cloud redaction (R7), fixtures, and acceptance gates
   before it is considered supported. *(Supersedes the original
   local+cloud shipping mandate. Satisfied on the local side by the
   accepted Kokoro Natural Voice route; cloud TTS remains unimplemented
   and is not advertised. Historical wording preserved in §7 D13/D14.)*
R4 *(amended 2026-09-23, approved by Alex — Option B)* Live partial
   transcripts are an **optional STT-provider capability**, not a
   mandatory v1 requirement. Each STT provider declares
   `BufferedRepass`, `Incremental`, or no partial support. When enabled
   **and** supported, partials are display-only, best-effort, bounded,
   cancellable/stale-safe, never submitted as turns, and always
   superseded by the final transcript. **A final-only provider remains
   fully conformant with VRO-17.** Guarantees retained: no turn
   submission from a partial; final always supersedes;
   stale/generation-mismatched partials rejected; capability declared
   honestly by the adapter; no fake partials for final-only providers;
   `PartialGate` remains reusable infrastructure; no provider is required
   to emulate partials via repeated full inference. **Status: PASS**
   (`foundation/voice-r4-optional-partials-execution.md`: capability-aware
   Settings with truthful unavailability for every current final-only
   backend; preference preserved; two-provider neutrality guard; all
   current adapters final-only — sidecar, HTTP, FLM). Historical PR-1
   implementation evidence (`PartialGate`, repass contracts, fakes)
   remains as infrastructure at its recorded scope.
R5 Sentence-gated TTS streams while the agent stream continues; first
   audio latency is measured per turn. *(PR-2/PR-3.)*
R6 Barge-in and Stop stop speech and request transactional runtime
   cancellation through the existing contract; the next turn receives a
   bounded interruption note built from acknowledged playback only,
   clearly labeled as Vesper-generated context, as ordinary content; no
   tool-call replay occurs. *(Contract: PR-0 types + §2.4; state machine:
   PR-3.)*
R7 Secret redaction runs before any third-party-cloud synthesis and
   cannot be disabled by a generic setting; canary fixtures fail the
   build on regression. Local presentation options, if any, are named
   separately. *(Contract: PR-0 egress policy; engine + fixtures: PR-2.)*
R8 Every accepted voice turn produces exactly one terminal
   `VoiceTurnReport`; absent stages are absent, never fabricated.
   *(Report type + uniqueness rule: PR-0; state-machine enforcement:
   PR-3.)*
R9 With voice unconfigured, the **deterministic observable surfaces**
   of both hosts are identical to pre-voice state: serialized tool
   registries, advertised capability sets, and protocol output
   sequences. Claimed scope is exactly those surfaces — not binary
   bytes, wall-clock startup, or nondeterministic identifiers (build
   paths/timestamps are normalized or excluded from comparison).
   Separately: no voice-specific startup side effects occur when
   unconfigured (no device probes, no adapter construction, no files).
   *(Baseline snapshot mechanism: PR-0 documents it; host proof: PR-4.)*
R10 The existing terminal dictation surface keeps its acceptance suite
   green throughout; its re-hosting (PR-4) preserves all
   `voice-control-prd.md` behaviors, including the F5/Stop/Retry/Discard
   lifecycle and the binding VAD behavior.
R11 Provider absence fails truthfully (`Unavailable`); VAD-silence is
   `NoSpeech` (nonfatal outcome); engine failure is `Inference`/
   `Unavailable` and visible. No exception-to-silence conversion.
R12 No audio or transcript content in telemetry, error strings,
   interruption diagnostics, provider responses, or timing records;
   operational metadata comes from an allowlist. Debugging never
   persists audio or extra transcript copies. *(Report/enum design:
   PR-0; audit of adapter paths: PR-1/PR-2.)*
R13 *(new)* Capture is user-initiated with manual interruption in v1;
   no always-on listening, wake word, or automatic conversational
   turn-taking. VAD **inside STT** (silence filtering) is a different
   feature from VAD-triggered auto-capture and remains enabled.
R14 *(new)* Speech egress is policy-gated (§2.9): on-device-declared
   speech never reaches remote classes; failover across egress classes
   requires configuration permission; failures never silently change the
   policy. *(Validator: PR-0; runtime enforcement: PR-1+.)*
R15 *(new)* Playback progress is acknowledged host-side per speech
   segment and generation; unknown progress is represented as `Unknown`,
   and "heard" claims are never derived from synthesis completion alone.
   *(Types: PR-0; enforcement: PR-3.)*
R16 *(updated by Alex's latency follow-up; clarified by the continuation
   directive; amended 2026-09-23 with Alex's approved scope decisions)*
   Native NPU support exists for STT and TTS as **separately gated,
   capability-conditional stages**; CPU remains first-class everywhere.

   **R16a — STT.** CPU remains supported. NPU STT is supported only where
   a verified compatible backend/model/runtime exists. Automatic uses
   verified acceleration when available, otherwise CPU. Explicit
   NPU-required mode refuses truthfully when unsupported.
   Selected-versus-actual backend and evidence scope remain observable.
   (Current state: the FLM/NPU STT route is production-implemented,
   verified-only, and user-accepted at its documented scope.)

   **R16b — TTS.** NPU TTS is **optional and capability-gated, not an
   unconditional v1 blocker**. CPU TTS is first-class. If no verified
   compatible backend/model path exists for the selected TTS
   engine/platform, VRO-17 remains **conformant on CPU**. Any future NPU
   TTS route must pass backend/model/inference/offload/quality/latency
   acceptance before advertisement. CPU Kokoro is never relabeled as
   NPU-backed. (Current state: no compatible Linux Kokoro NPU path is
   established; synthesis is CPU and user-accepted.)

   **R16c — common policy.** Native Settings owns consented setup and
   activation; selected-versus-actual backend, fallback/refusal reasons
   stay explicit; existing NPU runtimes/workloads are protected; no
   hidden driver/runtime changes; no unsupported downloads or
   initialization; STT and TTS readiness resolve independently.

   **Clarified (original directive, retained):
   support is conditional per machine, platform, backend, model, and stage —
   speech work is never assigned to an NPU merely because an NPU adapter is
   supported or the machine has one. On a machine without a compatible,
   usable NPU path, ordinary CPU voice must remain available without NPU
   installation, without warnings framed as voice failures, and without any
   acceleration requirement.** Selection is the landed pure-core stage
   policy (`vesper_voice::execution`: CPU / Automatic-verified-only /
   strict-NPU per stage, evidence-based readiness facts, in-process
   readiness caching, honest placement attribution; see
   [capability-gated execution](foundation/voice-capability-gated-execution.md)).
   Device/runtime detection alone never establishes accelerated voice. See
   the current NPU gate table below and
   `foundation/voice-first-speech-and-npu-assessment.md`. Acceptance for
   any *implemented* accelerated route still requires a validated
   model/backend path and real measured offload and quality/latency
   evidence (R16a meets this at its recorded correlation-level scope).
R17 *(new, PR-2-proved)* Production TTS creates **zero** audio files
   and no persistent response-audio cache: synthesis returns bounded
   in-memory PCM only (tested; storage receipts in
   `docs/foundation/voice-oracle-pr2-execution.md`).
R18 *(new, PR-2-proved)* Optional generated probe audio ≤8 MiB per
   file, ≤32 MiB aggregate per work unit, cleaned after every outcome
   (measured: 334 KiB peak, 0 residual).
R19 *(new, PR-2-pinned)* Any future optional voice disk write refuses
   when destination free space is unknown or below a ≥1 GiB reserve;
   never silently switches destination (contract test).
R20 *(new, binding PR-4; CLOSED 2026-09-23)* Shipped dictation capture
   must gain explicit duration/file-size caps, aggregate temporary-usage
   accounting across dictation/conversation instances, and
   cleanup-after-every-outcome + crash-recovery acceptance. **PASS**
   (repaired in `foundation/voice-r20-default-capture-repair.md`): every
   explicit capture — default-build F5 dictation and feature-build F9
   conversation alike — is owned by the one managed store (120 s/4 MiB
   writer caps, 32 MiB aggregate, 1 GiB reserve, lease-backed recovery,
   zero residual after every outcome; PTY-green on both paths). The
   historical feature-only store coverage and the audited default-build
   gap are preserved as history in that record.

## 4. Evidence and traceability

- Upstream mechanism analysis with `file:line` citations:
  `docs/architecture/recon_voice_oracle.md`.
- This PRD's design decisions trace to that reconnaissance (§ refs) and
  to existing Vesper contracts (ADR 0006/0007/0028, VAD PRD).
- Implementation evidence will land in `docs/foundation/` per PR as
  `voice-oracle-pr{N}-execution.md`, indexed in
  `docs/foundation/evidence-index.md`, and enrolled through ADR 0028
  acceptance on completion claims.

## 5. Migration plan (staged, gated)

Each PR lands green on: `cargo test --workspace --all-features` (floor
never drops), strict Clippy, fmt, `cargo xtask architecture` (voice crate
rules added in PR-0), `cargo xtask naming-guard`, and
`cargo xtask acceptance` where acceptance items change. PR-0 additionally
proves the crate compiles with **no features** and with `--all-features`.

**PR-0 — Contracts and architecture rails.** ✅ COMPLETE (see
`docs/foundation/voice-oracle-pr0-execution.md`): `vesper-voice` core
types, ports, descriptors, error taxonomy, PCM framing/validation,
configuration + egress-policy contracts, cancellation token, host/core
event vocabulary, report type, in-memory fakes exercising the async/
error/cancellation ports, architecture allowlist + naming-guard alias
tokens, and contract tests. **Explicitly not implemented:** adapters,
capture loops, model loading, runtime activation, host UI, live audio.

**PR-1 — STT adapters, failover, partials. ✅ COMPLETE** (execution:
`docs/foundation/voice-oracle-pr1-execution.md`). Landed: `stt-sidecar`
(Rust adapter + shipped sidecar inference; warm child; process-group-owned
reaping; bounded line IO; setup-prerequisite unavailability), `stt-http`
(validated endpoint; redirect refusal; body/transcript bounds; truthful
401/429/malformed/oversized mapping; D21 provenance), `FailoverStt`
(frozen policy executable: only `Unavailable` advances; per-attempt egress
recheck; attempt bound; sanitized trace), `PartialGate` (buffered-repass
partials: whole-capture budget, coalescing, final priority, stale/generation
rejection; no turn-submission API exists), `ThreadPoolExecutor`+`run_blocking`
(cancellable bridge with an honest computation-continues limit), D21
provenance contract, D22 features policy, and the pr1 adapter suite (30
tests over real adapter code with subprocess/TCP fixtures). Real-model
probes ran (cached `tiny`/`base`, VAD on: silence 30 s, long silence 90 s,
tone-burst 30 s → all empty, no hallucinations; receipts in
`voice-oracle-pr1-probe-results.json`). Native-STT feasibility verdict
recorded (§7 PR-1 report): **suitable with named prerequisites**, not
implemented. *Allocated-and-landed acceptance:* bounded queues, partial/
final ordering, policy-constrained failover, cancellation cleanup cycles.

**PR-2 — Synthesis-only TTS, hygiene/gating, storage bounds. ✅
COMPLETE (implementation)** (`docs/foundation/voice-oracle-pr2-execution.md`):
`hygiene.rs` (stateful bounded sentence gate: cross-chunk protected
spans — reasoning/code/PEM — credential/blob redaction, overflow
loudness, exactly-once finalize, CJK ending; 16 tests), `tts-subprocess`
(synthesis-only system-engine adapter: fixed argv, stdin text,
streaming-WAV validation with placeholder sizes ignored, 22050→16000
conversion, no files anywhere, cancellation/cleanup, 19 tests), engine
gate PASS as optional user-installed baseline (no bundling, no
`deny.toml` change, zero new deps), real no-speaker probe receipts,
storage receipt (probe peak 334 KiB, residual 0; no persistent voice
files/logs). (Historical at PR-2's date: R3's then-local+cloud gate was unmet. Under
the 2026-09-23 amendment R3 binds to the production-capable local route —
satisfied by Kokoro — with cloud TTS an optional gated integration.)
Egress enforcement for cloud TTS lands with any cloud adapter
(local-only synthesis needs none). **Original PR-2 — TTS adapters, hygiene engine, egress enforcement.** Local TTS
decision executed per §2.7 (Alex's selection); hygiene engine +
cross-chunk/cross-sentence protected-span fixtures (reasoning/code/PEM/
credentials/links split across chunks); mid-stream failure semantics
against fakes first, real adapters after. *Allocated:* cross-chunk
hygiene, budget-overflow behavior, R7 canaries.

**PR-3 — Voice session orchestration. ✅ COMPLETE** (`docs/foundation/voice-oracle-pr3-execution.md`):
`session.rs` reducer over the PR-0 events producing typed `VoiceEffect`s
(exactly-one `SubmitTurn`; transactional `CancelRuntimeTurn`; urgent-
ordered `StopPlayback`/`CancelSynthesis`); five-dimension overlap kept;
D21-provenance submission gating; PR-2 hygiene fed exactly once;
pre-identity cancellation intent applied to the matching late run;
idempotent Stop; bounded pending input with visible rejection; playback
receipt validation (identity/monotonic/gap) with honest frozen notes;
metadata-only reports with absent stages absent; 29 tests across the
four scenario families against the production session with simulated
hosts (not device evidence). Reachable-state-by-event table recorded in
the execution report. **Original scope — PR-3 — Voice session orchestration.** State machine over the §2.3
dimensions; barge-in/stop races (during TTS stream, during agent stream,
during STT, before run identity, after agent completion with audio
queued, repeated before cleanup settles); stale-generation rejection;
playback-ack correlation; terminal-outcome uniqueness; interruption-note
construction from acked playback. *Allocated:* all §3 race/uniqueness
cases.

**PR-4 — TUI integration, controlled playback, capture storage. ✅
COMPLETE (implementation)** (`docs/foundation/voice-oracle-pr4-execution.md`):
`voice-conversation` feature (default-off; default TUI suite count
unchanged 395/0 = R9 parity); `ConversationController` executing
PR-3 effects on the real seams (submit/cancel/stream/settlement;
urgent playback-first Stop; late-identity deferred cancel; speech
failure preserves text+runtime outcome); `PlaybackOwner` (bounded
512 KiB queue, own aplay child over stdin raw PCM, no files, verified
receipt semantics — `BytesWritten`=transport, `Drained`=player
finished our bytes, `Unknown` otherwise); R20 store (120 s/4 MiB hard
writer caps, 32 MiB cross-instance aggregate via live-PID+start-time
leases, ≥1 GiB reserve with unknown-space deferral, dead-lease-only
recovery, symlink/foreign preservation); F9 activation (conflict-free)
with Settings → Voice readiness truth; PR-3 note wording corrected to
evidence-explicit form. Synthetic **fixture STT gate CLOSED 6/6**
(case-insensitive exact match, pre-selected criterion; first-attempt
failure recorded honestly; trailing-silence + silence controls pass).
**User-operated device acceptance:** recorded OPEN at PR-4's own date;
subsequently satisfied in separately indexed records — the continuity and
NPU-conversation listening records plus the 2026-09-24 R6 real-device
interruption/recovery acceptance. One-press and repeated barge-in, explicit
Stop, exactly-one replacement submission, no stale-speech resume and later-F9
recovery passed on the tested setup. No numeric, every-platform or every-provider
claim is inferred (see the final completion audit).
**Original scope — PR-4 — TUI host
integration.** Voice conversation mode; dictation
re-host decision executed (either outcome preserves R10 and records the
compatibility-path retirement/retention per §5 note); native Settings →
Voice; dependency-setup integration; R9 baseline proof on deterministic
surfaces. Real-device microphone acceptance is Alex's run, never
fabricated; assistant-audio-retranscription/false-interruption is a
required real-device test case here (hands-free behavior is out of v1
scope, so the failure mode must be shown not to occur in the supported
push-to-interrupt flow).

**PR-5 — ACP host + release.** The real ACP process advertises audio support as
false. ACP therefore documents the microphone, playback, live-status, Voice
Settings and speech-provider-control exclusion while retaining shared generic
runtime/provider and cancellation semantics. No protocol extensions are
invented and no raw PCM or local audio/status bytes pollute ACP stdout. The TUI
ships the local voice features through the exact-commit release gate.

**Native scope definition (ADR 0028 alignment).** "Native" for VRO-17
means Rust orchestration plus, per component: inference engines either
Rust-native or explicitly bounded external processes with recorded
retirement/retention decisions (the shipped Python dictation sidecar is
the existing compatibility path; its retirement or honest retention is
decided at PR-4, not silently extended); audio capture via OS tools or a
Rust backend chosen at PR-4; no new Python production dependency hidden
behind a Rust wrapper. Rust orchestration alone never gets labeled
"native inference".

**Current NPU requirement and readiness.** STT and TTS remain separately
gated. The historical PR-0 no-userland/model observation is superseded: the
current machine has AMD XDNA, FLM 1.0.5, XRT 2.26.0 and the hash-verified Whisper
pack. The production `voice-flm` route composes CPU Silero VAD with owned FLM/NPU
recognition, registers only after in-process Verify, and is exercised through
Settings → Save → F9 by the no-device production-path harness. Placement strength
remains process/device/model correlation because the endpoint exposes no
per-request offload receipt. Alex's 2026-09-23 listening statement is positive
for the tested NPU-enabled conversation, but its exact historical executable is
still unidentified. No compatible Linux Kokoro NPU path is established; Kokoro
synthesis remains CPU-backed and conformant under amended R16b. CPU stays usable;
setup/activation belongs in native Settings. The current controlled-acceptance
candidate and release limits are recorded in
[`foundation/voice-vro17-release-readiness-audit-2026-09-24.md`](foundation/voice-vro17-release-readiness-audit-2026-09-24.md).

Deferred (explicit): wake-word capture, auto-turn-taking, multi-client
serving and browser HUD. Native NPU STT/TTS are now separately gated requested
work, not removed from scope by this historical deferral list.

## 6. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Local TTS engine unfit or unlicensable for distribution | §2.7 gated-candidate decision at PR-2 entry with three explicit options; a cloud-only preview is labeled a preview and cannot satisfy R3 — satisfying R3 requires either a permissive engine or Alex accepting a recorded scope change |
| Native audio capture cross-platform cost | v1 reuses the proven recorder pattern; native capture is a feature-gated later increment, never blocking |
| Sentence gate clipping mid-code speech | Hygiene handles code blocks as protected spans across chunk boundaries before speech units are emitted; fixtures pin it (PR-2) |
| Barge-in races corrupting turn state | PR-3 dimension model + dedicated race-test matrix; cancellation only through the runtime's existing path; generation stamps reject stale output |
| Scope creep toward an agent-in-voice | R1 architecture guards fail the build on dependency or symbol drift |
| Future cloud STT/TTS vendor integration | No provider is selected or advertised in v1. A future adapter enters through `VoiceStt`/`VoiceTts` only after its provider-specific security, transport, fixture and acceptance gates pass |
| NPU expectations inflate | R16: separate STT/TTS model/offload gates; native Settings consent, truthful actual backend and preserved CPU path |

## 7. Decision record (PR-0 corrections, with evidence and tests)

Each row: finding → disposition → evidence → acceptance. Historical
reconnaissance documents are unchanged; corrections live here and in the
PR-0 execution report.

| # | Finding | Disposition | Evidence | Test/gate |
|---|---|---|---|---|
| D1 | PRD banned runtime/agent/harness/provider deps yet described consuming `ProviderStreamEvent` | Confirmed contradiction. Core consumes **voice-owned** inputs; hosts translate runtime/provider events at the boundary | `crates/vesper-provider/src/stream.rs:41` (type lives in a forbidden dep); §2.3 host contract | `cargo xtask architecture` allowlist; crate has no provider import |
| D2 | Cancellation-token ownership unresolved ("re-export without dependency" was incoherent) | Confirmed. Local `VoiceCancel` token owned by `vesper-voice`; hosts construct/cancel alongside runtime cancellation | `crates/vesper-runtime/src/cancellation.rs` shape mirrored, not imported | PR-0 fake-port tests exercise cancel/wait; compile proves no dep |
| D3 | Feature-gated adapters "in the crate" could smuggle forbidden deps via features | Confirmed. Pure-core boundary: no adapter code in PR-0; future adapters evaluated per-PR; features never grant dependency ownership | `xtask/src/main.rs:1379` allowlist is feature-blind | arch gate fails any vesper-voice → forbidden dep |
| D4 | Sequential `AgentTurn → Speaking` model hid generation/playback overlap | Confirmed. Five independent dimensions + projected phase | Reference overlap evidence (recon §4) | PR-3 dimension matrix tests; PR-0 types |
| D5 | "Last spoken sentence" claimed heard-ness without proof | Confirmed. Playback acks = device-handoff estimates; `Unknown` when unobservable; note wording bound | Recon §7; §2.3 | R15 types in PR-0; PR-3 enforcement |
| D6 | TTS stream had no mid-stream error channel | Confirmed. `TtsChunk::{Audio,Finished}` + `TtsMidStreamError` | upstream closed responses in `finally` because mid-stream failure is real (recon §3) | PR-0 fake TTS emits mid-stream failure test |
| D7 | Partials kind unlabeled (repass vs incremental) | Confirmed. `PartialsKind::{BufferedRepass,Incremental}` in descriptor | Reference implements periodic repass (recon §2) | PR-0 descriptor tests |
| D8 | `EmptyTranscript` conflated no-speech with outage in failover | Confirmed. Split: `NoSpeech` (nonfatal, stops chain) vs `Unavailable` (advances) | Recon §2 conflation noted | PR-0 error taxonomy tests |
| D9 | R7 mandatory redaction vs Settings "redaction on/off" contradiction | Confirmed. Cloud redaction non-disableable; separate local-presentation-only option | §2.9 | PR-0 config validator rejects cloud-TTS-without-hygiene; PR-2 canaries |
| D10 | "Local" STT/TTS could imply local assistant | Confirmed. §2.9 explicit separation | — | docs; R12/R14 |
| D11 | Hygiene-before-gate vs gate-before-hygiene inconsistency | Confirmed. One stateful bounded pipeline; hygiene on gated segments; cross-boundary span tracking | §2.5 | PR-2 fixtures (contract frozen in PR-0) |
| D12 | R2 "non-local" vs Goals "cloud" wording drift | Confirmed. Reconciled to local+cloud; self-hosted remote is additional | §2.6 | R2 text |
| D13 | "Piper (MIT)" as approved dependency | **Refuted by primary source.** Original repo archived (MIT); maintained successor GPL-3.0 + espeak-ng phonemizer; deny.toml permits neither for distribution | PR-0 execution report §4 receipts | R3 cannot pass on cloud-only; §2.7 options for Alex |
| D14 | Risk table's cloud-only contingency could read as satisfying R3 | Confirmed. Relabeled as contingency requiring recorded scope change | §6 row 1 | R3 text |
| D15 | Always-on/wake-word ambiguity vs VAD-inside-STT | Confirmed. R13: user-initiated capture, manual interruption; internal VAD stays enabled | `docs/voice-trailing-silence-vad-prd.md` | R13; VAD kwarg asserted in existing suites |
| D16 | NPU enabled on Alex's laptop → risk of invented accelerator layer | Historical PR-0: device present, no userland/inference evidence then. Current assessment supersedes userland absence; speech offload remains unverified | Execution report §5 sysfs receipts | R16 |
| D17 | Capture buffer bounded only for partial window (reference) | Confirmed. `CaptureBudget` bounds whole capture + all queues; overflow policy explicit per queue | Recon §7 (30 s partial bound only) | PR-0 config/validation; PR-1 enforcement |
| D18 | Unimplemented upstream `ack` feature risk of inheritance | Already handled (recon §9 gap); preserved as non-goal | recon gap receipt | non-goal list |
| D19 | R9 "byte-identical hosts" overclaimed binaries/startup | Confirmed. Re-scoped to deterministic observable surfaces + no-startup-side-effects | §3 R9 | PR-4 baseline snapshot |
| D20 | Direct-chat fallback, broad exception swallowing, unbounded buffering from reference | Already handled as refusals (recon §13); restated binding | recon §13 | R11/R12/R6; arch + tests |
| D21 *(PR-1)* | Legacy pinned worker collapses engine exceptions and silence into empty text; a client cannot recover that distinction | Confirmed. `TranscriptProvenance::{InferredText,VadConfirmedSilence,LegacyEmptyResponse}` added to `SttTranscript` (smallest contract correction); the HTTP adapter emits `LegacyEmptyResponse`; the sidecar adapter (whose script reports VAD-filtered results) emits `VadConfirmedSilence`; empty `InferredText` is an adapter obligation violation | Pinned worker source (recon §2, exception→empty); shipped script's `vad_filter=True` | `stt_http` legacy test; `stt_sidecar` empty test; pr1 suite |
| D22 *(PR-1)* | PR-0 pinned "no `[features]`" — impossible with adapters landing | Confirmed as staged truth. Features `stt-sidecar`, `stt-http` declared default-off, zero production dependencies; pure core still builds bare | Manifest + `pr0_contracts` amended test | `cargo build --no-default-features`; amended manifest test asserts default-off + no `dep:` |
| D23 *(PR-2)* | Directive: synthesis adapter must not speak/play/fabricate playback evidence | Confirmed as binding contract; `tts-subprocess` captures stdout to memory, returns canonical PCM only; exit ≠ playback; no ack exists | Directive §2; recon synthesis+playback shortcut explicitly not taken | pr2 suite (no-file proofs, mid-stream cancel, no playback path) |
| D24 *(VRO-17 continuity repair)* | Routine 28/48-character successor cuts accumulated model fade quiet, while streamed `1.` list prefixes could reach Kokoro as empty-phoneme requests | Use bounded-first-piece plus sentence-level/remainder units; keep list markers attached to their item. Meaningful numeric/symbol input and unexpected zero-phoneme failures remain truthful | `docs/foundation/voice-continuity-phoneme-repair.md`; `docs/foundation/voice-continuity-listening-acceptance.md` | 13 formatting boundary cases, superseding pipeline fixture, four PTY modes; listening accepted 2026-03-03 with residual delay open |
| D25 *(VRO-17 boundary repair)* | The depth-0 handoff pinned the producer through the lane's write+drain, so successor synthesis slower than the previous unit's playback landed as boundary dead air (recording review 2026-09-23; red at 1.019 s) | Keep the bounded bank — depth 2 absorbs ~one unit's audio of synthesis latency — never a turn buffer; overlap contract, Stop/Replace generation invalidation and all byte bounds unchanged | `docs/foundation/voice-continuity-boundary-repair.md` | Boundary-gap regression (red→green), bounded-bank assertions in the pipeline fixture, four PTY modes + FLM PTY on the repaired candidate; acoustic acceptance remains Alex's |
| D26 *(VRO-17 short-reply quality repair)* | Cold/warm instrumentation proved no player starvation, while the real hygiene→Kokoro→worker path measured 0.972 s of stacked model padding at a two-sentence short-reply boundary. The Settings phrase “Partial transcripts · unavailable” also made healthy final transcription look unavailable. | Apply guarded low-amplitude edge cleanup only to short Kokoro units (≤64 chars): 50 ms retained at artificial piece joins, 100 ms per side at sentence joins, first onset preserved; leave long units/system speech untouched. Present the optional feature as “Live transcript preview” and state that final text still appears after Stop. | `docs/foundation/voice-short-reply-quality-repair.md` | Real-model full-path red 0.972 s → green 0.200 s for both installed voices; rejected broad trim exposed 2.15–2.21 s long-form starvation, narrowed policy restored 0.277–0.452 s paced envelope; combined tests + F5/CPU-F9/NPU-F9 candidate loops green |
| D27 *(VRO-17 R6 device acceptance closeout)* | The §2.4 binding repair was automated-verified but still required Alex's real-device interruption/recovery evidence; an earlier brief also misread acceptable Ctrl+C cancellation as a blocker. | Accept the bounded real-device matrix without inventing platform/provider/latency coverage; preserve the clarification that old speech did not resume, Stop did not open capture and the tested cancellation message was acceptable. | `docs/foundation/voice-r6-device-acceptance-closeout.md`; `docs/foundation/voice-r6-device-interruption-acceptance.md` | A/B/C/D + later F9 PASS; old speech resumed NO; accidental Stop capture NO; R6 PASS on tested setup; R1–R20 closed, PR-5 only open |
| D28 *(VRO-17 PR-5 product decision)* | Concrete third-party cloud STT/TTS providers are not VRO-17 v1 release blockers, but deferral must not weaken provider neutrality or speech-egress safety. | Ship the accepted local stack. Treat cloud speech as a future optional feature through the existing `VoiceStt`/`VoiceTts` contracts; require explicit permitted cross-egress configuration, existing credential seams, provider-specific auth/transport/privacy/error/failover/fixtures/acceptance, and mandatory pre-cloud TTS hygiene/redaction. Never silently fall back from local speech or advertise an adapter before its gates pass. | §§2.1, 2.6–2.9; R2, R3, R7, R14; PR-5 execution record | Future adapters require no `VoiceSession`, capture, playback or reasoning-provider branch; v1 truthfully advertises no cloud speech provider |

**Preserved stronger behaviors (explicit):** VAD enabled (Vesper
evidence over reference practice); transactional cancellation with
no-ambiguous-fragment replay (ADR 0007 + FinishOutcome::StreamInterrupted
classification) over the reference's stream-drop shortcut.

## 8. Traceability matrix

Reference mechanism → requirement → owner → test → deviation → stage.
"Ref" rows cite `docs/architecture/recon_voice_oracle.md` sections.

| Ref mechanism | Requirement | Owner | Test | Deviation | Stage |
|---|---|---|---|---|---|
| STT failover remote→local (Ref §2) | R2 (permit-contract), R11, R14 | core policy + adapters | PR-1 failover tests (Unavailable advances, NoSpeech stops) | typed taxonomy; egress-gated; no production cross-class pair exists yet (correct under the amendment) | PR-1 ✅ |
| Partial repass with bounds (Ref §2) | R4 (amended: optional capability) | core `PartialGate` (reusable infra) | PR-1 partial/final ordering; `voice_provider_neutrality` capability tests | kind-labeled; never a command; final-only providers conformant (all current ones are) | PASS (optional) |
| Canonical 16 kHz i16 PCM (Ref §2/§7) | — (contract) | core `PcmFrame` | PR-0 framing/truncation tests | validated frames vs raw bytes; truncation visible | PR-0 ✅ |
| Odd-byte carry (Ref §3/§7) | — | `PcmReassembler` | PR-0 reassembler tests | same mechanism, plus end-of-stream truncation error | PR-0 ✅ |
| Sentence gate pipelining (Ref §3/§4) | R5 | hygiene pipeline | PR-2 gating fixtures | one stateful bounded pass; overflow never flushes unvalidated | PR-2 |
| Streaming list-prefix hygiene + synthesis subdivision | R5 | hygiene pipeline + TUI synthesis worker | `hygiene_formatting_units` (13), subdivision fixture, four PTY modes, device listening | marker joins its item; bounded early piece then sentence-level/remainder successors; 5.2 s cold-start successor gap remains a historical measurement on that candidate | accepted 2026-03-03 (superseded as the present user verdict by the 2026-09-23 NPU acceptance, `voice-npu-user-acceptance.md`; kept as history) |
| Native NPU speech integration | R16 | TUI composition adapters + existing stage selector | `docs/foundation/voice-npu-integration-execution.md` | FLM NPU STT integrated and PTY-receipted 2026-09-22: shared Silero VAD worker, owned loopback FLM lifecycle, final-only adapter registered behind real verification, Settings Verify/Save, and the production F9 path proven to route through the FLM adapter (a CPU-sidecar defect in F9's stop path was found by the PTY test and repaired — `voice.rs` conversation-STT slot; Settings save → Verify → F9 → FLM adapter → one agent turn → unchanged CPU Kokoro PCM, CPU recognizer never constructed). Superseded 2026-09-22/23 by the continuity boundary repair (`voice-continuity-boundary-repair.md`): the FLM-session CPU adapter had silently dropped `VESPER_PYTHON_PATH`/`GLM_VENV_PATH` precedence (legacy PTY loop caught it; older candidates pass, the FLM candidate failed), and the readiness line conflated availability with selection — both repaired with red/green evidence; the boundary-gap bank also covers the recording-class pauses. Final-only; the installed handler still ignores its cancellation token (owned teardown covers it). Natural-speech accuracy remains unmeasured quantitatively; Alex's live-device listening acceptance is now recorded positive at its scope (2026-09-23, `voice-npu-user-acceptance.md`; tested artifact association pending). Kokoro Linux-NPU path unestablished (synthesis stays CPU; the user's NPU wording covers recognition). CPU remains unchanged | IMPLEMENTED / user listening accepted for current use (quantitative accuracy + artifact identity open) |
| Secret redaction pre-cloud (Ref §3) | R7, R12 | hygiene + egress policy | PR-2 canaries; PR-0 validator | non-disableable for cloud; local-only option separate | PR-0 contract ✅ / PR-2 engine |
| Barge-in cancel + last-heard note (Ref §6) | R6, R15 | host + core | PR-3 race matrix; `voice_r6_binding::speaking_f9_is_one_gesture_genuine_barge_in`; repeated-cycle regression; device acceptance | acked-playback-only note; runtime-path cancellation; labeled context; current F9 production handler executes stop + cancel + immediate capture in one gesture. Device test confirms old speech stops and does not resume, one replacement turn, repeated recovery | R6 PASS 2026-09-24 ✅ |
| Stop run / explicit Stop control (Ref §6) | R6 | host | PR-3; `voice_r6_binding::explicit_stop_stops_everything_and_opens_no_capture`; device acceptance | complete stop/cancel semantics with no new capture; current `cancel_turn` binding invokes the host Stop path while voice speech is active; tested cancellation message accepted | R6 PASS 2026-09-24 ✅ |
| Turn latency telemetry (Ref §9) | R8, R12 | report type | PR-0 report tests | metadata allowlist; no content | PR-0 ✅ |
| Named-session reuse (Ref §5) | — | existing runtime | existing session tests | unchanged; voice adds no store | n/a |
| 404-recreate session (Ref §5) | — | existing runtime | existing | unchanged | n/a |
| SSE event vocabulary (Ref §4) | R1 | host translation | PR-3/PR-4 host tests | voice-owned enums; no provider types in core | PR-3/4 |
| Plugin→voice callback (Ref §11) | — | future hosted-tool seam | deferred | not in v1 scope | deferred |
| Worker/GPU remote STT (Ref §2) | R2 (additional deployment shape) | `stt-http` adapter | PR-1 | self-hosted class; policy-gated; not a cloud substitute (R2 amended 2026-09-23) | PR-1 ✅ |
| Python sidecar STT (existing Vesper) | R10 | TUI (existing) | existing PTY suites | retained; retire/retain decided PR-4 | PR-4 |
| Warm-once init (Ref §8) | — | adapters | PR-1 | concept noted; no shared state in core | PR-1 |
| — (new) egress policy | R14 | config validator | PR-0 validation tests | no upstream equivalent | PR-0 ✅ |
| — (new) playback acks | R15 | host + core types | PR-3 | upstream claimed heard-ness | PR-0 types ✅ |
| NPU STT/TTS | R16a/R16b (amended 2026-09-23) | native backends + Settings required | first-speech/NPU assessment; `voice-npu-stt-implementation-progress.md`; `voice-vro17-final-completion-audit.md` | STT: production-implemented, verified-only, user-accepted at correlation-level scope. TTS: optional/capability-gated, no compatible path established, CPU conformant | R16a PASS — scoped limitation; R16b OPTIONAL (conformant on CPU) |


## Shared playback diagnostic follow-up

[Diagnostic repair evidence](foundation/voice-playback-diagnostics-repair.md) records
the device-free player-stderr repair. This does not close Preview/F9 audibility
acceptance or identify the cause of the reported Kokoro silence.

[Authorized real-output probe](foundation/voice-kokoro-authorized-playback.md): one
Kokoro worker/player attempt drained, but Alex subsequently confirmed he did not
hear it. The original listening result is negative, not unknown; the corrected
playback evidence below is a separate attempt.

## Kokoro silent-output correction

[PCM scaling repair and primary-source audit](foundation/voice-kokoro-pcm-scaling-repair.md)
reproduces the near-zero signal defect, records red→green conversion tests and
real direct/VRO amplitude receipts, and records Alex’s confirmation of one clear,
complete corrected Kokoro playback. This supersedes the earlier unresolved-cause
and unanswered-listening status for that defect, not full Settings/F9 acceptance.

## Voice latency requirement and repair evidence

Alex confirmed audible Preview/coding voice but requires immediate-feeling F9
recording and substantially faster replies; the reported 20-second delay is not
acceptable. [Latency repair evidence](foundation/voice-latency-repair.md) records
verified local overhead reductions, direct/VRO first-PCM measurements, retained
integrity checks and open real-device/provider latency acceptance. Fixture timings
are not an instant end-to-end completion claim.

[Sentence-pipeline repair](foundation/voice-speech-pipeline-repair.md) adds bounded
one-unit synthesis lookahead during playback, generation-safe Stop and separate
agent-wait/synthesis/player stage reporting. The serial control had a 2.412-second
software handoff gap; the overlapping check handed off within one 10 ms polling
interval, and Alex confirmed no perceived sentence delay in that check. The
initial live-provider reply-delay requirement is still open.

## Local first-speech follow-up and current NPU gates

[First-speech repair and NPU assessment](foundation/voice-first-speech-and-npu-assessment.md)
records one authorized fresh live turn (26.99 s to PCM before the local repair),
lossless bounded subdivision of long approved units (8.602 s whole preparation
versus 2.733 s first PCM on the fixed benchmark), user-confirmed complete/natural
playback, and the actual-frame wait-status correction. Saved model/reasoning
choices remain unchanged. There is no post-repair live end-to-end latency claim.

| R16 stage | Current status | Required next acceptance |
|---|---|---|
| NPU STT | PRODUCTION ROUTE IMPLEMENTED (2026-09-22, `voice-flm` feature, candidate `8264033f…14e7`): owned loopback FLM supervisor + VAD-composed final-only adapter + verified-only route registration + native Settings Verify/compute + F9 wiring; real-model no-device receipts green. **Verify reliability repaired 2026-09-23** (`voice-verify-read-failure-repair.md`, candidate `b289a6c1…`): Alex's user-tested `owned ASR read failed or timed out` reproduced verbatim through the native Settings route; cause proven from the vendor log (three prior-session orphaned owned-shape FLM servers exhausted NPU device contexts, so a fresh server's model load failed `DRM_IOCTL_AMDXDNA_CREATE_HWCTX (EINVAL)` and died, and the collapsed read-error label misreported the reset as a timeout). Owner-only repairs with red→green proof: error-kind classification (`process exited while answering` vs `read timed out`; no deadline change anywhere) and a pure-std durable child-registry + reaper anchored on registrar-host liveness (dead-host orphan reaped; live-host child proven untouched; default-build cfg defect fixed). Native Verify + F9 PTY re-receipted PASS against the exact candidate bytes. **Real-device user acceptance recorded 2026-09-23** (`voice-npu-user-acceptance.md`): Alex's verbatim *"Cooooollll all working with npu and there is not gaps its sounds natural"* — accepted for current use (perceived continuity/naturalness on the tested NPU-enabled setup; exact tested artifact association pending; not a measured zero-gap or placement claim) | Natural-speech accuracy fixtures through the F9/host path (quantitative), per-request placement evidence (endpoint has none), repeated-interruption and error/cleanup device cases, PTY menu-navigation hardening (the legacy `settings_pty.py` submenu click drifts on every preserved candidate and is tracked as pre-existing) |
| NPU TTS | **OPTIONAL / CAPABILITY-GATED** (2026-09-23 approved amendment; not a v1 blocker): CPU Kokoro retained and user-accepted; no compatible Linux NPU backend/model path established | If ever pursued: compatible native backend/model artifact and license/integrity review, Settings setup, offload attribution, signal/listening and latency evidence — all before any advertisement |

Stack validation is not a pass for either stage. No NPU installation or workload
change was authorized by the assessment or performed in it.

## Capability-gated execution (clarified R16, implemented)

[Capability-gated execution](foundation/voice-capability-gated-execution.md)
lands the clarified policy as production behavior: the pure-core stage policy
(`vesper_voice::execution`) with CPU / Automatic-verified-only / strict-NPU
resolution per stage, evidence-based readiness facts, in-process readiness
caching with invalidation, honest placement attribution, F9 gate resolution,
Settings → Voice compute menus, and scope persistence. The historical empty
registry described by that record is superseded in `voice-flm` builds by exactly
one verified-only FLM NPU STT route; builds without that feature and the TTS
route registry remain honestly empty. Production-path regressions prove the
no-NPU matrix: CPU policy makes zero accelerator calls,
Automatic-on-no-hardware is an ordinary CPU outcome, strict refusals name the
exact stage blocker, stages resolve independently, copied strict preferences are
revalidated locally and never silently rewritten, and placement metadata never
fabricates full-NPU success. No-NPU machines keep working CPU voice with no
installation and no failure framing. R16a is implemented with documented
correlation-level limits; R16b remains optional/capability-gated and conformant
on CPU.

## Latest native latency acceptance

*(Historical, correctly scoped: the 20/40/20-second observations below
describe the superseded pre-repair build, not the current one. The
latency lineage continued through the latency repair, the
latency-performance candidate ("ok its way faster then before"), the
continuity repairs, and the 2026-09-23 NPU-conversation acceptance
("no gaps … sounds natural" — perceived continuity, no numeric claim).
The current status lives in the final completion audit and
`foundation/voice-npu-user-acceptance.md`.)*

The [initial user retest](foundation/voice-user-latency-acceptance.md) recorded
approximately 20 s, 40 s and 20 s to first spoken output over three attempts and
approximately 10–15 s for Preview. Recording was described as fast, approximately
2–3 s in the first attempt. These approximate human observations are not a stage
breakdown, but they fail the existing requirement that the approximately
20-second delay is unacceptable. The code-level early-PCM fixture remains useful
regression evidence and does not override native user acceptance.

The [latency/performance candidate repair](foundation/voice-latency-performance-repair.md)
now prepares one reusable worker while the Natural Voice pack menu is visible and
splits ordinary approved units at a smaller first word boundary. The fixed long-unit
fixture reached first PCM in 1.62–2.59 s across observed post-repair runs (machine
load varied), and release-mode ready-worker Preview attempts reached fixture PCM in
1.559/1.569 s. Alex subsequently ran the release candidate and reported **“ok
its way faster then before,”** providing positive comparative device evidence.
No repeat seconds or error-path matrix was supplied in that historical latency
session. Later records close the approved NPU and R6 scopes. VRO-17 remains OPEN
only on PR-5; no numeric latency threshold is invented.
