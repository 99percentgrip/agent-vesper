# First-speech latency repair and native NPU assessment

> **Follow-up:** [Latency and performance repair after native acceptance](voice-latency-performance-repair.md) reduces the first-piece target again and reuses the Settings Preview worker. The original measurements below remain historical evidence for the earlier implementation, not the current final policy or user acceptance.

## Objective and delivery status

Follow the [sentence-pipeline repair](voice-speech-pipeline-repair.md) with one authorized live-provider timing check, then optimize local first-sentence speech without changing the saved model/reasoning. Assess native NPU support separately for transcription and speech output, preserving CPU usability.

**Code implemented and locally fixture-verified; user latency acceptance failed/remains open:** long, already-hygiene-approved speech units are divided losslessly at word/clause boundaries and pipelined through one player stream. The fixed 188-byte benchmark reached first PCM in **2.733 s**, compared with **8.602 s** for whole-unit synthesis. Alex confirmed that one fixed-text real-player sample was **complete and natural**. Existing short preview units remain whole. Native running-turn voice status is now actually rendered, not merely stored in session state. These narrow results are not phase completion evidence.

Alex's subsequent native retest observed first spoken output after approximately **20 s, 40 s and 20 s** across three attempts, with Preview delayed approximately **10–15 s**. Recording was described as fast, approximately **2–3 s** in the first attempt. The [user latency acceptance record](voice-user-latency-acceptance.md) therefore marks the phase **NOT ACCEPTED / OPEN**.

**Open:** live end-to-end initial response latency is not eliminated. Native NPU STT and NPU TTS are requested, separate, **unimplemented acceptance gates**. This report completes only the CPU code repair and bounded NPU assessment work unit; it does not complete user latency acceptance, the voice phase, or the NPU feature. No CPU execution is described as NPU inference.

## User decisions and authorization

Verbatim browser answers:

```text
live_timing: Authorize one bounded live-provider timing check
latency_tradeoff: Keep model/reasoning unchanged; optimize local first-sentence speech only, find out how to better enhance ! BTW it should support native NPU!
long_speech: Yes, complete and natural
npu_scope: Both stages; keep CPU usable and track each NPU gate separately
```

Exactly one fresh native live coding turn was run. Capture, STT and player were fixtures; the selected provider and Kokoro were real. The launcher copied saved provider/model/reasoning choices and skill-routing state into isolated session/UI/memory roots, narrowed the copied execution permission to read-only, and used normal provider credential resolution without printing or copying credentials. Authentication refresh/locking remains the real adapter's normal authorized behavior. The prompt requested a one-sentence explanation of why a Rust mutex must not span blocking playback and explicitly requested no tools/files. This was not a replay of the original conversation or its cognitive history. Request/tool-call counts were not independently instrumented; do not translate “one native turn” into “exactly one HTTP request” or a verified zero-tool trace.

The prior permission for several short real-output checks also covered one long fixed-phrase playback after the local optimization. No microphone, installer, OS setting, volume/device change, new model/runtime download, NPU workload change or installed-binary replacement occurred. No second live provider turn was run. Playback/model benchmarks are explicit authorized local evaluations, separate from isolated foundation verification; the real worker retains its existing pack-lease refresh.

## Live timing result and resulting diagnosis

Command:

```sh
python3 apps/agent-vesper-tui/examples/voice_live_timing.py --execute-live
```

Receipt:

```text
Saved execution choices: {"/mixture": "/mixture off", "/model": "/model gpt-6-astra", "/thinking": "/thinking max"}
t=0.301s Transcribing
t=17.518s Synthesizing voice
{"observed_stage_s": {"Synthesizing voice": 17.518, "Transcribing": 0.301}, "pcm_bytes": [448800], "scope": "one fresh native live turn; fixture capture/STT/player; real selected provider and Kokoro; no acoustic timing", "stop_to_first_pcm_s": 26.99}
```

The model name above is the user's saved selection used by the native adapter in this check, not a new product catalog declaration. Saved project skill routing was Enhanced with model assistance. Those choices were preserved. The first observed synthesis stage came at 17.518 s; first PCM at the fake player came at 26.990 s. The intervening 9.472 s includes local synthesis and observation overhead. The preceding interval includes fixture STT and upstream routing/provider work; this check does **not** isolate how much maximum reasoning versus routing/network contributed. The requested one-sentence answer generated 448,800 bytes (14.025 s of canonical audio), making whole-sentence buffering an evidenced local latency source.

The live launcher did not observe the new upstream wait label. Audit found that `ui::run_status_line` ignored `model.status` during active agent turns and rendered generic activity instead. A failing renderer regression established this separate defect before it was fixed.

## Implementation and invariants

Changed production files:

- `apps/agent-vesper-tui/src/voice_speech_worker.rs` (historical policy measured in this report): units of at most 120 Unicode scalar values stayed whole; longer approved units targeted 56 characters first and 80 thereafter. The follow-up report supersedes those thresholds with a lower-latency 32/28/48 policy. Both versions slice only fully hygiene-approved text, preserve valid UTF-8 and exact reconstruction, leave unsplittable tokens intact, and introduce no reasoning speech, punctuation or filler.
- The existing rendezvous now hands off bounded PCM pieces. One player stream remains open across pieces of the same segment; accepted-byte progress is cumulative, and one terminal outcome is emitted for the entire original segment. The next synthesis overlaps pipe/player work; Stop and generation replacement reject stale pieces and never reopen an old stream. A per-segment failure flag bounds waste after player failure to an already-running inference; the original error is retained. Each prepared PCM piece has a 16 MiB bound, excluding adapter-native allocations; no full reply is accumulated.
- `apps/agent-vesper-tui/src/ui.rs`: active-turn `Voice:` status is visible in the existing activity row with theme-owned colors. Other agent activity remains unchanged.
- `apps/agent-vesper-tui/src/main.rs`: the previously added upstream wait status/test remain; the misplaced helper documentation comment was corrected.

Verification/evaluation files:

- `tests/voice_speech_pipeline.rs`: extends the real-worker fixture with deliberately slow whole-sentence synthesis, early-PCM assertion, within-segment Stop, cumulative counts and exactly one player/terminal outcome for a successful three-piece segment.
- `tests/r3_loop_pty.py`: adds a deliberate one-second provider hold and asserts the upstream wait label in the actual native frame for direct and VRO paths.
- `examples/first_speech_receipt.rs`: fixed nonsensitive long-unit comparison; `--measure` opens no device, `--play-one` requires separate playback consent.
- `examples/voice_live_timing.py`: explicit `--execute-live` only, one fresh native turn, 90-second turn deadline, no automatic retry. The currently implemented credential-path isolation is OpenAI-specific and refuses other selected providers; production remains provider-neutral. The helper does not modify the installed application or saved settings.

No production crate/adapter or provider configuration was changed. ACP has no host-owned device player or F9 frame; this remains the documented terminal-only composition exclusion. Any future host-neutral NPU port must be evaluated for both hosts before it is shipped.

## Regression and benchmark receipts

### Red: whole-sentence buffering

Before phrase subdivision, the expanded production-worker regression failed:

```text
first PCM must arrive before slow whole-sentence synthesis completes
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.19s
```

The synthetic engine delays text longer than 120 characters by six seconds. The first-PCM deadline is four seconds, so only subdividing the real worker job can satisfy it. Stop cleanup still waits for the bounded fixture run to settle. Final expanded regression:

```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.29s
```

It additionally requires `Progress` totals `[3200, 6400, 9600]`, one `Spoke { segment: 7, samples: 4800 }`, one player for the fragmented segment, and exactly one Stale terminal for Stop during its predecessor's partial playback.

### Red: invisible wait stage

```text
running voice stage must not be hidden by generic agent activity: ✦ Beaming…  (20s · TODO 0/0)
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 277 filtered out; finished in 0.00s
```

The repaired renderer regression passed, and both native PTY routes now assert the label during a deliberate provider wait.

### Local benchmark and listening

```sh
cargo run -p agent-vesper-tui --features voice-kokoro --example first_speech_receipt -- --measure
cargo run -p agent-vesper-tui --features voice-kokoro --example first_speech_receipt -- --play-one
```

An initial 80-character first-piece candidate measured whole-unit 8.926 s versus first PCM 5.593 s, with 11.202 s total generation/drain at the fixture. It was improved to the final shorter-first-piece policy rather than advertised as the final result.

Final no-device comparison:

```text
warm whole-unit synthesis open: 8.602s; 188 text bytes; no playback
t=2.723s first observed Sending PCM stage; not acoustic onset
t=2.733s first PCM observed by fixture player
t=2.733s Progress { segment: 1, through_bytes: 133600 }
t=6.381s Progress { segment: 1, through_bytes: 312000 }
t=9.404s Progress { segment: 1, through_bytes: 460800 }
t=9.414s Spoke { segment: 1, samples: 230400 }
```

This is a **5.869 s / approximately 68% local first-PCM reduction** on that fixed text, not a universal latency guarantee. Observed total preparation/delivery increased from 8.602 s to approximately 9.414 s (the latter includes fixture transport/polling): subdivision prioritizes onset and overlaps remaining work with playback rather than claiming reduced total compute. Phrase prosody/audio length can differ; canonical chat text is unchanged.

Authorized output:

```text
warm whole-unit synthesis open: 8.598s; 188 text bytes; no playback
t=2.707s first observed Sending PCM stage; not acoustic onset
t=4.395s Progress { segment: 1, through_bytes: 133600 }
t=9.984s Progress { segment: 1, through_bytes: 312000 }
t=14.505s Progress { segment: 1, through_bytes: 460800 }
t=17.333s Spoke { segment: 1, samples: 230400 }
```

Alex answered **“Yes, complete and natural”** to whether this was complete/natural without long gaps or cut-off words. Later defensive stale-error bookkeeping and renderer changes were verified device-free; no new acoustic claim is inferred from those tests.

## Native NPU: current machine evidence

The old PR-0 “no userland runtime” statement is **stale**, not current readiness. Read-only commands:

```sh
uname -srmo
lspci -nn
ls -l /dev/accel /sys/class/accel
lsmod
ldconfig -p
FLM_DISABLE_UPDATE_CHECK=1 flm version --json
FLM_DISABLE_UPDATE_CHECK=1 flm list --filter installed --json
FLM_DISABLE_UPDATE_CHECK=1 flm list --json
FLM_DISABLE_UPDATE_CHECK=1 flm validate --json
xrt-smi examine --batch
```

Selected verbatim receipts:

```text
Linux 7.2.5-200.fc44.x86_64 x86_64 GNU/Linux
65:00.1 Signal processing controller [1180]: Advanced Micro Devices, Inc. [AMD] Strix/Krackan/Strix Halo Neural Processing Unit [1022:17f0] (rev 10)
{ "version": "1.0.5" }
```

```json
{
    "all_fw_ok": true,
    "amd_device_found": true,
    "devices": [{"cols": 8, "device": "/dev/accel/accel0", "fw_build": 64,
        "fw_major": 1, "fw_minor": 1, "fw_ok": true, "fw_patch": 2}],
    "drm_version": "0.10", "enough_cols": true,
    "kernel": "7.2.5-200.fc44.x86_64", "kernel_ok": true,
    "memlock_limit": "infinity", "memlock_ok": true,
    "object": "npu_stack_validation", "platform": "linux", "ready": true
}
```

```text
XRT
  Version              : 2.26.0
  NPU Firmware Version : 1.1.2.64
Device(s) Present
|[0000:65:00.1]  |NPU Gorgon Point 1  |aie2p         |6x8       |
```

The existing installation includes `/opt/fastflowlm/lib64/libwhisper_npu.so` and XRT/amdxdna libraries. The installed model list reports `qwen3:1.7b`; the full catalog explicitly reports:

```json
{"name":"whisper-v3:turbo","model":"whisper-v3:turbo","installed":false,"size":1000000000,"footprint":0.62}
```

The size/footprint are catalog metadata, not measured download/peak storage. No whisper/kokoro-named cache directory was found in the default FLM model root, and no `FLM_MODEL_PATH` override was set. Stack validation is not voice-model loading or measured NPU inference. No FLM service, model pull, driver update, power-mode change or NPU inference job was launched. Existing workloads were preserved.

## Current primary sources and separate NPU gates

1. [FLM Linux setup](https://github.com/ROCm/FastFlowLM/blob/0cfecb0881de95a5d48e2bdcf02e9b38d04e1ecf/docs/linux-getting-started.md), 4,719 bytes, SHA-256 `3199bbee2a717849e6f71d54d99f131ef17e010a044c0d357a0a1b2c5fde94ed`: documents amdxdna, firmware, XRT and distinguishes stack validation from actual XRT device availability.
2. [FLM Whisper model/API](https://github.com/ROCm/FastFlowLM/blob/0cfecb0881de95a5d48e2bdcf02e9b38d04e1ecf/docs/docs/models/whisper.md), 3,178 bytes, SHA-256 `f5169beca77fb6eaf629bdb8aeb2ea963910a2def14c4a92d62685bde1c5e7f3`: standalone `flm serve --asr 1` is documented for server mode; API `POST /v1/audio/transcriptions`. This is an NPU STT candidate, not evidence of Kokoro synthesis support.
3. [ONNX Runtime Vitis AI EP](https://onnxruntime.ai/docs/execution-providers/Vitis-AI-ExecutionProvider.html), 57,924 bytes, SHA-256 `0200ed0cf90b5416d0465dd6de4254f86aa0f30333a4029cf66bace0a1272a06`: the AMD64/Ryzen AI row lists **Windows**; its Linux rows concern other ARM/AMD adaptable-SoC targets. It also requires a compiler/runtime and model quantization compatibility. This does not establish a drop-in Fedora Kokoro backend and is not a claim that Linux NPU synthesis is impossible.

| Stage | Confirmed now | Missing acceptance / next gate |
|---|---|---|
| Shared NPU readiness | AMD device/driver/firmware, FLM 1.0.5, XRT 2.26.0 and stack/device enumeration | Actual speech-model inference and offload attribution; preserve existing runtimes/workloads |
| NPU STT | Documented native FLM Whisper backend and local runtime library | Whisper model absent; native Settings consent, verified setup/reuse, bounded process/API composition, PCM/privacy/cancellation tests, real NPU transcription/quality/latency evidence |
| NPU TTS | CPU Kokoro is usable; no verified Linux NPU Kokoro path found in inspected runtime/docs | Select a real compatible Linux backend/compiler/model artifact, license/integrity/budget review, native Settings setup and selected-versus-actual backend reporting, measured NPU offload and signal/listening/latency gates |

Both STT and TTS gates remain required by Alex's latest direction. Neither is satisfied by an installed driver, a checkbox, a CPU fallback, an NPU-ready LLM, or this assessment. CPU use stays available and truthfully labeled. Any fallback must disclose its actual backend/reason; activation/setup belongs in native Settings with explicit consent and no normal manual configuration workflow.

## Final verification and audit

```sh
cargo test -p agent-vesper-tui --features voice-kokoro --test voice_speech_pipeline
cargo test -p agent-vesper-tui --features voice-kokoro --lib --test r3_speech_worker --test voice_playback_diagnostics --quiet
cargo test -p agent-vesper-tui --features voice-kokoro --bin agent-vesper-tui voice_wait_status --quiet
cargo clippy -p agent-vesper-tui --all-targets --features voice-kokoro -- -D warnings
cargo build -p agent-vesper-tui --features voice-kokoro --bin agent-vesper-tui
cargo check -p agent-vesper-tui --bin agent-vesper-tui
cargo check -p agent-vesper-tui --features voice-conversation --lib
cargo xtask architecture
```

```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.29s
test result: ok. 278 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.44s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 155 filtered out; finished in 0.05s
architecture boundaries validated for 30 packages
```

Scoped Clippy (`-D warnings`), voice build, default binary and conversation-only checks passed. Final native direct/VRO PTY commands use the same two invocations as the preceding report, now with an explicit one-second provider hold:

```text
LATENCY: recorder process observed=250.4 ms; stop-to-first-PCM=2453.1 ms; complete fixture reply=6514.8 ms; includes PTY polling, synthetic STT/provider with 1 s hold, real Kokoro; not real end-to-end latency
PASS: F9 -> real agent/provider wire -> hygiene -> Kokoro am_michael (off) -> fixture player; PCM bytes=[52000, 124800]; peaks=[22327, 13045]; provider requests=1
Device/listening acceptance NOT performed. No fresh install; existing asset inodes reused.
LATENCY: recorder process observed=250.4 ms; stop-to-first-PCM=2359.9 ms; complete fixture reply=4761.5 ms; includes PTY polling, synthetic STT/provider with 1 s hold, real Kokoro; not real end-to-end latency
PASS: F9 -> real agent/provider wire -> hygiene -> Kokoro af_heart (balanced) -> fixture player; PCM bytes=[50400, 108000]; peaks=[21248, 14919]; provider requests=1
Device/listening acceptance NOT performed. No fresh install; existing asset inodes reused.
```

Audit re-derived: exact text reconstruction, valid UTF-8 boundaries, no input before hygiene, bounded prepared pieces, one stream/segment, cumulative receipts, one terminal settlement, generation-safe Stop and recovery, and visible status in the actual frame. First-speech and rendering regressions both failed before their corresponding fixes. The old empty-runtime and invisible-stage claims are explicitly corrected, not silently retained.

Deviations/limits: background delegation was unavailable, so research was performed directly. `xrt-smi examine --format JSON` returned usage rather than a device report; `examine --batch` produced the receipt above. One combined Cargo invocation accidentally filtered out the integration test; the unfiltered integration test was subsequently run and passed, so the zero-test invocation is not counted as evidence. No full workspace/MSRV/five-platform/release gates were run. The frozen oracle path remains unavailable as recorded previously; it was not altered and no impossibility claim was made. Actual live-microphone transcription, original-history provider latency, post-optimization live-provider timing, and native NPU inference remain open.

DOX: nearest TUI/tests/examples/Kokoro/foundation contracts, evidence index and owning PRD are updated. Root/apps/crates/docs parents retain their existing ownership boundaries and are intentionally unchanged. NPU requirements are recorded without turning research into implementation readiness. Final link/whitespace and artifact-identity receipts are appended at closeout.

### Closeout receipts

```text
PASS: modified Python parses without executing live actions
PASS: 17 relative Markdown link targets in three voice reports/PRD
cargo clippy -p agent-vesper-tui --all-targets --features voice-kokoro -- -D warnings: exit 0
git diff --check: exit 0
HEAD: 94ed16de502e98110498010b399b24659b17a63f
```

Source SHA-256 at closeout:

```text
74bd5418a8ee008a2fa69e8d450449a8b607a670406230a3fb086c83851294a3  apps/agent-vesper-tui/src/voice_speech_worker.rs
ecf90a6112685247c4caa7d14ff022bff0df22d9eecf774a80455c3c69a12a51  apps/agent-vesper-tui/src/ui.rs
8db462fa01dd016f88ea4c3c43d3a8cf976784a439f923439403e4e796c65356  apps/agent-vesper-tui/tests/voice_speech_pipeline.rs
```

The workspace contains extensive pre-existing modified/untracked integration work;
this report does not attribute the entire working-tree diff to this repair. No
commit, push, release, or installation was performed. Final documentation audit
also corrected the PRD traceability row: historical PR-0 detection success is not
completion of the newly requested R16 NPU STT/TTS implementation gates.
