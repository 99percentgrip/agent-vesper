# VRO-17 NPU STT implementation progress — shared Silero VAD worker

Status: **BLOCKED / NOT READY — authorized FLM asset setup succeeded, but installed FLM 1.0.5 failed the bounded loopback inference gate; adapter registration is prohibited**

## Objective

Continue the approved R16 implementation with a persistent, backend-neutral local
Silero VAD owner, then compose FLM 1.0.5 Whisper NPU STT without regressing the
accepted CPU STT/Kokoro TTS path or claiming NPU readiness before real evidence.

## Methods and commands

Repository and baseline were re-established before editing:

```text
repository: /home/Alex/Projects/agent-vesper
HEAD: 8f258ba28f4ea2f749526fb32b5b180ecc7dcead
accepted candidate sha256:
9d59189a00999fffc5bde5994c49ab7061ecda380c7d7f1e4698522052b54271
```

The applicable root, apps/TUI/tests, crates/voice, docs, and foundation DOX
contracts were read. Source owners inspected included `VoiceStt`,
`SharedSidecarStt`, `voice_accel`, the F9 gate, and the installed
faster-whisper VAD implementation. Two requested delegated investigations were
attempted, but both were unavailable because the configured worker model was not
in the current OpenAI account model list; no delegated result is treated as
evidence.

Commands executed:

```sh
git rev-parse --show-toplevel
git rev-parse HEAD
git status --short
sha256sum target/voice-candidates/agent-vesper-tui-continuity-phoneme-repair
grep faster_whisper/vad.py for installed symbols
~/.local/share/agent-vesper/voice-venv/bin/python -m py_compile \
  apps/agent-vesper-tui/src/voice_vad.py
# one-second canonical-silence request through the persistent worker protocol
FLM_DISABLE_UPDATE_CHECK=1 flm pull whisper-v3:turbo
FLM_DISABLE_UPDATE_CHECK=1 flm check whisper-v3:turbo
flm serve whisper-v3:turbo --asr 1 --host 127.0.0.1 --port 18080 --cors 0 --quiet
curl --max-time 60 -F model=whisper-v3 -F file=@silence.wav \
  http://127.0.0.1:18080/v1/audio/transcriptions
AGENT_VESPER_VOICE_PYTHON=~/.local/share/agent-vesper/voice-venv/bin/python \
  python3 apps/agent-vesper-tui/tests/voice_vad_worker.py
```

## Files

Created:

- `apps/agent-vesper-tui/src/voice_vad.py` — persistent line-delimited JSON VAD
  worker using the already-installed faster-whisper Silero implementation.
- `apps/agent-vesper-tui/tests/voice_vad_worker.py` — controlled, device-free
  persistence/silence/output/shutdown/error contract regression.
- `docs/foundation/voice-npu-stt-implementation-progress.md` — this truthful
  progress record.

Updated:

- `docs/foundation/evidence-index.md` — progress-record link.

No Rust production route, Settings behavior, configuration, model asset, or
accepted candidate was changed.

## Exact evidence

Installed API inspection found:

```text
faster_whisper/vad.py:15:class VadOptions
faster_whisper/vad.py:45:def get_speech_timestamps
faster_whisper/vad.py:186:def collect_chunks
faster_whisper/vad.py:289:def get_vad_model
```

Syntax verification exited successfully. The controlled protocol regression
returned:

```text
PASS: persistent VAD protocol, silence provenance, canonical output, redacted failure
```

The bounded real installed-worker silence probe returned verbatim:

```json
{"ready":true,"protocol":1}
{"speech":false,"input_samples":16000,"output_samples":0,"id":1}
{"shutdown":true}
```

The probe used generated one-second zero PCM, no microphone, no provider, no
FLM server, no NPU workload, no download, and no settings/user-state write. It
also verified that no speech-output WAV was published for confirmed silence.

## Implemented contract

- Canonical input is restricted to uncompressed mono i16, 16 kHz WAV.
- Input is bounded to 64 MiB.
- The installed Silero model is initialized before readiness and retained by one
  process for later requests.
- Default installed `VadOptions` are used; no unsupported threshold tuning was
  invented.
- Speech output is atomically published as canonical WAV; silence is a distinct
  result and publishes no output file.
- Errors fail closed with metadata-only text and do not reflect paths, audio, or
  exception details.

## Authorized asset setup and real runtime gate

Alex explicitly authorized the itemized FLM Whisper download and bounded NPU
verification through the native interview. The installed CLI reported a 622.91
MB transfer (620.06 MB model plus metadata) and completed all four vendor hash
checks. The resulting files total 653,169,645 bytes; the principal model is:

```text
650175128 bytes  model.q4nx
sha256 8fb97604bf5762ee26efa696cfc9eb70724110358c7b4ca628a7973bf8a16291
```

`flm check whisper-v3:turbo` reported every file present and compatible. This
matches the reconnaissance pin. Existing `qwen3:1.7b` assets were preserved.

The real installed-runtime gate then failed. `flm serve whisper-v3:turbo --asr
1 --host 127.0.0.1 --port 18080 --cors 0 --quiet` emitted:

```text
[ERROR] Unsupported model family or non-llm: "whisper-v3"
```

The process nevertheless listened on loopback. A canonical one-second silence
multipart request to `/v1/audio/transcriptions` (`model=whisper-v3`, `file` WAV)
returned zero bytes and exceeded the bounded 60-second deadline (`curl` exit 28,
HTTP 000). A second documented-form launch (`flm serve --asr 1 ...`) conflicted
with that listener while appearing to begin an additional 1237.7 MB transfer;
it was not allowed to replace or disturb the first owned process. All owned FLM
server processes were terminated and observed; no `flm serve` remains.

This is a concrete installed-version/runtime incompatibility, not an adapter
implementation omission that may be papered over. R16 forbids registering or
offering this route as Ready. No production Rust route was therefore added.

## Deviations and unresolved items

This turn did **not** complete the requested implementation. In particular:

- no Rust owner/supervisor yet starts, bounds, cancels, or reaps this worker;
- no FLM multipart adapter or loopback-only endpoint validation exists;
- no owned FLM lifecycle/readiness/lost-service recovery exists;
- no late-result suppression or cancellation composition exists;
- the model setup was explicitly confirmed and its retained bytes/hash verified,
  but no productized Settings setup/progress/repair UI exists;
- no `voice_accel` STT route is registered, so Automatic/strict NPU behavior is
  unchanged and honest;
- no F9 selection wiring, save/reload test, production-loop test, long-dictation
  test, or NPU placement receipt exists;
- the approved asset is now installed in FLM's existing user cache, but the real
  server/inference gate failed and the partial second launch exposed an additional
  temporary-download cost not represented by the catalog estimate;
- Kokoro NPU TTS remains independently unproven and unchanged;
- the requested full verification matrix was not run.

The new worker is not wired into production and therefore creates no partial or
misleading capability. It must not be described as NPU STT implementation.

## Readiness effect

The reviewed shared-VAD architecture now has a concrete, locally probed worker
protocol and deterministic regression. The authorized FLM model is installed
and hash-verified, but the real FLM 1.0.5 server/inference gate failed before an
adapter could truthfully be registered. Overall VRO-17 NPU STT remains **NOT
READY / BLOCKED ON THE INSTALLED FLM RUNTIME**. CPU STT, CPU Kokoro, R16 policy
semantics, D24 continuity/list behavior, and the accepted candidate remain
untouched. PR-5 and release work were not started.

---

# Launch-gate correction (2026-09-22)

Status after the corrected experiment: **BACKEND EVIDENCE ESTABLISHED — FLM
1.0.5 standalone ASR serves real NPU transcription on this machine. Production
integration is NOT resumed: no adapter, route registration, Settings setup
action, F9 wiring, or candidate exists.**

## What the first two launches actually proved and did not prove

The two receipts above remain real failures, but their causes were launch
confounders, not an installed-runtime limitation:

1. `flm serve whisper-v3:turbo --asr 1 …` placed the speech model tag in the
   positional chat-model slot. Source-verified behavior (pinned revision
   `0cfecb0881de95a5d48e2bdcf02e9b38d04e1ecf`, revalidated locally in
   `/tmp/FastFlowLM-recon`): `src/include/AutoModel/all_models.hpp` maps family
   `whisper-v3` to the non-LLM error case, prints `Unsupported model family or
   non-llm`, and substitutes a `Llama3` engine with replacement tag
   `llama3.2:1b`. The error therefore describes the chat-model fallback, not
   ASR capability. ASR loads separately and independently in the
   `RestHandler` constructor (`src/server/rest_handler.cpp:340-345`).
2. `RestHandler::ensure_model_loaded` (same file) then invokes the downloader
   for the missing replacement chat model. This is the source-verified origin
   of the previously unexplained ~1237.7 MB transfer: catalog `llama3.2:1b`
   totals ~1.2 GB (model.q4nx 744,607,744 bytes plus tokenizer/config). Local
   residue confirms the attribution — a partial `Llama-3.2-1B-NPU2` directory
   holding only `config.json` and `model.q4nx` (744,608,763 bytes, mtime
   2026-09-22 10:35–10:39) sits in the FLM model root, and `flm check
   llama3.2:1b` reports it as not found / incomplete (tokenizer files never
   arrived). The residue is left in place; no deletion was authorized. The
   corrected launch makes this path unreachable (see below).
3. The second attempt, `flm serve --asr 1 …` (the correct standalone form),
   conflicted with the still-running first listener on the target port and was
   aborted; its observed transfer was the same fallback resuming, not a second
   model family.

## The corrected standalone launch (verified installed behavior)

`flm serve` with no positional tag makes `main.cpp:537-539` set the tag to the
internal `model-faker` sentinel, and `rest_handler.cpp:359` then skips
chat-model loading entirely. `ensure_asr_model_loaded("whisper-v3:turbo")`
(`rest_handler.cpp:425-449`) finds the installed model `Ready` and loads it
without any download path. Verified launch:

```sh
cd /tmp/flm-asr-probe
FLM_DISABLE_UPDATE_CHECK=1 /usr/bin/flm serve --asr 1 \
  --host 127.0.0.1 --port 18081 --cors 0 --quiet
```

Installed-binary identity: `/usr/bin/flm` → `/opt/fastflowlm/bin/flm`
(symlink), ELF, 12,346,792 bytes, `flm version --json` reports
`{"version":"1.0.5"}`. The binary contains compiled `Whisper::load_audio`,
`/v1/audio/transcriptions`, `libwhisper_npu.so`, and the ASR memory-reserve
message; Whisper xclbins ship under
`/opt/fastflowlm/share/flm/xclbins/Whisper-V3-Turbo-NPU2/` (5 files). The
installed build therefore includes the ASR branch matching the audited source.
Model root resolved to `~/.config/flm/models` (`FLM_MODEL_PATH` unset).

No-download proof during the probe: zero files created or modified under
`~/.config/flm/models`; zero outbound sockets for the flm PID beyond its
loopback listener; the startup log names the installed model path directly.

## Bounded loopback verification results

One probe-owned process (pid 935428, verified sole listener on
127.0.0.1:18081, timeout-bounded 180 s, stdout+stderr drained to a bounded
log, full exit and port release observed afterward):

| Case | Route | Result |
|---|---|---|
| Catalog | `GET /v1/models` | HTTP 200 (listener liveness only, not ASR readiness) |
| Digital silence 1.0 s | composed: production `voice_vad.py` first | `{"speech":false,"input_samples":16000,"output_samples":0}`; 0 FLM requests dispatched |
| Speech surrogate, VAD-filtered (1.584 s retained of 4.0 s) | `POST /v1/audio/transcriptions`, `model=whisper-v3`, `file=<WAV>` | HTTP 200 in 2.348 s, non-empty text, server log `NPU Locked` → `Transforming audio to text` → `NPU Lock Released` |
| Warm repeat | same | HTTP 200 in 2.306 s, identical shape |
| Direct-to-FLM silence (diagnostic; bypasses production VAD) | `POST` with raw 1 s digital zeros | HTTP 200, non-empty hallucinated text |

The last row is decisive for the architecture: the backend has no silence
contract of its own. Unfiltered silence produces confident hallucinated text on
this exact endpoint, so the local Silero preprocessor is load-bearing, not
decorative. The composed silence short-circuit plus the backend hallucination
re-prove the R16 decision that the route may be registered only as
VAD-protected.

Fixtures were synthetic (envelope-modulated tonal surrogates and digital
zeros): no microphone, speaker, recorded person, or live provider was used.
Natural-speech recognition quality is not claimed or measured here.

## VAD worker speech-positive path: defect found and fixed

The earlier turn proved only the silence path. The speech-positive path was
broken: installed faster-whisper 1.2.1's `collect_chunks` returns
`(audio_tuple, segments)` where `audio_tuple` is a tuple of per-segment
arrays; the worker's direct `np.clip(speech, …)` failed with an
inhomogeneous-shape error, surfaced as the worker's fail-closed
`{"error":"VAD failed"}`. Fixed in `voice_vad.py` by concatenating the
returned part arrays before quantization. Post-fix receipts:

```text
silence: {'speech': False, 'input_samples': 16000, 'output_samples': 0, 'id': 1}
speech:  {'speech': True, 'input_samples': 64000, 'output_samples': 25344, 'id': 2}
filtered == source[0:25344] after 32768->32767 scale round-trip: max error 0 LSB
trailing silence removed: 4.000 s input retained as 1.584 s
PASS: persistent VAD protocol, silence provenance, canonical output, redacted failure
```

The detected window covers the fixture's first two voiced segments plus the
intra-window gap; the genuine trailing 2.4 s tail is removed. Content is
preserved exactly (0-LSB fidelity check); the probe's original strict
frame-subsequence assertion was corrected because the float32→int16
round-trip legitimately changes values by at most 1 LSB, which is neither
reordering nor corruption. With default `VadOptions` the surrogate's third
segment was not separately detected — an expected property of a synthetic
tonal surrogate, not a regression; natural-speech detection quality remains
Alex's acceptance item.

## Execution placement evidence and its limits

Correlated per-request evidence that the installed model executed on the AMD
NPU through this process: `/dev/accel/accel0` open as an fd of the flm
process; `libwhisper_npu.so` and `libxrt_driver_xdna.so.2.26.0` mapped in its
`/proc/<pid>/maps`; startup log naming the installed Whisper model path;
per-request `NPU Locked` / `NPU Lock Released` pairs; one owned process on the
verified port. The endpoint returns no offload or placement field and no
per-request trace exists, so this is process-device-model correlation —
sufficient to establish the backend gate, not a per-request offload receipt
suitable for a full-NPU label. Stage-wise labeling (CPU VAD preprocessing +
NPU recognition; CPU Kokoro unchanged) remains mandatory under R16.

## Corrected status line

ASR model installed and hash-verified; shared Silero VAD worker component
tested on both silence and speech-positive paths (one defect found and fixed);
FLM 1.0.5 standalone ASR backend gate PASSES on loopback with real NPU
execution; production wiring remains unimplemented (no adapter, lifecycle
owner, Settings setup action, route registration, F9 selection, or acceptance
candidate); the prior failures are now attributed to model-slot misuse and a
port-conflict confounder, with the fallback chat-model download as the
source-verified origin of the residual ~745 MB partial `Llama-3.2-1B-NPU2`
directory.

Next implementable increment (scope unchanged from the prior unit):
`VoiceStt`-boundary FLM adapter plus owned loopback supervisor, `voice_accel`
route registration behind real verification, native Settings setup/verify, F9
composition, and red-first production-path tests. No download, runtime, or
device change is required to start it.

---

# Production NPU speech recognition implemented (2026-09-22, later)

Status: **PRODUCTION ROUTE IMPLEMENTED AND FIXTURE-VERIFIED IN THE
OPTIMIZED CANDIDATE. Natural-speech accuracy, live microphone, and the
full PTY F9 round-trip remain open acceptance items.**

## What now exists (all behind the default-off `voice-flm` feature)

| Component | Owner | Evidence |
|---|---|---|
| Owned FLM ASR process supervisor | `apps/agent-vesper-tui/src/voice_flm.rs` (`FlmProcess`: verified standalone launch `serve --asr 1 --host 127.0.0.1 --port <owned> --cors 0 --quiet`, **no positional chat model**; constructed env — no proxy/FLM_MODEL_PATH inheritance except the explicit read-only root; owned process group; continuous bounded log drain with an allowlisted classifier that **stops the owned service on any chat-model/download evidence**; bounded spawn retry over the pre-bind/release port race; exclusive teardown of the owned group only) | unit tests + receipt |
| Local Silero VAD composition owner | `apps/agent-vesper-tui/src/voice_flm_vad.rs` (one lazily-started persistent child per process over the fixed `voice_vad.py`; readiness-gated; `PYTHONPATH` stripped so a fixture/stub cannot shadow `faster_whisper`; stale per-PID dirs reclaimed opportunistically) | unit tests + real-model receipt |
| Final-only `VoiceStt` adapter | `voice_flm.rs::FlmNpuStt` (`descriptor`: on-device, `partials: None`, `vad_enabled: false` — truthful that the engine itself has no VAD; silence = the preprocessor's explicit zero-speech decision with **zero ASR requests and no FLM spawn**; positive-VAD + empty backend text = `Inference` error, never silence; R20 4 MiB capture cap enforced before any work; cancellation before/after VAD settles locally; a cancelled in-flight request **retires the owned service** so no later request queues behind an unstoppable NPU inference) | `voice_flm_route.rs` (11 tests) |
| Audited multipart transport | `voice_flm.rs::transcribe_request` (hand-rolled POST `/v1/audio/transcriptions`, exactly `model`+`file`, canonical WAV body, `Connection: close`, bounded read, redirects refused, no invented fields) | wire-shape test with a loopback double |
| Passive asset verification | `apps/agent-vesper-tui/src/voice_flm_assets.rs` (`PackState`: RuntimeMissing / AssetsMissing / SizeMismatch / DigestMismatch / Installed; hashes the 650 MB weight **only when its size already matches**; every non-installed state names a Settings remedy; documents the verified vendor quirk that `FLM_MODEL_PATH` is the PARENT of the pack root — passing the pack root itself breaks flm's catalog parse) | `voice_flm_assets.rs` (3 tests) |
| Route registration + verification gate | `voice_accel.rs`: `registered_routes()` returns exactly one STT route in `voice-flm` builds (never TTS); `flm_stt_readiness()` is the ordered chain device → runtime → pack → **in-process verification** (`record_flm_stt_verification`, set only by a successful real-model Verify; never persisted, re-derived per process) | `voice_flm_route.rs` gate tests |
| Native Settings surface | `settings_host.rs`: "Accelerated recognition (FLM NPU) · verify or review…" row (offered only when the runtime exists), a real **Verify** action that runs the production composition on a nonsensitive synthetic fixture with a truthful verified/not-verified dialog, and the existing per-stage compute menus (NPU required appears only when a route is registered) | in-app PTY receipt below |
| F9 selection wiring | `main.rs::SelectedStt::build` resolves the adapter from the **saved** scope through the shared rule: a verified accelerator route constructs `FlmNpuStt` (the CPU sidecar is **not** constructed on the NPU path); any lesser fact constructs the CPU sidecar. The F9 gate refuses strict-NPU scopes with the exact stage blocker. `ConversationHost`/`VoiceSession`/`ConversationController` gained `?Sized` STT bounds for the type-erased host in `voice-flm` builds | `voice_flm_route.rs` F9 gate tests |

## Real-model receipts (no device)

Release candidate (`--features voice-flm,voice-kokoro`), real installed
pack, synthetic fixtures (no microphone/speaker/provider):

```text
silence (1 s digital zeros): Ok(("", "VadConfirmedSilence")) in 0.284 s
speech surrogate (4.0 s → 1.584 s retained by VAD):
    Ok(("Ooooooooh. ×6 …", "InferredText")) in 3.683 s (cold, incl. model load)
warm repeat through the same owned service: ok in 3.214 s
PASS: composed FLM NPU recognition receipt (synthetic fixtures, no device)
```

The speech transcript is the tonal surrogate's expected hallucinated
vowels — it proves request/response execution and provenance, **not**
word accuracy (an open acceptance item).

An in-app native **Settings → Voice → Accelerated recognition → Verify**
receipt was also captured through the release binary's real UI: the
dialog rendered **"Accelerated recognition · verified — Verified: the
accelerated recognizer answered a local check through the full
composition (CPU speech detection + local NPU recognition)"**, with the
live process tree showing the owned `flm serve` (port 18130) answering
the multipart request (`NPU Locked` → `Transforming audio to text` →
`NPU Lock Released` in its drained log).

Two real composition defects were found and fixed during this work:

1. **The Verify fixture was undetectable by the installed Silero
   defaults** (a frequency-sweeping chirp). Replaced with the recorded
   gate's proven three-fixed-segment shape.
2. **`PYTHONPATH` shadowing killed the VAD worker**: a fixture
   `faster_whisper` stub on `PYTHONPATH` (which test harnesses set)
   imported instead of the real package, so the worker died on import
   and the adapter surfaced a deadline error. The worker now strips
   `PYTHONPATH` (it never needs one).

## Test and gate receipts

```text
cargo test -p agent-vesper-tui --features voice-flm,voice-kokoro \
  --test voice_flm_route --test voice_flm_assets --test voice_execution_policy \
  --test pr4_f9_gate --test pr4_wiring        → 40 passed, 0 failed
cargo test … --test voice_policy_parity --test voice_interruption_lifecycle \
  --test r3_speech_worker --test voice_multiturn_playback -- --test-threads=1
                                              → 29 passed, 0 failed
cargo test -p vesper-voice                    → 5 suites ok
voice_vad_worker.py (real venv)               → PASS
cargo clippy --features voice-flm,voice-kokoro --all-targets → 0 warnings
cargo fmt -p agent-vesper-tui -p vesper-voice  → clean
cargo xtask architecture                       → 30 packages validated
cargo xtask naming-guard                       → clean (36 frozen)
cargo check default / voice-conversation / voice-kokoro → all clean
```

Feature matrix proven: the default build compiles untouched (the
registry stays empty — no phantom option), `voice-conversation` and
`voice-kokoro` builds are unchanged, and CPU policy remains
structurally acceleration-blind with the route registered.

## Candidate

```text
target/voice-candidates/agent-vesper-tui-flm-npu-stt-candidate
sha256 8264033f19dfbc50229165f82c65f2fd496df0464a0993f668f6e6263dda14e7
features: voice-flm, voice-kokoro (release)
accepted CPU candidate preserved: 9d59189a…54271 (byte-identical)
```

## Native activation sequence (for Alex)

1. Run the candidate binary in a workspace.
2. Settings (`s` at the landing menu) → Voice → **Accelerated recognition
   (FLM NPU) · verify or review…** → **Verify accelerated recognition** →
   expect the verified dialog (~4 s; loads the model once).
3. Back on the Voice menu → **Speech recognition compute** → **NPU
   required** → Esc, Esc → **Save changes**.
4. F9 captures through the composed route (CPU Silero VAD → owned
   loopback FLM ASR → one final transcript → one ordinary agent turn).
   Verification is per-process: a restarted app re-runs Verify once.

## Production F9 routing repair (2026-09-22, second session)

The previous open item ("PTY F9 round-trip not receipted") is closed, and
the attempt exposed a **real production defect** that the four
independent-but-indirect F9 proofs could not catch: F9's stop path drove
the shared recorder into `voice.rs`'s **CPU sidecar** transcription
(`Control::Stop → stop() + transcribe()`), while the FLM adapter
constructed by `SelectedStt::build()` was never given audio. The gate
admitted the NPU route, but recognition ran on CPU — exactly the R16
violation the poison module detects.

### Defect and repair

- `voice.rs`'s worker gained a conversation-STT slot
  (`Arc<Mutex<Option<Arc<dyn VoiceStt>>>>`, shared with the Controller).
  The F9 gate installs the *selected* adapter into it before capture
  starts; a conversation-origin `Control::Stop` then transcribes through
  that adapter (`transcribe_through`) — reading the capture WAV once,
  driving the adapter future with the worker's cancel flag bridged in,
  and emitting the final through the same channel `drain_voice` routes
  by origin. The CPU sidecar is never spawned or consulted on that
  path; the CPU scope leaves the slot empty (F5/F9-CPU unchanged).
- One adapter instance backs both the worker seam and the conversation
  host (one warm VAD child + one owned FLM service per process).

### Test repairs (fixture defects, not production)

- Recorder fixture: `to_bytes(2, "little")` crashed on the first
  negative sample (unsigned default) — the surrogate died mid-capture.
  Fixed with `signed=True`.
- Phase-2 restructure: the harness relaunched a **fresh process** after
  Save, but FLM verification is per-process by design, so the fresh
  process correctly refused NPU. The F9 phase now runs in the same
  process that performed Verify.
- Provider double returned a single JSON body; the production client
  parses SSE deltas, so the reply never rendered or synthesized.
  Rewritten as word-sized SSE deltas (same shape as `r3_loop_pty.py`).

### Receipt (three consecutive passes)

```text
PASS: Settings save -> Verify -> F9 -> FLM NPU adapter -> one agent turn;
      provider requests=2; player PCM bytes=[92800]; CPU recognizer untouched
PASS: … player PCM bytes=[92800]
PASS: … player PCM bytes=[92800, 58400]   (two F9 turns in one process)
```

The on-screen transcript shows `you (voice): Ooooooooh. …` (the FLM
backend's own reading of the tonal surrogate), exactly one agent turn,
and Kokoro PCM at the no-device sink. The CPU recognizer module is
poisoned in the fixture; nothing on the FLM route imported it.

### Gates re-run after the repair

```text
cargo test -p agent-vesper-tui --features voice-flm,voice-kokoro --lib   → 281 passed
cargo test -p agent-vesper-tui --features voice-flm,voice-kokoro --tests → 157/6/10/6/10 passed
cargo test -p agent-vesper-tui --test voice_flm_route --test voice_flm_assets → 3 + 11 passed
cargo test -p vesper-voice                                                → 5 suites ok (86/13/13/29)
cargo check -p agent-vesper-tui (default, no voice features)              → clean
cargo clippy --features voice-flm,voice-kokoro --all-targets              → 0 warnings
cargo fmt -p agent-vesper-tui                                             → clean
cargo xtask naming-guard                                                  → clean (36 frozen)
voice_vad_worker.py (real venv)                                           → PASS
flm_stt_receipt --i-have-the-installed-model                              → PASS
  (silence: VadConfirmedSilence 0.35 s; speech: InferredText 3.57 s;
   warm repeat 2.62 s)
```

### Candidate (rebuilt after the repair)

```text
target/voice-candidates/agent-vesper-tui-flm-npu-stt-candidate
sha256 3c634c739e773d2c975f9eb61f9b39f1b845077185d386edcd0d489f13e1dfc8
features: voice-flm, voice-kokoro (release)
accepted CPU candidate preserved: 9d59189a…54271 (byte-identical)
```

### Operational note

The PTY harness tears the app down with SIGKILL, so the owned `flm
serve` children of *aborted* runs survive their parent (Drop cannot run
under SIGKILL). Three such stragglers accumulated across failed probe
iterations and caused a later Verify timeout ("owned ASR read failed or
timed out") by contending for the NPU. They were identified by exact
owned launch shape and terminated; Alex's own live TUI sessions and any
foreign service were untouched. Clean runs (test PASS) leave no
stragglers; only hard-killed iterations can leak, and the leak is
bounded to probe-owned processes.

## Honest open items

- **Natural-speech accuracy unmeasured** (tonal surrogates only).
- **Alex's live microphone/speaker acceptance open** (device tests are
  user-operated by directive).
- **`flm list`'s ~745 MB partial `Llama-3.2-1B-NPU2` residue** remains
  untouched in the user cache (not authorized for removal; never used).
- **Placement evidence level**: process/device/model correlation
  (`/dev/accel/accel0` fd + `libwhisper_npu.so` maps + model path +
  per-request NPU-lock log pairs). No per-request offload field exists
  on this endpoint; no "full-NPU" claim is made. CPU VAD + NPU
  recognition + CPU Kokoro are labeled separately everywhere.
- Unrun: full workspace suite, MSRV, CI, five-target matrix.
- **Natural-speech accuracy unmeasured** (tonal surrogates only).
- **Alex's live microphone/speaker acceptance open** (device tests are
  user-operated by directive).
- **`flm list`'s ~745 MB partial `Llama-3.2-1B-NPU2` residue** remains
  untouched in the user cache (not authorized for removal; never used).
- **Placement evidence level**: process/device/model correlation
  (`/dev/accel/accel0` fd + `libwhisper_npu.so` maps + model path +
  per-request NPU-lock log pairs). No per-request offload field exists
  on this endpoint; no "full-NPU" claim is made. CPU VAD + NPU
  recognition + CPU Kokoro are labeled separately everywhere.
- Unrun: full workspace suite, MSRV, CI, five-target matrix.

## Verify read-failure repair (2026-09-23, third session)

Alex's live Settings test failed Verify with `owned ASR read failed or timed
out`. Full diagnosis and repair live in `voice-verify-read-failure-repair.md`
(evidence-indexed). Summary for this owning record: reproduced verbatim
through the native Settings route; the vendor's own server log proved the
mechanism — three prior-session orphaned owned-shape FLM servers exhausted
the NPU's device contexts, so a fresh server's model load failed
`DRM_IOCTL_AMDXDNA_CREATE_HWCTX (EINVAL)` and the child died mid-request;
the collapsed read-error label then misreported the immediate reset as a
timeout (0.879 s; no deadline was ever involved). Owner-only repairs in
`voice_flm.rs`, both red→green: `io::ErrorKind` classification (`process
exited while answering` / `read timed out`; no deadline change) and a
pure-std durable child registry (`~/.local/share/agent-vesper/
flm-child-registry/`) with a registrar-host-liveness reaper (dead-host
orphan reaped on next spawn; live-host child proven untouched; owned shape
re-confirmed from `/proc` before any signal; no unsafe). Candidate
`agent-vesper-tui-verify-read-repair` sha256 `b289a6c1…`; native Verify +
F9 PTY re-receipted PASS against those exact bytes. The installed
`~/.local/share/agent-vesper/agent-vesper-tui` still predates FLM and was
not replaced (Alex's installation is his to update).
