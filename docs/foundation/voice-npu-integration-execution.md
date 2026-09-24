# VRO-17 real NPU speech integration execution

Status: **RECONNAISSANCE COMPLETE; FLM STT IMPLEMENTATION BLOCKED BY MATERIAL VAD-CONTRACT GAP AND ASSET CONSENT; KOKORO NPU TTS GATE NOT ESTABLISHED**

## Objective

Integrate actual, independently selected NPU speech stages through the existing
R16 policy, native Settings, F9 production path, and `VoiceStt`/`VoiceTts`
boundaries while preserving the accepted CPU Kokoro experience and all prior
continuity/list-formatting behavior.

## Baseline protection

The accepted executable exists and is preserved:

```text
target/voice-candidates/agent-vesper-tui-continuity-phoneme-repair
size: 22393288 bytes
sha256: 9d59189a00999fffc5bde5994c49ab7061ecda380c7d7f1e4698522052b54271
```

Repository identity at reconnaissance:

```text
HEAD: 8f258ba28f4ea2f749526fb32b5b180ecc7dcead
workspace: extensive pre-existing modified and untracked voice integration work
kernel: Linux 7.2.5-200.fc44.x86_64 x86_64
```

A candidate name is not treated as identity; the full digest above is the
baseline. The executable is not modified or replaced by this work.

The listening record's `2026-03-03` date was supplied during the acceptance
closeout, but it conflicts with repository evidence and the currently observed
September 2026 kernel/build chronology. No original timestamp receipt was found
that could establish March 3 as the actual listening date. The verdict remains
valid user evidence; the date is retained as a **supplied/unverified record
label**, not asserted as a reconstructed event timestamp. This correction does
not reopen the verdict.

## Reconnaissance gate

### Existing owner → missing behavior → smallest change → production test

| Existing owner | Verified missing behavior | Smallest authorized change | Required production-path test |
|---|---|---|---|
| `vesper_voice::VoiceStt` | No FLM adapter | Composition-boundary Rust adapter using the verified loopback multipart endpoint | Controlled loopback server plus F9 event drain |
| `voice_shared_stt::SharedSidecarStt` | CPU sidecar is the only constructed STT | Select CPU or registered FLM adapter from the saved R16 stage decision; retain one warm owner | CPU zero-call and independent STT/TTS policy tests |
| `voice_accel` | Honest registry is empty | Register FLM for STT only behind real compatibility/setup/readiness inspection | Settings/readiness and strict/automatic routing tests |
| Settings → Voice | Compute policy exists, but no real NPU setup/verify action | Reuse the existing Voice setup screen/result and saved draft; no second settings framework | Real menu save/reload and missing-assets cases |
| Existing process supervision patterns | No owned FLM ASR lifecycle | One lazy, loopback-only owned process only when no safe verified endpoint is configured; bounded output/readiness/shutdown | Busy/lost/cancel/recovery fixture process |
| Kokoro `VoiceTts` and ORT CPU engine | No compatible Linux NPU execution provider or compiled Kokoro artifact established | No TTS production edit until graph/runtime compatibility evidence exists | Preserve CPU continuity/PCM/interruption tests |

No second voice framework, scheduler, VAD pipeline, player, model session, or
configuration writer is justified.

### Current machine and runtime evidence

Read-only inspection produced:

```text
flm: /usr/bin/flm
FLM version: 1.0.5
xrt-smi: /usr/bin/xrt-smi
XRT version: 2.26.0
amdxdna kernel: 7.2.5-200.fc44.x86_64
NPU firmware: 1.1.2.64
/dev/accel/accel0: present, mode crw-rw-rw-, group render
Alex groups: includes render
libwhisper_npu.so: /opt/fastflowlm/lib64/libwhisper_npu.so, 2164688 bytes
active FLM/Whisper service before recon: none observed
available filesystem headroom: 262269251584 bytes
```

The installed FLM catalog identifies `Whisper-V3-Turbo-NPU2` with files
`config.json`, `tokenizer.json`, `tokenizer_config.json`, and `model.q4nx`;
catalog download size is 1,000,000,000 bytes and catalog footprint is 0.62.
These are vendor metadata, not measured retained/peak storage. A read-only
repository inventory at immutable model revision
`594eecd2d80b20cbb04ef0099162335d1dd1899a` enumerated 654,144,161 bytes in
11 files. The principal `model.q4nx` is 650,175,128 bytes with Hugging Face
LFS SHA-256 `8fb97604bf5762ee26efa696cfc9eb70724110358c7b4ca628a7973bf8a16291`.
The inventory receipt SHA-256 is
`2aab174d84051bb098e8ed935384ff2636c4b92404fa8fd70e641908428fd216`.
This establishes a pin and download floor, but not FLM's temporary/compiled
peak or final retained bytes. The installed model list contains only
`qwen3:1.7b`; Whisper is not installed.

### Installed-version backend contract

`flm serve --help` for installed 1.0.5 confirms:

```text
-a, --asr arg (=0)
--host arg (=127.0.0.1)
-p, --port arg (=-1)
-s, --socket arg (=10)
-q, --q-len arg (=10)
--cors arg (=1)
--quiet
```

Primary FLM documentation at the inspected upstream revision documents:

```text
flm serve --asr 1
POST /v1/audio/transcriptions
multipart fields: model plus file
example model request value: whisper-v3
standalone ASR server supported from FLM v0.9.21
```

Source receipts fetched read-only:

```text
f5169beca77fb6eaf629bdb8aeb2ea963910a2def14c4a92d62685bde1c5e7f3  FLM whisper.md (3178 bytes)
3199bbee2a717849e6f71d54d99f131ef17e010a044c0d357a0a1b2c5fde94ed  FLM linux-getting-started.md (4719 bytes)
ac95d836f6f71263580564cc243743f60d613cd3cc3657d1241a3463b9b98a59  FLM README.md (8884 bytes)
```

The documentation does not establish partial streaming, a distinct VAD result,
language/task fields for installed 1.0.5, request cancellation that aborts NPU
inference, or per-request offload receipts. Integration must therefore remain
final-only, avoid invented request fields, reject empty success as unverified
rather than call it VAD-confirmed silence, suppress late canceled results, and
obtain actual placement evidence separately.

### STT gate verdict

**BLOCKED before production integration.** The machine, installed runtime
version, standalone server mode, loopback endpoint, multipart shape, and
NPU-specific model artifact are concrete. Source inspection then established
that this endpoint provides no verified VAD/silence contract and ignores its
request cancellation token. Vesper has no shared preprocessing owner to fill
that gap without a reviewed contract change. The model is also absent and is an
additional approximately 1 GB catalog download; its measured retained/peak cost
is not yet known. No download is authorized without itemized native confirmation.

### TTS gate verdict

**NOT ESTABLISHED.** The accepted Kokoro path uses a pinned CPU ONNX Runtime
library and `model_quantized.onnx`. FLM exposes a Whisper NPU library/model but
no Kokoro TTS route. Existing primary evidence for ONNX Runtime Vitis AI on
Ryzen AI does not establish this Fedora/Linux graph as supported, and no
compatible compiled Kokoro artifact, execution provider, operator partition,
or per-request NPU trace is installed. This does not prove all NPU TTS
impossible. It blocks speculative TTS production changes while CPU Kokoro
remains the truthful route.

## Commands executed before production edits

```sh
git status --short
git rev-parse HEAD
uname -a
sha256sum target/voice-candidates/agent-vesper-tui-continuity-phoneme-repair
FLM_DISABLE_UPDATE_CHECK=1 flm version --json
FLM_DISABLE_UPDATE_CHECK=1 flm serve --help
FLM_DISABLE_UPDATE_CHECK=1 flm list --filter installed --json
xrt-smi --version
ls -l /dev/accel /dev/accel/*
find /opt/fastflowlm -maxdepth 3 -type f
ps -eo pid,user,cmd
df -B1 /home/Alex /opt/fastflowlm
```

No model pull, server start, NPU inference, microphone/speaker access, settings
write, driver change, install, process termination, or installed-app replacement
occurred during this gate.

## Installed-version source audit and stop gate

The exact upstream source at `0cfecb0881de95a5d48e2bdcf02e9b38d04e1ecf`
was cloned read-only under `/tmp` and inspected before production edits.
`src/server/server.cpp:1027-1038` parses only multipart `model` and `file`.
`src/server/rest_handler.cpp:1340-1381` always invokes transcription, does not
consume the request cancellation token, and returns an empty JSON object when
ASR was not loaded. The three booleans passed to `Whisper::generate` are
`enable_time_stamp` and `return_time_stamp` controls, not VAD. No endpoint-owned
VAD contract or silence classification was found.

The current Vesper VAD owner is backend-local:
`apps/agent-vesper-tui/src/voice_transcribe.py` and
`crates/vesper-voice/src/stt_sidecar.rs` enforce faster-whisper
`vad_filter=True`. There is no shared preprocessing owner that can truthfully
set FLM's `SttDescriptor.vad_enabled = true`. Registering FLM now would either
violate the binding Whisper VAD contract or require keeping the CPU recognizer
in the NPU path, which the mission explicitly forbids.

This is a **material architecture/scope gate**, not an ordinary implementation
milestone: proceeding requires approving a narrow shared on-device VAD
preprocessor/port (with silence provenance and production-path tests) or a
verified FLM release/interface that provides equivalent VAD. It also requires
native confirmation for the absent approximately 1 GB Whisper asset after its
real retained/peak budget is measured. Therefore no production route, setup
button, decorative Ready state, or candidate was created. This is the
authorized stop condition for an unsupported route; manufacturing an adapter
would weaken R16.

## Reviewed architecture decision

Alex approved the narrow shared local Silero VAD preprocessing route using the
already-installed faster-whisper environment, and approved implementing a
confirmed native setup flow for the FLM Whisper model. This resolves the policy
question but does not make the route ready: the implementation still needs a
persistent bounded VAD owner, model-payload manifest/budget measurement, owned
FLM supervision, late-result suppression, and production-loop regressions.
Those components were not manufactured as an unverified partial adapter in this
work unit.

## Implementation, verification, and closeout

No production code changed. The accepted CPU executable and CPU voice path are
byte-preserved. NPU STT remains **NOT IMPLEMENTED / BLOCKED** by the verified
VAD ownership gap and absent consented model. NPU TTS remains **NOT IMPLEMENTED
/ BLOCKED** by the separately unestablished Kokoro Linux-NPU graph/runtime path.
No candidate is offered and no real-offload claim is made.
