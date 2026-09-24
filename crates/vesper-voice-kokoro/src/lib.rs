//! VRO-17 R3: the local neural TTS composition adapter — the Kokoro-82M
//! acoustic model (ONNX export, pinned revision) behind the pure core's
//! [`vesper_voice::ports::VoiceTts`] port, with espeak-ng as the verified
//! pronunciation component and the official ONNX Runtime CPU library as
//! the verified inference runtime.
//!
//! Boundary rules (binding):
//! - **Reusable model weights and the narrowly reviewed inference runtime
//!   are reused, not first-party.** Upstream licenses (Apache-2.0 model,
//!   voices, export; MIT ONNX Runtime; GPL-3.0 user-installed espeak-ng)
//!   are honored; nothing is rebranded, copied into the repository, or
//!   distributed as Vesper content. The Vesper-specific implementation in
//!   this crate is deliberately small: pack integrity, pronunciation
//!   bridge, bounded inference, and the port adapter.
//! - The adapter is **synthesis-only** like the PR-2 subprocess engine:
//!   text in → validated canonical PCM out. It never plays audio, never
//!   creates files, never treats process exit as playback. Playback stays
//!   the host's `PlaybackOwner`.
//! - The pack is **optional, user-installed, per-user, and shared across
//!   projects/updates**. Absent pack = truthful `Unavailable` with setup
//!   guidance. No download is ever started by an inference request.
//! - The baseline system engine stays selectable and untouched; the host
//!   owns provider/voice selection and never falls back silently.

#![forbid(unsafe_code)]

#[cfg(feature = "ort")]
pub mod engine;
pub mod pack;
#[cfg(feature = "ort")]
pub mod phonemize;
#[cfg(feature = "ort")]
pub mod setup;
pub mod vocab;

pub use pack::{
    PINNED_REVISION, PROVIDER_ID, PackProblem, PackRecord, RETAINED_BUDGET_BYTES,
    RETAINED_PACK_BYTES, RUNTIME_ASSET, RUNTIME_VERSION, assess as assess_pack, pack_root,
};
pub use vocab::PhonemeVocab;

#[cfg(feature = "ort")]
pub use engine::{EngineState, KokoroEngine, MODEL_SAMPLE_RATE_HZ, StylePack, resolve_phonemizer};

#[cfg(feature = "ort")]
mod adapter;
#[cfg(feature = "ort")]
pub use adapter::KokoroTts;

#[cfg(feature = "mock-synthesis")]
mod mock;
#[cfg(feature = "mock-synthesis")]
pub use mock::MockSynthesisTts;

/// The voice profile ids this build supports (exact pack voice ids).
pub const SUPPORTED_VOICES: [&str; 2] = ["af_heart", "am_michael"];

/// Crate-unique provider id (used by error taxonomy entries).
#[must_use]
pub fn provider_id() -> vesper_domain::ProviderId {
    vesper_domain::ProviderId::new(PROVIDER_ID).expect("static id fits")
}
