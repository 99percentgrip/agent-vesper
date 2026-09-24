//! Per-turn telemetry report (PRD §2.8, R8/R12).
//!
//! Metadata only, from an allowlist: stage timestamps, identities,
//! counts, class enums, and truthful markers. **No audio bytes and no
//! transcript text** — there is no field that could carry them, error
//! strings included. Stage durations use a caller-supplied instant so
//! the core stays clock-free (fakes inject deterministic stamps; hosts
//! inject real ones).

use serde::{Deserialize, Serialize};
use vesper_domain::{BoundedString, ProviderId};

use crate::events::{AgentSettlement, SpeechSegmentId};

/// Truthful marker describing what was skipped or bounded in speech.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReportMarker {
    /// A protected span was not spoken (e.g. a code block), with the
    /// spoken placeholder used.
    SkippedSpan {
        /// Span class label (reasoning/code/pem/…).
        class: BoundedString<32>,
    },
    /// Secret-shaped text was replaced before synthesis.
    Redacted,
    /// The pending-speech budget forced truncation of a unit.
    TruncatedByBudget,
    /// Capture ended mid-sample (unmatched byte).
    AudioTruncated,
    /// Playback progress was unavailable; "heard" claims were not made.
    PlaybackUnknown,
}

/// Exactly-one terminal report for an accepted voice turn (R8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceTurnReport {
    /// Sequential voice-turn number within the session (core-owned).
    pub turn_number: u64,
    /// Runtime turn identity, if the turn was dispatched before ending.
    pub runtime_turn: Option<BoundedString<256>>,
    /// STT provider that produced the final transcript.
    pub stt_provider: Option<ProviderId>,
    /// TTS provider used for synthesis.
    pub tts_provider: Option<ProviderId>,
    /// How the agent dimension settled.
    pub settlement: Option<AgentSettlement>,
    /// Whether the turn ended interrupted (barge-in/stop).
    pub interrupted: bool,
    /// Number of speech segments synthesized.
    pub segments: Vec<SpeechSegmentId>,
    /// Playback state per segment (acknowledged estimates only).
    pub playback: Vec<crate::events::PlayState>,
    /// Stage durations in milliseconds (present only when both endpoints
    /// were observed; absent stages stay absent, never fabricated).
    pub stage_ms: Vec<(BoundedString<32>, u64)>,
    /// Truthful markers for skipped/redacted/truncated speech.
    pub markers: Vec<ReportMarker>,
    /// Classified terminal error class, if the turn failed
    /// (infrastructure failures only; `NoSpeech` is a normal outcome).
    pub failure: Option<crate::error::VoiceError>,
}

impl VoiceTurnReport {
    /// An empty report the state machine fills in as stages complete.
    #[must_use]
    pub fn new(turn_number: u64) -> Self {
        Self {
            turn_number,
            runtime_turn: None,
            stt_provider: None,
            tts_provider: None,
            settlement: None,
            interrupted: false,
            segments: Vec::new(),
            playback: Vec::new(),
            stage_ms: Vec::new(),
            markers: Vec::new(),
            failure: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_carries_metadata_only() {
        let report = VoiceTurnReport::new(1);
        let encoded = serde_json::to_string(&report).unwrap();
        // R12 by construction: no transcript/audio field exists to leak.
        assert!(!encoded.contains("text\":"));
        assert!(!encoded.contains("audio\":"));
        assert!(report.failure.is_none());
        assert!(report.stage_ms.is_empty());
    }

    #[test]
    fn markers_serialize_with_class_labels() {
        let marker = ReportMarker::SkippedSpan {
            class: BoundedString::new("code").unwrap(),
        };
        let encoded = serde_json::to_string(&marker).unwrap();
        assert!(encoded.contains("code"));
    }
}
