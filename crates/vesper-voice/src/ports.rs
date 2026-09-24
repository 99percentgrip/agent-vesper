//! STT/TTS ports and descriptors (PRD §2.2, decisions D6/D7).
//!
//! Ports mirror the house provider-port shape (cancellation in, futures
//! out, descriptor/catalog out) without importing any provider type.
//! PR-0 ships no real implementations; [`crate::fakes`] exercises these
//! contracts deterministically and is the only permitted stand-in until
//! PR-1/PR-2 adapters land.

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use vesper_domain::{BoundedString, ProviderId};

use crate::audio::PcmFrame;
use crate::cancel::VoiceCancel;
use crate::error::{TtsMidStreamError, VoiceError};

/// Owned future type for port operations.
pub type VoiceFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// How a provider produces live partial transcripts (D7).
///
/// The two kinds are not interchangeable labels: the reference implements
/// periodic re-transcription of the buffered capture, not incremental
/// streaming inference, and text must never claim otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartialsKind {
    /// Repeated transcription of the utterance buffer so far (the
    /// reference mechanism: periodic repass over bounded audio).
    BufferedRepass,
    /// Provider-maintained incremental inference state.
    Incremental,
}

/// Provider-declared locality + egress class (PRD §2.9, R14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpeechEgressClass {
    /// Runs on the user's device; audio/text never leaves it.
    OnDevice,
    /// Runs on a configured self-hosted endpoint.
    SelfHostedRemote,
    /// Runs on a third-party cloud service.
    ThirdPartyCloud,
}

/// What an STT provider is and supports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SttDescriptor {
    /// Provider identity (registry key).
    pub provider: ProviderId,
    /// Engine/model identity (metadata).
    pub model: BoundedString<128>,
    /// Egress class for speech data.
    pub egress: SpeechEgressClass,
    /// Partial mode, if any.
    pub partials: Option<PartialsKind>,
    /// Whether the engine applies VAD (silence filtering). Vesper's
    /// contract requires VAD enabled for Whisper-class engines
    /// (`docs/voice-trailing-silence-vad-prd.md`); a descriptor declaring
    /// `false` is a defect, not a feature.
    pub vad_enabled: bool,
    /// Supported language tags (BCP-47-ish metadata, bounded).
    pub languages: Vec<BoundedString<16>>,
}

/// What a provider's *absence of transcript* actually proves (D21).
///
/// `VadConfirmedSilence` requires a provider that reports VAD-filtered
/// silence as a distinct, truthful outcome. A legacy worker that catches
/// engine exceptions server-side and returns empty text cannot make that
/// claim: an empty response proves only "no transcript was returned",
/// not verified silence. Clients must not treat legacy empties as
/// confirmed silence; adapters declare which guarantee they can make.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TranscriptProvenance {
    /// Transcript text produced by completed inference.
    InferredText,
    /// Provider-verified silence (distinct silence outcome, not an
    /// exception-swallowing empty string).
    VadConfirmedSilence,
    /// No transcript was returned; the provider discarded the
    /// distinction between silence and engine failure (legacy worker
    /// protocol shape). Honest placeholder — never labeled as confirmed
    /// silence.
    LegacyEmptyResponse,
}

/// A finalized transcription.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SttTranscript {
    /// Final text. Exactly one final result per accepted capture; it
    /// always supersedes any partial work.
    pub text: BoundedString<8192>,
    /// Provider that produced it (after any failover).
    pub provider: ProviderId,
    /// Provider-declared confidence metadata, if any (never fabricated).
    pub confidence: Option<BoundedString<32>>,
    /// What this result proves. `text` is empty exactly when provenance
    /// is `VadConfirmedSilence` or `LegacyEmptyResponse`; empty text
    /// with `InferredText` is a contract violation adapters must not
    /// produce.
    pub provenance: TranscriptProvenance,
}

/// Live partial transcription surface. Best-effort by contract:
///
/// - Calls neither block capture callbacks nor the host event loop (the
///   host invokes partial work on its own worker; the core never spawns).
/// - Partial text is interim display data only: never submitted as a
///   turn input, never treated as an executable command, and always
///   superseded by the final [`VoiceStt::transcribe`] result.
pub trait SttPartial: Send + Sync {
    /// Offers the capture buffer so far; returns interim text when the
    /// provider has a better estimate than before.
    ///
    /// # Errors
    ///
    /// [`VoiceError`] classified as usual; a failing partial pass never
    /// fails the turn and never invalidates the final path.
    fn partial<'a>(
        &'a self,
        audio: &'a [PcmFrame],
    ) -> VoiceFuture<'a, Result<Option<BoundedString<2048>>, VoiceError>>;
}

/// Speech-to-text port. Interchangeable provider boundary.
pub trait VoiceStt: Send + Sync {
    /// Transcribes a complete captured utterance.
    ///
    /// # Errors
    ///
    /// [`VoiceError::NoSpeech`] for VAD-filtered silence (nonfatal),
    /// [`VoiceError::Unavailable`] when the provider cannot be reached
    /// (failover-eligible), and the rest per the taxonomy.
    fn transcribe<'a>(
        &'a self,
        audio: &'a [PcmFrame],
        cancel: &'a VoiceCancel,
    ) -> VoiceFuture<'a, Result<SttTranscript, VoiceError>>;

    /// Partial surface, when the provider supports one.
    fn partial(&self) -> Option<&dyn SttPartial> {
        None
    }

    /// Stable descriptor.
    fn descriptor(&self) -> &SttDescriptor;
}

/// One synthesized audio stream item (D6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TtsChunk {
    /// One validated audio frame.
    Audio(PcmFrame),
    /// Clean end of synthesis. Proves the provider finished — **never**
    /// that the user heard it (playback acknowledgments are separate,
    /// PRD §2.3).
    Finished,
}

/// What a TTS provider is and supports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TtsDescriptor {
    /// Provider identity.
    pub provider: ProviderId,
    /// Engine/model identity.
    pub model: BoundedString<128>,
    /// Egress class for speech text.
    pub egress: SpeechEgressClass,
    /// Whether synthesis streams incrementally (audio before full text
    /// completes) or returns only after full synthesis.
    pub streaming: bool,
    /// Whether this provider requires mandatory pre-egress hygiene
    /// (always true for [`SpeechEgressClass::ThirdPartyCloud`]).
    pub requires_hygiene: bool,
}

/// A selectable voice profile (catalog-owned; never invented).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceProfile {
    /// Provider-unique voice identity.
    pub voice_id: BoundedString<128>,
    /// Display name.
    pub label: BoundedString<128>,
    /// Declared output format metadata (must match canonical after any
    /// host-side conversion; adapters that cannot emit canonical PCM say
    /// so and hosts convert at their boundary).
    pub sample_rate_hz: u32,
}

/// Stream of synthesized audio; errors after the first frame surface as
/// [`TtsMidStreamError`] on the stream, not as an open-time error.
pub type TtsStream<'a> =
    Pin<Box<dyn futures_core::Stream<Item = Result<TtsChunk, TtsMidStreamError>> + Send + 'a>>;

/// Text-to-speech port. Interchangeable provider boundary.
pub trait VoiceTts: Send + Sync {
    /// Opens one synthesis stream for a hygiene-passed text unit.
    ///
    /// # Errors
    ///
    /// Open-time errors (auth/quota/unavailable/invalid input) return
    /// `Err` before any audio; failures after audio begins surface on
    /// the returned stream as [`TtsMidStreamError::AudioFailed`].
    fn synthesize<'a>(
        &'a self,
        text: &'a str,
        voice: &'a VoiceProfile,
        cancel: &'a VoiceCancel,
    ) -> VoiceFuture<'a, Result<TtsStream<'a>, VoiceError>>;

    /// Stable descriptor.
    fn descriptor(&self) -> &TtsDescriptor;

    /// Voice catalog. Empty only when the provider genuinely exposes
    /// none; never fabricated.
    ///
    /// # Errors
    ///
    /// [`VoiceError`] per the taxonomy.
    fn voices<'a>(&'a self) -> VoiceFuture<'a, Result<Vec<VoiceProfile>, VoiceError>>;
}

/// Failover semantics shared by STT/TTS chains (contract frozen in PR-0;
/// the composition wrapper lands in PR-1 with the first real adapters).
///
/// Binding rules: `Unavailable` advances to the next adapter;
/// `NoSpeech`/`Auth`/`Quota` stop the chain; failover across
/// [`SpeechEgressClass`] boundaries requires the configuration to permit
/// the target provider's data flow (PRD §2.9) — a local-only policy
/// never silently reaches a remote class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailoverPolicy {
    /// Try adapters in configured order under the binding rules.
    OrderedFailover,
    /// Use only the first configured adapter; never advance.
    Single,
}
