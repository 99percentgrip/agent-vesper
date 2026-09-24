# VRO-17 Local Neural Voice Audition — Kokoro-82M Feasibility & Asset Budget

Scope: bounded audition of Kokoro-82M as a local neural TTS candidate.
**No downloads, no installations, no production integration.** Alex
needs a listening choice and an exact setup proposal before any asset
budget is approved.

## 1. Baseline (what works now)

- Revision `94ed16d` on `main`; 41 dirty paths (all VRO-17; preserved).
- F9 conversation confirmed working by Alex (see
  `voice-oracle-pr4-repair.md` device-acceptance section): voice in →
  agent → voice out.
- Current TTS: espeak-ng synthesis-only adapter (feature
  `tts-subprocess`); the system-engine baseline is functional and
  unchanged. The seam is `crate::ports::VoiceTts` — the adapter returns
  canonical 16 kHz mono s16 PCM through `TtsChunk::Audio/Finished` and
  the `PlaybackOwner` plays it. Any new engine plugs into the same port.
- No Kokoro assets exist on this machine (verified: no `*kokoro*` under
  `~/.local/share` or the Hugging Face cache). Local probe: **not run**.

## 2. Listening options for Alex

### Option A — Creator-published samples (remote, no setup)

Verified accessible and belonging to the creator:

- **af_heart** (Overall grade A, Target Quality A):
  [af_heart_0.wav](https://huggingface.co/hexgrad/Kokoro-82M/resolve/main/samples/af_heart_0.wav)
  (232 KiB), [af_heart_3.wav](https://huggingface.co/hexgrad/Kokoro-82M/resolve/main/samples/af_heart_3.wav)
  (1 375 KiB), [af_heart_5.wav](https://huggingface.co/hexgrad/Kokoro-82M/resolve/main/samples/af_heart_5.wav)
  (1 009 KiB). Six samples total; `SAMPLES.md` lists the exact text and
  IPA phonemes for each.
- **HEARME.wav** (973 KiB) — the creator's showcase recording.
- **am_michael**: **no samples exist on the creator repo** (verified:
  the `samples/` tree contains only HEARME + af_heart × 6). The only
  public am_michael recording is the CDN-hosted sample in the
  onnx-community README:
  [am_michael.wav](https://cdn-uploads.huggingface.co/production/uploads/61b253b7ac5ecaae3d1efe0c/IPKhsnjq1tPh3JmHH8nEg.wav)
  — provenance: uploaded by the ONNX-community maintainer, not the
  creator. Listen with that caveat.

These samples use each voice's own text (not our passages). They
establish how that voice sounds in that recording; they do not
establish quantized-CPU quality or speed on Alex's laptop.

### Option B — Creator-linked interactive demo (remote)

[hexgrad/Kokoro-TTS](https://huggingface.co/spaces/hexgrad/Kokoro-TTS)
(Hugging Face Space, Gradio, Apache-2.0). Verified real: creator is
`hexgrad`, same account as the model. Has a voice dropdown and a
custom-text field. Alex types his own text, selects `af_heart` or
`am_michael`, and listens. **Remote processing** — text is sent to the
HF Space. No repository contents, private conversations, or microphone
audio are involved; Alex initiates any submission himself.

### Controlled-comparison note

For a controlled af_heart vs am_michael comparison on identical text,
either use Option B's custom-text field, or approve the local probe
(§5) to generate the three audition passages with both voices.

## 3. Route assessment (two candidates inspected)

### Candidate 1: hexgrad/kokoro (reference Python pipeline)

Apache-2.0; active (pushed 2025-08-06). **Rejected for Vesper
integration** — it requires `torch`, `transformers`, `misaki[en]`
(which pulls `spacy`, `spacy-curated-transformers`, `phonemizer-fork`,
`espeakng-loader`, `num2words`), i.e. a full Python/Torch environment.
This is the exact shape the PR-2 §2.7 gate was designed to prevent
bundling. It remains the upstream reference for algorithm behavior.

### Candidate 2: onnx-community/Kokoro-82M-v1.0-ONNX + `ort` crate (RECOMMENDED)

- **Model export**: `onnx-community/Kokoro-82M-v1.0-ONNX` (Apache-2.0);
  eight quantizations available. Recommended: `model_q8f16.onnx`
  (82.0 MiB) for the smallest footprint with acceptable quality; the
  fp32 original (310.5 MiB) is available if quality requires it.
- **Rust binding**: `ort` 2.0.0-rc.13 (MIT OR Apache-2.0), MSRV **1.88**
  — matches Vesper's MSRV exactly. Wraps Microsoft ONNX Runtime 1.30
  (MIT). Active (pushed 2026-09-16). The crate downloads/bundles the
  platform ONNX Runtime native library at build time.
- **Inputs** (verified from the ONNX-community README):
  `input_ids` (i64, shape [1, ≤512]), `style` (f32, shape [1, 256]),
  `speed` (f32, shape [1]). Token IDs come from a char→ID vocab in
  `config.json`; style vectors come from per-voice `.bin` files
  (shape [-1, 1, 256], indexed by token count).
- **Phonemization**: the model takes **IPA phoneme IDs**, not text.
  espeak-ng `--ipa -v en-us` produces the required IPA string —
  **verified 100 % char coverage** against `config.json` `vocab` for
  our audition sentences. The official kokoro-js binding uses the same
  approach (espeak-ng WASM phonemizer). espeak-ng is the phonemizer
  **only** — the acoustic generation is entirely Kokoro/ONNX. It does
  not substitute baseline audio, and the dependency already exists
  (installed espeak-ng 1.52.0; no new binary needed).
  **Limitation honestly stated**: espeak-ng `--ipa` is not the full
  misaki pipeline (no number expansion, limited abbreviation handling);
  complex text normalization is a known gap. The PR-2 hygiene gate
  already handles numbers and abbreviations before text reaches TTS,
  which partially compensates.
- **Output**: 24 000 Hz mono f32 → requires conversion to the canonical
  16 000 Hz s16. The existing bounded linear resampler
  (`tts_subprocess.rs`) already handles arbitrary input rates; a
  24 000→16 000 variant is the same code path.
- **Streaming**: the model is **buffered per call** (text in → full
  audio out). No true streaming. The sentence gate dispatches short
  units (typically 1–3 sentences), so per-call latency is bounded; the
  descriptor will advertise `streaming: false`.

### Why this route

| Criterion | Route 2 (ONNX + ort) | Baseline (espeak-ng) |
|---|---|---|
| Voice quality | Neural (near-human per samples) | Formant robotic |
| Voice choice | 20 English voices (20 grades A–F+) | espeak-ng voice list |
| Licenses | All Apache-2.0 / MIT | GPL-3.0 (system binary, process boundary) |
| New Cargo deps | `ort` (1 crate) | 0 |
| Model download | 82 MiB (q8f16) | 0 (system) |
| MSRV | 1.88 (match) | n/a |
| Streaming | Buffered per sentence | Buffered per sentence |
| CPU-only | Yes | Yes |

License posture: the model, voice packs, ONNX export, and `ort` binding
are Apache-2.0 or MIT — all permitted by `deny.toml`. espeak-ng remains
a system binary invoked at a process boundary (GPL-3.0 engine; Vesper
distributes nothing). No `deny.toml` change, no new vendored binary,
no attribution removal.

## 4. Asset budget manifest (for Alex's approval)

### Downloads (one-time, ~82.5 MiB total)

| File | Source | Size (bytes) | Purpose |
|---|---|---|---|
| `onnx/model_q8f16.onnx` | `onnx-community/Kokoro-82M-v1.0-ONNX` | 85 997 568 | Neural model (quantized) |
| `voices/af_heart.bin` | same repo | 522 240 | Style vector (af_heart) |
| `voices/am_michael.bin` | same repo | 522 240 | Style vector (am_michael) |
| `tokenizer.json` | same repo | 3 482 | Token→ID mapping |
| `config.json` | same repo | ~1 200 | Vocab + model config |

**Total download**: ≈ 85 997 568 + 1 044 480 + 4 682 ≈ **87.0 MiB**

### Installed size

All five files stored as-is (no extraction): ≈ **87.0 MiB** under
`~/.local/share/agent-vesper/tts-kokoro/`.

### ONNX Runtime native library

The `ort` crate downloads the ONNX Runtime shared library (~10–20 MiB
platform-dependent) into `target/` at build time. This is a **build
dependency**, not a runtime file; it does not add to the installed
voice footprint.

### Peak temporary

Model deserialization allocates ~82 MiB RAM during inference; no
additional disk peak. No extraction archives.

### Existing assets reused in place

- espeak-ng binary + data (`/usr/share/espeak-ng-data`, 23.0 MiB):
  used for `--ipa` phonemization only.
- Playback player (`aplay`): used as-is.
- Voice venv: **not needed** for this route (no Python).

### Total new disk: ≈ 87.0 MiB (+10–20 MiB build-time ONNX Runtime in `target/`)

### Unknowns (recorded, not disguised)

- Inference speed of q8f16 on the AMD Ryzen AI 9 465 CPU — unmeasured.
- RAM during inference — bounded by the model size but not measured.
- Quality of the q8f16 quantization vs fp32 — unlistened.
- espeak-ng `--ipa` quality for unusual words/abbreviations — the
  hygiene gate compensates partially, but mispronunciations will occur.

## 5. Local probe status

**Not run** — no Kokoro assets exist locally, and this unit does not
authorize downloads. If the budget is approved, the probe procedure is:

1. Download the five files above into
   `~/.local/share/agent-vesper/tts-kokoro/`.
2. Run a standalone probe (outside the live conversation): synthesize
   the three audition passages + "Understood." with af_heart and
   am_michael; record cold/warm timing, first-audio timing, output
   sample rate, and conversion to canonical 16 kHz.
3. Save bounded audition WAVs (≤8 MiB/file, ≤32 MiB aggregate) for
   Alex to listen to and choose.
4. Report actual measurements; no quality claims before listening.

## 6. Integration proposal (for later, not this unit)

A `tts-kokoro` feature-gated adapter implementing `VoiceTts`:

- Spawn or in-process `ort` inference session; construct token IDs from
  `espeak-ng --ipa` output + `config.json` vocab; load style vector
  from the selected voice `.bin`; run inference; convert 24 kHz→16 kHz;
  return canonical `PcmFrame`s through the existing `TtsChunk` stream.
- Native Settings → Voice gains a provider selector (system-engine /
  Kokoro) reading the voice scope. The system-engine baseline remains
  unchanged and available; no silent fallback.
- Mid-stream semantics: a single `TtsChunk::Finished` after the full
  PCM; the buffered-per-sentence descriptor is truthful.

Integration acceptance: Settings → provider → F9 → speech, plus
interruption across multiple turns, re-proven with real audio.
Valid PCM alone is not acceptance.

## 7. Summary for Alex

1. **Listen now** (no setup): click the af_heart sample links above;
   for am_michael use the CDN sample (provenance caveat noted) or the
   interactive demo at https://huggingface.co/spaces/hexgrad/Kokoro-TTS
   (remote; your text is processed by the Space).
2. **If you want local neural voice**: approve the ~87 MiB asset
   budget (§4); I will download the exact files, run the bounded probe,
   generate audition samples of the three passages + "Understood." for
   both voices, and report measurements.
3. **If you like what you hear**: a narrow `tts-kokoro` adapter is a
   focused follow-up — same port, same playback, no engine change.

The system-engine baseline stays as-is regardless.

## Verification

- All URLs verified accessible and belonging to the stated source
  (creator model `hexgrad/Kokoro-82M`, creator Space
  `hexgrad/Kokoro-TTS`, ONNX export `onnx-community/Kokoro-82M-v1.0-ONNX`).
- af_heart/am_michael availability verified against the actual file
  trees (creator samples: af_heart only; ONNX voices: both).
- Vocab coverage computed from the real `config.json` against real
  espeak-ng `--ipa` output on this machine.
- `ort` MSRV verified from the upstream `Cargo.toml` (1.88 = ours).
- No downloads, no installations, no device use, no production change.
