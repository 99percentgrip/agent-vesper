# Vesper Voice Kokoro Adapter

## Purpose

Own the VRO-17 R3 Natural Voice pack composition adapter: the Kokoro
acoustic model (ONNX export, pinned immutable revision) behind the
`vesper-voice` pure core's `VoiceTts` port, with espeak-ng as the
verified pronunciation component and the pinned ONNX Runtime CPU library
as the verified inference runtime.

## Ownership

- `pack.rs` — the pack descriptor (pinned assets, sizes, sha256
  digests, installed layout, remote URLs at the pinned revision), the
  `pack.json` installation record (atomic write; `verified` published
  only after digests AND the silent-synthesis probe), integrity
  validation with identity-gated caching, the managed per-user cache
  root, ownership-safe removal, and budget constants (retained/peak).
- `phonemize.rs` — speech-only text normalization → espeak-ng IPA
  (bounded subprocess, fixed argv, process-group kill-on-drop) →
  official post-processing rules (reimplemented algorithm from the
  published pipeline behavior; no source port) → model input IDs with
  the defined upstream strip-out-of-vocab behavior surfaced as a count.
- `vocab.rs` — the pack-owned phoneme→ID vocabulary parse (115 symbols).
- `engine.rs` — the ORT session (one per process, lazily built, 2
  intra-op threads), style-vector packs and row indexing, waveform
  validation (finite, bounded), 24 kHz → canonical 16 kHz s16
  conversion (never relabeled), PATH-resolved phonemizer discovery.
- `adapter.rs` — the `VoiceTts` port wiring: open-failure vs mid-stream
  taxonomy, truthful `streaming: false`, cancellation-before-run, the
  shared-session serialization, per-selection engine rebuilds.
- `setup.rs` — the production pipeline: plan/space gate → resumable
  bounded curl downloads (HTTPS-only, pinned sizes) → digest
  verification → extraction of the fixed runtime members → backup/
  restore of prior packs → silent-synthesis probe → record publish;
  plus lease-based active-user protection for removal.
- `mock.rs` — dev/test-only deterministic double (`mock-synthesis`
  feature; never a production path).

## Local Contracts

- Alex requests native NPU STT and TTS with separate acceptance gates and CPU
  still usable. This adapter currently implements CPU Kokoro only; AMD device,
  XRT/FLM presence and stack validation do not establish Kokoro NPU offload.
  A future TTS backend needs a verified compatible model/runtime, consented native
  Settings setup, selected-versus-actual backend reporting and measured offload,
  signal, listening and latency evidence. Never turn detection into an enabled
  acceleration claim. Current gates: `docs/foundation/voice-first-speech-and-npu-assessment.md`.

- `assess` caches at most eight successful verifications by canonical root and
  complete asset identity. Unix identity includes device/inode/ctime alongside
  size/mtime; changed/missing files invalidate and full digests remain pinned.
  Cache no failures; mutation detected during verification fails closed. Other
  platforms keep unconditional hashing. `verify_pack` is always uncached.
- `KokoroTts::prepare` loads the verified session without synthesizing audio;
  hosts invoke it only on a background worker after voice activation.

- Model waveform values are normalized float32, not integer-scale samples.
  Convert amplitude by multiplying by 32768, round and saturate to s16, and
  convert 24 kHz to canonical 16 kHz before little-endian serialization.
  Signal regression checks must assert amplitude, not merely frame/byte counts.

- Dependencies: workspace `vesper-domain`, `vesper-security`,
  `vesper-voice` only; `ort` behind the default-off `ort` feature with
  `default-features = false`, features exactly `std, load-dynamic,
  ndarray`. No build-time native linking and no build-script downloads:
  the binary launches without the pack and offers setup.
- The crate must compile bare (`cargo test -p vesper-voice-kokoro`);
  all adapter/engine/setup code is feature-gated.
- Upstream weights/runtime are reused third-party content: pinned
  revision + digests are the identity; licenses (Apache-2.0 / MIT /
  GPL-3.0 espeak as an external system component) are disclosed in the
  user-facing Details panel and preserved beside the runtime library.
  Nothing is vendored into the repository.
- Never download at inference time; an inference request with a missing
  pack returns setup guidance. Never claim device/audio acceptance;
  playback acknowledgments remain the host's `PlaybackOwner` semantics.
- Readiness must stay cheap per call: identity-gated digest checks, no
  inference on redraws.
- `#![forbid(unsafe_code)]` (the ORT unsafe lives inside `ort-sys`).

## Work Guidance

- PRD: `docs/voice-oracle-extraction-prd.md`; evidence:
  `docs/foundation/voice-oracle-kokoro-implementation.md` (includes the
  recorded q8f16 → `model_quantized.onnx` decision and budgets).
- The TUI composition (`voice-kokoro` feature) owns selection/install
  UX; this crate owns no UI.

## Verification

- `cargo run -p vesper-voice-kokoro --features ort --example latency_receipt`
  measures installed-pack cold/warm assessment, session load and fixed-phrase
  synthesis without devices, downloads or audio files. It is a local measurement,
  not a live end-to-end latency or cross-platform guarantee.

- `cargo run -p vesper-voice-kokoro --features ort --example signal_receipt`
  explicitly reads an already-installed verified pack and checks real PCM signal
  amplitude for both voices without speakers, microphone, downloads or audio files.
  Missing assets fail; this optional local gate is not human audibility evidence.

- `cargo test -p vesper-voice-kokoro` (bare) and
  `cargo test -p vesper-voice-kokoro --features ort,mock-synthesis`.
- `cargo xtask architecture` enforces the dependency allowlist.
- Real setup/performance receipts run only via the `ort`-gated examples
  (`real_setup_receipt`, `perf_receipt`) — evidence helpers, never
  registered as tools.

## Child DOX Index

No children.
