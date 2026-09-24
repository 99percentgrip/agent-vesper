//! Error taxonomy (PRD §2.2, decisions D6/D8).
//!
//! The voice oracle's "unavailable ≠ silence" discipline, made typed and
//! exhaustive for PR-0. Two rules are binding:
//!
//! - `NoSpeech` is a **nonfatal outcome**, not a provider fault: VAD
//!   found no speech, the turn ends politely, and no retry chain runs.
//!   It never advances a failover chain (silence is an answer).
//! - No arbitrary engine failure may be converted into `NoSpeech`.
//!   Engine failures map to `Inference`/`Unavailable` and stay visible;
//!   the reference's exception-to-empty-transcript behavior is
//!   intentionally not inherited (recon §13 "refuse" list).

use serde::{Deserialize, Serialize};
use vesper_domain::{BoundedString, ProviderId};

/// Classified voice failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
pub enum VoiceError {
    /// Provider not reachable or not configured. Failover-eligible:
    /// a failover policy may advance to the next adapter.
    #[error("voice provider {provider} unavailable: {reason}")]
    Unavailable {
        /// Provider identity.
        provider: ProviderId,
        /// Operational reason (metadata only; no transcript/audio content).
        reason: BoundedString<256>,
    },
    /// VAD found no speech. **Nonfatal outcome**, not an error turn and
    /// not failover-eligible: retrying a different provider cannot
    /// produce speech that was not captured.
    #[error("no speech detected in capture")]
    NoSpeech,
    /// Provider rejected credentials.
    #[error("voice provider {provider} rejected authentication")]
    Auth {
        /// Provider identity.
        provider: ProviderId,
    },
    /// Provider account quota exhausted.
    #[error("voice provider {provider} quota exhausted")]
    Quota {
        /// Provider identity.
        provider: ProviderId,
    },
    /// Caller requested stop. Not an error; carried in the taxonomy so
    /// terminal flows classify uniformly.
    #[error("voice operation cancelled")]
    Cancelled,
    /// Malformed input (parameters, formats). Never failover-eligible.
    /// Owned label so the taxonomy stays serializable.
    #[error("invalid voice input: {0}")]
    InvalidInput(String),
    /// Capture ended with an unmatched sample byte; audio was truncated.
    #[error("audio stream truncated mid-sample")]
    Truncated,
    /// A bounded queue or budget was exhausted.
    #[error("voice resource exhausted: {0}")]
    ResourceExhausted(String),
    /// The inference engine itself failed on valid input.
    #[error("voice inference failed: {0}")]
    Inference(String),
}

impl VoiceError {
    /// Whether a failover policy may advance to the next adapter after
    /// this error. `Unavailable` advances; `NoSpeech`/`Auth`/`Quota`/
    /// `InvalidInput`/`Truncated` do not (silence is an answer, account
    /// state is not an outage, bad input is not the next provider's
    /// problem, truncation is a capture defect).
    #[must_use]
    pub fn failover_eligible(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }
}

/// Failure of a TTS stream **after** audio has begun (PRD §2.2, D6).
///
/// Distinct from a clean [`super::TtsChunk::Finished`] and from an
/// open-time [`VoiceError`]: the host has already played part of the
/// segment and must surface the gap honestly instead of treating the
/// segment as complete or silently retrying it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
pub enum TtsMidStreamError {
    /// Audio began, then the provider failed before finishing.
    #[error("tts audio stream failed after beginning")]
    AudioFailed {
        /// Operational reason (metadata only).
        reason: BoundedString<256>,
    },
    /// Synthesis was cancelled mid-stream (barge-in/stop).
    #[error("tts audio stream cancelled")]
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> ProviderId {
        ProviderId::new("fake-stt").unwrap()
    }

    #[test]
    fn only_unavailable_is_failover_eligible() {
        let unavailable = VoiceError::Unavailable {
            provider: provider(),
            reason: BoundedString::new("offline").unwrap(),
        };
        assert!(unavailable.failover_eligible());
        for error in [
            VoiceError::NoSpeech,
            VoiceError::Auth {
                provider: provider(),
            },
            VoiceError::Quota {
                provider: provider(),
            },
            VoiceError::Cancelled,
            VoiceError::InvalidInput("x".into()),
            VoiceError::Truncated,
            VoiceError::ResourceExhausted("x".into()),
            VoiceError::Inference("x".into()),
        ] {
            assert!(!error.failover_eligible(), "{error:?}");
        }
    }

    #[test]
    fn errors_carry_no_content_by_construction() {
        // The taxonomy's payloads are identifiers, bounded reasons, and
        // static labels; there is no field that could carry transcript or
        // audio bytes (R12 by construction).
        let error = VoiceError::Unavailable {
            provider: provider(),
            reason: BoundedString::new("connection refused").unwrap(),
        };
        let encoded = serde_json::to_string(&error).unwrap();
        assert!(!encoded.contains("transcript"));
        assert!(!encoded.contains("audio"));
    }
}
