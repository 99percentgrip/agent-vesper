# External Reconnaissance: Real-Time Bidirectional Voice Interface (voice oracle)

Read-only reconnaissance into a cloned external repository, referred to
exclusively as the **voice oracle**. The alias was assigned by Alex in the
mission directive ("avoid the original name; call it the Voice Oracle"),
following the alias-request contract of this directory. The upstream's
product name, repository name, owner names, and its agent-runtime brand
must never appear in any Vesper PRD, doc, source file, or commit message
produced by this work. Where an upstream-internal identifier embeds the
brand (script filenames, plist labels, environment variable names, header
names, the default conversation name, the stock wake-word model name),
this report paraphrases it instead of quoting it.

**Mission deviation, authorized in the directive.** This directory's default
contract limits analysis to documentation and configuration. The voice
oracle mission explicitly ordered a full architecture analysis of the cloned
implementation (STT pipeline, TTS pipeline, streaming, sessions, barge-in,
audio lifecycle, concurrency, errors, configuration, integration boundary),
so production source **was** read, line-cited, and analyzed — MIT-licensed,
pinned below, authorized as trusted data by the directive. No Vesper
production file was modified by the reconnaissance itself; this report and
its index entries are the only workspace writes (see
`docs/foundation/voice-oracle-recon-execution.md`).

## Evidence pinning

| Source | Commit | License | Constraint on reuse |
|---|---|---|---|
| voice oracle (LAN voice + HUD server, PTT client, GPU STT worker, agent plugin) | `88998de8369e9d36f6d434b5e01feb93fcf1c33f` (HEAD, 2026-06-13) | **MIT** (holder recorded in the pinned commit's `LICENSE`) | Concepts and protocols mappable; no source-code port. The clone lives outside this workspace at a sibling project directory supplied by Alex; the directory name itself carries the upstream brand and is intentionally not reproduced here. |

Component inventory actually present at the pinned commit (all line counts
measured):

| Path (brand-free) | Lines | Role |
|---|---|---|
| `server/server.py` | 1,241 | The entire voice pipeline: WebSocket protocol, STT fan-in/failover, agent SSE client, sentence-gated TTS, turn lifecycle, barge-in, HUD HTTP surface |
| `server/hud/index.html` | 885 | Single-file browser client: 16 kHz worklet capture, PCM playback with odd-byte carry, barge-in UX |
| `client/client.py` | 378 | Desktop push-to-talk / wake-word client (sounddevice + openWakeWord) |
| `worker/stt_server.py` | 76 | Optional GPU STT sidecar (faster-whisper on CUDA) |
| `worker/worker_stats.py` | 70 | Optional machine-stats agent (not voice) |
| the agent-plugin directory (4 files) | ~140 | Agent-side tool plugin driving HUD media panels |
| `docs/ARCHITECTURE.md` | 112 | Author's own protocol/latency/gotcha record |
| `server/config/server.example.yaml` | 77 | Full configuration surface |

Citations below are repository-relative paths with `file:line` anchors into
the pinned commit.

---

## 1. System architecture (as built)

Three cooperating processes plus the agent runtime:

```
 Browser HUD / desktop PTT client          Voice pipeline server (server.py)
 ── ws(s): binary PCM up, JSON+PCM down ──►  one asyncio event loop:
                                             STT failover → agent SSE →
                                             sentence gate → streaming TTS
                                                        │ HTTP (loopback, bearer)
                                                        ▼
                                              the agent runtime's Sessions API
                                             (named persistent sessions, run ids,
                                              tool events, approval events)
 [optional] GPU STT sidecar (worker/stt_server.py) ◄─ raw PCM POST, JSON text
```

Key structural facts:

- **The voice server is a translation shim, not a brain.** It owns audio,
  transcription failover, sentence gating, TTS streaming, and the client
  protocol; all reasoning, tools, memory, and approval policy live in the
  agent runtime behind an HTTP/SSE boundary (`server/server.py:174-294`,
  the thin agent-API client class at `server/server.py:174-294`). This is exactly the separation the Vesper extraction must
  preserve: a voice subsystem with no agent logic.
- **One brain, many faces.** Typed chat (HTTP `/api/chat`) and voice share
  one named agent session, so memory is continuous across input modalities
  (`docs/ARCHITECTURE.md` "How a voice turn flows" step 3;
  `server/server.py:731-769`).
- **Degradation is designed-in, first-class.** STT: remote GPU worker →
  local Whisper (`server/server.py:319-346`). Agent: sessions API → direct
  cloud chat fallback in "basic mode" (`server/server.py:365-410`). A
  missing optional stats dependency degrades a panel (`server/server.py:57-60`).
- **The agent can call back into the voice surface** through a bundled
  plugin that POSTs a panel-broadcast endpoint (the agent plugin's `tools.py`,
  `server/server.py:883-905`). The author's recorded lesson: prose in a
  persona file could not redirect the model away from built-in tools; an
  explicit tool schema won immediately (`docs/ARCHITECTURE.md` gotcha 11).

## 2. STT pipeline

Two interchangeable STT implementations behind one internal call
(`transcribe()` at `server/server.py:319-346`):

1. **Remote GPU worker (preferred when configured).** Raw int16 PCM POSTed
   as `application/octet-stream` with a shared-token header; JSON `{"text"}`
   response; ~6 s timeout; **any** failure returns `None`, which means
   "unavailable, use fallback" — never a user-visible error
   (`server/server.py:348-363`). The worker (`worker/stt_server.py:57-73`)
   loads `large-v3-turbo` on CUDA fp16, rejects <0.1 s of audio, trims odd
   trailing bytes, uses beam_size=1, and converts any transcription
   exception into an **empty transcript** rather than a 500.
2. **Local Whisper fallback.** A `RealtimeSTT` `AudioToTextRecorder`
   constructed once with `use_microphone=False`, CPU, int8, 16 kHz,
   `beam_size=1`, `faster_whisper_vad_filter=False`
   (`server/server.py:296-317`). Finalization feeds the accumulated float32
   samples and runs `perform_final_transcription` under an `asyncio.Lock`
   (one model, one GPU/CPU — STT is serialized), then clears the queue
   (`server/server.py:330-336`). Any exception — including
   faster-whisper's "No clip timestamps found" on near-silence — is treated
   as an **empty transcript**, not a failed turn (`server/server.py:337-342`).

**Live partials (interim transcripts).** While recording, every received
PCM chunk is buffered per connection; a background transcription of the
whole buffer is scheduled only when ≥1.2 s of *new* audio has arrived since
the last pass, the buffer holds ≥0.5 s and ≤30 s, and no partial pass is
already running; results are emitted as `partial_transcript` only if still
recording (`server/server.py:1119-1141`). Partials reuse the exact same
failover `transcribe()` path — there is no separate partial model.

**Formats.** Canonical audio is int16, 16 kHz, mono, little-endian PCM
(`server/server.py` docstring; client `SAMPLE_RATE=16000`,
`CHUNK_MS=80`). The browser captures at a 16 kHz `AudioContext` to avoid
resampling damage, with a box-filter downsample fallback worklet when the
browser refuses 16 kHz (`server/hud/index.html:546-568`).

**Notable Vesper-relevant finding — VAD.** The reference disables
faster-whisper's VAD on both STT paths (`faster_whisper_vad_filter=False`,
`server/server.py:311`; `vad_filter=False`, `worker/stt_server.py:66`) and
instead *bounds* partial input. Vesper already shipped the opposite lesson
with primary-source evidence: trailing silence hallucinates text and
`vad_filter=True` is the fix (`docs/voice-trailing-silence-vad-prd.md`,
probe receipt: 30 s of zeros → `"You"` unfiltered, `""` filtered). The
extraction must keep VAD on and treat the reference's silence-as-empty
exception handling as the second line of defense, not the first.

## 3. TTS pipeline

One cloud TTS provider (streaming Flash-class model), sentence-streamed:

- **Request shape.** POST `/v1/text-to-speech/{voice_id}/stream` with
  `output_format=pcm_16000`, fixed voice settings (stability 0.55,
  similarity 0.70, style 0.10, speaker boost), 120 s timeout, streamed
  4 KiB chunks (`server/server.py:437-466`). First audio byte is
  timestamped for latency telemetry.
- **Cancellation hygiene.** The response is closed in a `finally` because
  barge-in cancels mid-stream; leaked connections were an observed failure
  (`server/server.py:464-466`).
- **Sentence gating.** Assistant text deltas accumulate in a `pending`
  buffer; a regex `(.+?[.!?])(?=\s|$)` extracts complete sentences as they
  close, each is cleaned and dispatched to TTS immediately while the LLM
  keeps generating; the unmatched tail is flushed after the stream ends
  (`server/server.py:476-568`, `595-599`). This is the core
  latency-combining trick: LLM streaming and TTS streaming are pipelined
  at sentence granularity.
- **TTS hygiene pass** (`_clean_for_tts`, `server/server.py:604-616`):
  strip `<think>` blocks; replace secret-shaped strings (key/value
  patterns, token-like compounds, ≥36-char opaque blobs, PEM blocks) with
  ` redacted ` **before any text reaches the cloud**; replace code blocks
  with " code omitted. "; unwrap inline code, links, bullets, emphasis;
  collapse whitespace.
- **Client-side decode discipline.** TTS PCM frames arrive at arbitrary
  byte boundaries; both clients carry the odd byte across frames before
  decoding int16 pairs (`server/hud/index.html:528-531`; the desktop client
  accumulates the entire turn and plays once on `done`
  `client/client.py:117-152`).

**No local TTS exists in the reference.** Cloud-only, with a privacy filter
as the compensating control. A Vesper extraction that promises local/cloud
interchangeable TTS must add the local engine the reference never had —
and must not claim it before it exists (house rule: no invented providers).

## 4. Streaming behavior (end-to-end)

A voice turn, with the exact event chain (`server/server.py:476-568`,
`1055-1093`):

1. Client opens WebSocket, sends `start` (sample rate, format, channels,
   optional conversation), then binary PCM chunks.
2. Server: per-connection buffer grows; partial transcriptions emitted
   every ≥1.2 s of new audio (§2).
3. `stop` → `end_of_speech` timestamp; a **turn task** is spawned
   (`asyncio.create_task(_run_turn)`) so the socket loop stays free — this
   is what makes barge-in possible while a turn is in flight.
4. Final transcript (failover STT) → `transcript` event.
5. Agent turn dispatched over SSE. Parsed event classes
   (`server/server.py:253-291`): `run.started` → run id (surfaced as
   `run_started`, enabling stop); `assistant.delta` → text; `tool.started`
   → name+preview → `agent_status{tool_use}` (internal `_`-prefixed
   pseudo-tools filtered); `*approval*` → `approval_request`;
   `assistant.completed` → final content + `interrupted` flag;
   `run.completed` → usage accounting; `run.failed`/`error` → raise.
6. Text deltas → sentence gate → per-sentence TTS → binary PCM frames to
   the client. First spoken sentence flips `agent_status{speaking}`.
7. `done{timing}` closes the turn with a full latency summary.

**Sync/async bridging.** All blocking I/O (requests-based agent SSE, TTS)
runs on worker threads and is marshalled back through per-call
`asyncio.Queue`s with `loop.call_soon_threadsafe`
(`server/server.py:528-568`). The agent generator is consumed by a forward
task so the turn loop can cancel it on barge-in.

**Measured latency profile** (author's numbers, Apple Silicon, small.en
CPU): STT finalize ~0.7 s; agent first token 1–3 s; first audible audio
~3 s; total simple turn ~4 s; GPU worker cuts STT to ~0.2 s
(`docs/ARCHITECTURE.md` "Latency profile"). The two biggest levers: the
LLM, then Whisper model size.

## 5. Session management

- **Agent-side sessions are the memory.** Session ids are created via the
  agent API, persisted to a JSON state file keyed by conversation name,
  and reused across restarts (`server/server.py:196-217`). A 404 (stale
  id after a DB reset) triggers exactly one forced recreation
  (`server/server.py:412-431`, `731-769`).
- **Voice and typed chat share the session** (§1); the conversation name
  is selectable per `start` message and defaults from config.
- **Interruption context is fed back into the session.** On barge-in the
  server remembers the last sentence actually spoken and prefixes the next
  turn's input with a bracketed note ("your previous spoken reply was cut
  off by the user after you said: …") so the agent's memory matches what
  the user actually heard (`server/server.py:1071-1078`). This is a
  user-message-content mechanism, not a history rewrite — the right shape
  for Vesper's interruption contract.
- **Voice-server-side state is per-connection and ephemeral**
  (`ConnState`, `server/server.py:1042-1053`): audio buffer, recording
  flag, turn timing, turn task, current run id, spoken-sentence log,
  interrupt note, partial task, byte watermark. No durable voice state of
  its own beyond usage/latency logs.

## 6. Interruption / barge-in logic

The reference's most instructive subsystem. Two triggers, one handler:

- **Barge-in** = `start` received while a turn is active
  (`server/server.py:1160-1161`), or the client clicking the ring while
  speech plays (browser stops local playback first,
  `server/hud/index.html:605`). **Stop** = explicit `stop_run` message or
  the STOP button/Esc (`server/hud/index.html:459-462`).
- Both funnel into `_cancel_active_turn` (`server/server.py:1095-1117`):
  1. Capture the run id **before** cancelling (turn cleanup clears it).
  2. If anything was spoken, record the last spoken sentence as the
     interrupt note (§5).
  3. Cancel the local turn task and await it — cancellation propagates
     through the SSE consumer, the forward task, and the in-flight TTS
     request (`finally` blocks close streams).
  4. Then, and only if a turn was actually active, POST the agent's
     run-stop endpoint; a 404 is tolerated because on the pinned agent
     build session runs are not in the runs registry — dropping the SSE
     stream is what actually halts the turn (`server/server.py:1108-1116`;
     `docs/ARCHITECTURE.md` gotcha on the stop button).
- The desktop PTT client mirrors this locally: on interrupt it stops
  playback immediately, waits ≤2 s for turn completion, and auto-starts
  the next capture (`client/client.py:230-258`).
- `assistant.completed` carries an `interrupted` flag the agent sets; the
  server records it in turn metrics (`server/server.py:285-289`, `523`).

**Vesper mapping caution.** The reference can drop the agent stream
mid-tool-call and rely on the agent's own recovery. Vesper's binding
contract is stricter: automatic recovery only when no ambiguous tool-call
fragment exists, and never replay a possibly side-effecting tool call.
The voice subsystem must therefore route interruption through Vesper's
existing transactional cancellation, not reimplement a "drop the stream"
shortcut.

## 7. Audio lifecycle

- **Capture**: browser 16 kHz worklet emits 80 ms frames (1,280 samples)
  only while capturing (`server/hud/index.html:569-572`); desktop client
  uses an 80 ms sounddevice callback into a bounded queue
  (`client/client.py:96-103`). Echo cancellation/noise
  suppression/AGC requested from the browser (`index.html:578`).
- **Server buffer**: unbounded per-turn list of chunks, joined at `stop`;
  partial passes bounded to ≤30 s of audio (§2).
- **Playback (browser)**: `playChunk` decodes even-byte-aligned int16 →
  float32, schedules on a running playhead (`currentTime+0.06` floor),
  tracks active sources, and `stopPlayback` stops every source and resets
  the playhead for barge-in (`server/hud/index.html:526-542`).
- **Playback (desktop)**: accumulates the whole turn's PCM and plays once
  on `done`, explicitly to avoid gapped audio from many small frames
  (`client/client.py:117-152`) — a latency/quality tradeoff opposite to
  the browser's, and a documented decision either way.
- **Retention**: voice audio is never persisted server-side; only
  transcripts, metrics, and usage counters are logged.

## 8. Concurrency model

- One asyncio event loop; per-connection state machine; **one turn task
  per connection at a time**, cancellable (§6).
- Blocking work (STT model calls, agent SSE, TTS HTTP) bridged via
  `asyncio.to_thread` + thread-safe queues (§4).
- **STT is globally serialized** by an `asyncio.Lock` around the single
  local model (`server/server.py:299, 330-336`); partials and finals
  contend for the same lock.
- **Exactly-once warm initialization**, double-checked under a
  `threading.Lock`, because the startup hook fires once per listener and
  concurrent recorder inits crashed three of four lifespans
  (`server/server.py:630-645`, comment; `docs/ARCHITECTURE.md` gotcha 8).
  Warm runs in the background so listeners open immediately and a warm
  failure never takes a listener down (`server/server.py:649-669`).
- FD-exhaustion hardening: streaming responses closed in `finally`,
  launcher FD limit raised (`docs/ARCHITECTURE.md` gotcha 9) — evidence
  that long-lived streaming servers leak without RAII discipline. Rust's
  ownership makes this class of bug structurally hard; keep it that way
  with cancellation-safe streams.
- Process supervision gotchas (orphaned STT child inheriting the listen
  socket; stop-by-port-ownership) are launcher-level and do not transfer
  to an in-process Rust subsystem, but they argue for child processes
  being optional and confined.

## 9. Error handling

Pattern, consistently applied:

| Failure | Behavior | Evidence |
|---|---|---|
| Remote STT unreachable/error | Silent failover to local; `None` = unavailable | `server/server.py:348-363` |
| Local STT exception (incl. silence) | Empty transcript; turn continues (user told "No transcript detected.") | `server/server.py:337-342`, `1066-1068` |
| Agent API down at turn start | Spoken notice + fallback to direct cloud chat "basic mode" | `server/server.py:390-403` |
| Stale agent session (404) | Recreate session once, retry | `server/server.py:412-431` |
| Agent stream error mid-turn | Raise → `error` event to client; turn metrics record it | `server/server.py:286-288`, `1086-1091` |
| TTS HTTP failure | Raise → `error` event; connection closed in `finally` | `server/server.py:455-466` |
| Turn cancelled | Recorded as `turn cancelled (barge-in or stop)` in metrics | `server/server.py:1080-1083` |
| Quota endpoint slow | Serve cached usage, refresh in background, never block | `server/server.py:790-833` |
| Malformed client event | Explicit `error` event (`Unknown event type`, `stop` before `start`) | `server/server.py:1175-1181` |

Every turn — success, error, or cancellation — writes a latency/timing
record (`TurnTiming` with ~20 stamped fields, `server/server.py:118-166`,
`618-628`). Honest per-stage telemetry is a first-class feature, not an
afterthought.

**Gap found:** the config schema documents an acknowledgment feature
(`ack_after_seconds`, `ack_texts` — speak a short "On it." while the agent
works) that **no code path reads** (grep over `server/server.py`: zero
uses). Documented-but-unimplemented. Vesper must not inherit the
aspiration without the implementation.

## 10. Configuration

Single YAML file (`server/config/server.example.yaml`), secrets in an env
file, never in config. Surfaces: LLM provider/model/tokens; agent base
URL (loopback), conversation name, session key, timeout, fallback
provider, voice-behavior system instructions; STT model/device/compute/
sample-rate/partials/interval + optional remote worker; machines list;
usage pricing; server host/ports/TLS/dashboard-proxy; security token env
+ origin allowlist; TTS provider/model/voice/format; persona prompt for
the fallback path. Env loading is `os.environ.setdefault` — process env
wins over file (`server/server.py:134-146`).

Security model worth keeping: token-gated endpoints and browser
WebSockets (origin allowlist + cookie), native clients exempt (threat
model = malicious websites, not local processes); the agent API key never
reaches any browser; agent API on loopback; strict path allowlist on the
agent proxy (`server/server.py:671-728`); secret redaction before cloud
TTS (§3); LAN-only by design.

## 11. Integration boundary with the agent runtime

The boundary is narrow, explicit, and — for Vesper — the most portable
part:

- **Outbound (voice → agent):** create/reuse named session; stream one
  chat turn over SSE with the transcript as input; stop a run; post an
  approval decision (`server/server.py:174-294`). Header-injected session
  key for long-term memory scope (`server/server.py:184-193`).
- **Inbound (agent → voice):** the SSE event vocabulary of §4 (run id,
  text deltas, tool started with preview, approval requests, completion
  with `interrupted`, usage).
- **Callback path (agent → voice surface):** a bundled agent-side plugin
  registers two tools that POST the voice server's broadcast endpoint to
  show/dismiss HUD media panels (the agent plugin). The voice surface is
  thus *addressable by the agent* through ordinary tool calls, gated by
  the agent's own approval machinery.
- **Persona ownership stays with the agent.** Voice-shaped behavior rules
  (speak plainly, short sentences, never speak secrets) are injected as
  agent instructions (the agent persona block in the upstream config;
  its field name embeds the upstream agent runtime's brand, so it is
  paraphrased here), not implemented
  in the voice server. The setup guide says it outright: "The agent — not
  the voice server — should own its personality" (`docs/SETUP.md` §1).

## 12. Mechanism-to-Vesper mapping (and gaps)

| Voice oracle mechanism | Vesper primitive | Status |
|---|---|---|
| `transcribe()` STT failover (remote → local) | `VoiceStt` port + failover chain policy in a new `vesper-voice` core | Gap — to build (PRD) |
| Sentence-gated streaming TTS | Sentence gate + `VoiceTts` streaming port pipelined with `ProviderStreamEvent::ContentDelta` | Gap — to build |
| `ConnState` + turn task + `_cancel_active_turn` | `VoiceSession` actor + existing `RuntimeCancellation` / transactional interruption outcomes (`docs/adr/0007`) | Partial — runtime side exists |
| Interrupt-note prefix | Next-turn user-message content field on the ordinary turn path | Gap — to build; must not rewrite history |
| `run.started`/stop endpoint | Existing run/turn identity + cancellation in `vesper-runtime` | Exists |
| Tool/approval event forwarding | Existing `ProviderStreamEvent::ToolCallStarted`/permission gate + ACP approval flow | Exists; voice only forwards |
| TTS hygiene + secret redaction | Deterministic `TtsHygiene` pass with fixtures (secret canaries per foundation conventions) | Gap — to build |
| `TurnTiming` telemetry | `VoiceTurnReport` value type → existing observability surface | Partial |
| Plugin calling back into the voice surface | Vesper hosted-tool seam (precedent: `WebService` in `vesper-harness`) | Pattern exists |
| Usage tally + quota cache | Existing `/usage` provider-neutral port rules (never fabricate) | Exists |
| Wake-word client loop | Non-goal v1; recorded | — |
| Browser HUD, LAN server, TLS, dashboards | Non-goals (single-user local hosts) | — |
| Local TTS | Did not exist upstream | Must be added for local/cloud parity, license-gated |

## 13. What Vesper should take, adapt, and refuse

**Take:** the turn-shaped protocol (start/PCM/stop/stop_run/approval with
partial, final, phase, audio, done events); sentence-gated TTS pipelining;
STT failover with "unavailable ≠ error"; barge-in that captures
last-heard-sentence context and routes through real cancellation;
byte-boundary-safe PCM framing on both ends; per-stage latency telemetry
on every outcome; privacy redaction before cloud synthesis; agent-persona
ownership outside the voice layer.

**Adapt:** VAD policy (keep `vad_filter=True` per our own shipped
evidence); interruption semantics (no ambiguous-fragment replay — stricter
than the reference); provider interchangeability as real registered
adapters with catalog-owned metadata (house rule), not a YAML string.

**Refuse:** the LAN multi-client server topology, browser HUD, TLS/proxy
surface, dashboard iframe proxy, machines panel, cinematic boot — all
host-product concerns foreign to Vesper's two hosts; any direct
source-code port; cloud-only TTS as the end state; the documented-but-
unimplemented ack feature (unless actually built); Python sidecars as the
production STT engine (the existing TUI dictation sidecar remains a
compatibility path until the native engine lands).

## Verification

- Upstream citations re-checked against the pinned working tree at
  `88998de8369e9d36f6d434b5e01feb93fcf1c33f` during the mission; line
  anchors refer to that commit.
- Workspace paths cited in this report verified to exist:
  `docs/voice-trailing-silence-vad-prd.md`, `docs/adr/0007-session-actor-and-hierarchical-cancellation.md`,
  `crates/vesper-provider/src/stream.rs`, `crates/vesper-runtime/src/session.rs`,
  `crates/vesper-harness/src/web_service.rs`, `apps/agent-vesper-tui/src/voice.rs`,
  `apps/agent-vesper-tui/src/voice_transcribe.py`, `xtask/src/main.rs`.
- License recorded (MIT); upstream commit pinned; no external repository
  state persists inside the Vesper tree.
- No Vesper production code was changed by this reconnaissance.
