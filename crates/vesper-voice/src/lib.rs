#![forbid(unsafe_code)]
//! VRO-17 PR-0: provider-neutral voice contracts (no adapters, no I/O).
//!
//! This crate owns the **pure core** of the voice subsystem extracted from
//! the voice oracle reconnaissance (`docs/architecture/recon_voice_oracle.md`):
//! validated audio framing, the STT/TTS ports and descriptors, the error
//! taxonomy, the speech-egress policy and configuration contracts, the
//! host↔core event vocabulary, and in-memory fakes that exercise the
//! async/error/cancellation ports deterministically.
//!
//! Binding boundaries (enforced by `cargo xtask architecture` and the
//! crate's own tests):
//!
//! - Workspace dependencies are exactly `vesper-domain` and
//!   `vesper-security`. No runtime, agent, harness, or provider-adapter
//!   dependency exists here; hosts translate the existing runtime/provider
//!   surface into the voice-owned inputs in [`events`].
//! - No I/O, no clock, no process spawning, no device access, no vendor
//!   names. A fake is visibly a fake (`Fake*`) and carries fixture
//!   markers, so fake evidence cannot masquerade as adapter evidence.
//! - PR-0 deliberately contains **zero adapter implementations**: ports,
//!   contracts, and fakes only. Adapter phases (PR-1/PR-2) implement the
//!   real engines against these frozen contracts.

pub mod audio;
pub mod cancel;
pub mod composition;
pub mod config;
pub mod error;
pub mod events;
pub mod execution;
pub mod fakes;
pub mod hygiene;
pub mod ports;
pub mod report;
pub mod session;
pub mod test_util;

#[cfg(feature = "stt-http")]
pub mod stt_http;
#[cfg(feature = "stt-sidecar")]
pub mod stt_sidecar;
#[cfg(feature = "tts-subprocess")]
pub mod tts_subprocess;

pub use audio::{AudioFormat, PcmFrame, PcmReassembler};
pub use cancel::VoiceCancel;
pub use config::{
    CaptureBudget, SpeechEgress, VoiceProviderSelection, VoiceScope, VoiceScopeError,
    parse_voice_table, read_voice_scope,
};
pub use error::{TtsMidStreamError, VoiceError};
pub use events::{
    AgentActivity, AgentSettlement, DeviceFailureClass, HostEvent, PlayState, SpeakUnit,
    SpeechSegmentId, VoiceClientEvent, VoiceTurnPhase, project_phase,
};
pub use execution::{
    AcceleratorReadiness, OffloadPlacement, PlacementEvidence, ReadinessCache, ReadinessCacheKey,
    ReadinessInvalidation, SpeechStage, StageBackend, StageExecutionPolicy, StageResolution,
    StageRouteDecision,
};
pub use ports::{
    FailoverPolicy, PartialsKind, SpeechEgressClass, SttDescriptor, SttPartial, SttTranscript,
    TranscriptProvenance, TtsChunk, TtsDescriptor, TtsStream, VoiceFuture, VoiceProfile, VoiceStt,
    VoiceTts,
};
pub use report::{ReportMarker, VoiceTurnReport};
pub use session::{HostClock, SessionClock, SttFinal, VoiceEffect, VoiceSession};

/// PR-0 scope marker: this crate currently contains contracts and fakes
/// only. Adapter implementations belong to later PRs; nothing may cite
/// this crate as evidence that a provider is implemented.
pub const PR0_CONTRACTS_ONLY: bool = true;
