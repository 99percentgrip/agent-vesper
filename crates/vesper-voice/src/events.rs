//! Host↔core event vocabulary and turn dimensions (PRD §2.3, decisions D4/D5).
//!
//! **All agent/provider types stay in the host.** The host translates the
//! existing runtime surface (submit/cancel commands, content/tool/terminal
//! events) into the voice-owned inputs below; `vesper-voice` neither
//! imports nor names those types. Tool and approval *status* may be
//! forwarded for display only — the voice surface never approves a tool,
//! never submits an interim transcript as a command, and never holds
//! reasoning content.

use serde::{Deserialize, Serialize};
use vesper_domain::{BoundedString, TurnId};

use crate::audio::PcmFrame;
use crate::error::VoiceError;

/// Monotone identity of one speech segment within a turn.
///
/// Correlation stamp shared by synthesis requests, audio frames, and
/// playback acknowledgments so stale output can never attach to a newer
/// turn or segment (PRD §2.3).
pub type SpeechSegmentId = u64;

/// Agent-side activity forwarded for display only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentActivity {
    /// Turn dispatched; no visible output yet.
    Thinking,
    /// A tool call is running. `preview` is the host-approved summary
    /// (never raw arguments).
    Tool {
        /// Tool name.
        name: BoundedString<128>,
        /// Host-approved preview text.
        preview: BoundedString<256>,
    },
    /// Assistant text is being spoken.
    Speaking,
}

/// Voice-owned translation of the runtime's terminal outcome for one
/// accepted turn (the host maps `FinishOutcome`/`TurnCancelled` onto this;
/// the core never sees the runtime type).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentSettlement {
    /// Agent turn completed normally.
    Completed,
    /// Agent turn failed.
    Failed,
    /// Agent turn was cancelled (including by our own barge-in/stop).
    Cancelled,
    /// Agent stream ended after visible output with an unresolved
    /// tool-call fragment — the no-replay boundary; the runtime governs
    /// any continuation, never the voice layer.
    InterruptedNoReplay,
}

/// Host-observed playback state for one speech segment (D5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayState {
    /// Audio is queued/playing; no acknowledgment yet.
    Draining,
    /// The host acknowledged audio through a byte offset reached the
    /// output device. An **estimate of device handoff**, not proof the
    /// user heard it.
    Acked {
        /// Acknowledged byte offset within the segment.
        through_bytes: u64,
    },
    /// Playback stopped (barge-in/stop/flush).
    Stopped,
    /// The host cannot observe playback; no acks will arrive. Any
    /// "heard" claim derived from synthesis completion alone is a defect.
    Unknown,
}

/// Everything the host feeds into the core (complete v1 surface).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    /// User began capture (explicit; v1 has no auto-capture).
    CaptureStarted,
    /// One validated capture frame.
    CapturedAudio(PcmFrame),
    /// User ended capture; STT may finalize.
    CaptureStopped,
    /// Host-approved **user-visible assistant text** chunk. The host
    /// selects the visible-content channel before this call; reasoning
    /// and tool channels never reach here, and hygiene's stripping is
    /// defense-in-depth, not channel selection.
    AgentText {
        /// Approved visible text chunk.
        text: BoundedString<4096>,
    },
    /// The runtime assigned a turn identity. Absence before this event
    /// is legal: cancellation requested earlier is recorded and applied
    /// when identity arrives (or the turn settles without us).
    AgentTurnIdentity {
        /// Runtime turn identity.
        turn_id: TurnId,
    },
    /// The runtime turn settled terminally.
    AgentTurnSettled {
        /// Settlement classification.
        outcome: AgentSettlement,
    },
    /// Host acknowledgment that audio through a byte offset of a segment
    /// was handed to the output device (estimate, not heard-proof).
    PlaybackAck {
        /// Segment being played.
        segment: SpeechSegmentId,
        /// Acknowledged byte offset.
        through_bytes: u64,
    },
    /// The host cannot observe playback for this turn.
    PlaybackUnavailable,
    /// Capture/playback device or surfaced provider failure.
    DeviceOrProviderFailure {
        /// Failure class.
        class: DeviceFailureClass,
    },
    /// The user began speaking during playback (barge-in trigger).
    BargeIn,
    /// Explicit stop request (STOP control/command).
    StopRequested,
}

/// Classified device/provider failure surfaced by the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceFailureClass {
    /// Microphone capture failed.
    Capture,
    /// Output device failed.
    Playback,
    /// A provider failed (metadata only; no content).
    Provider(VoiceError),
}

/// A hygiene-passed unit the core asks the host to speak.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeakUnit {
    /// Segment this unit belongs to.
    pub segment: SpeechSegmentId,
    /// Hygiene-passed text. Fully validated: protected spans closed,
    /// secrets redacted, budget respected.
    pub text: BoundedString<8192>,
    /// Truthful markers for what was skipped/redacted/truncated while
    /// producing this unit.
    pub markers: Vec<BoundedString<64>>,
}

/// Everything the core emits to the host (complete v1 surface).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceClientEvent {
    /// Interim capture transcription (display only; never a command).
    PartialTranscript {
        /// Interim text.
        text: BoundedString<2048>,
    },
    /// Final transcription for the accepted capture.
    Transcript {
        /// Final text.
        text: BoundedString<8192>,
    },
    /// Agent activity forwarded for display only.
    AgentActivity(AgentActivity),
    /// Speak this unit (host drives TTS through the core pipeline in
    /// PR-3, or directly at its boundary).
    Speak(SpeakUnit),
    /// Synthesized audio for a segment.
    SpeechAudio {
        /// Segment identity.
        segment: SpeechSegmentId,
        /// Audio frame.
        frame: PcmFrame,
    },
    /// Stop playback now (flush the queue).
    PlaybackStop,
    /// The active turn was interrupted; `heard_through` reflects
    /// **acknowledged playback only** and may be absent/unknown.
    Interrupted {
        /// Last acknowledged playback position, when known.
        heard_through: Option<BoundedString<2048>>,
    },
    /// Exactly-once terminal outcome for an accepted voice turn.
    TurnDone(crate::report::VoiceTurnReport),
    /// Infrastructure failure (never used for `NoSpeech` outcomes).
    Failed(VoiceError),
}

/// UI projection of the concurrent dimensions (PRD §2.3).
///
/// A projection, not the state: generation and playback overlap, so the
/// underlying model keeps independent dimensions and this enum is
/// derived for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoiceTurnPhase {
    /// Awaiting user capture start.
    Idle,
    /// Capturing audio.
    Capturing,
    /// Capture ended; transcription running.
    Transcribing,
    /// Turn dispatched; no visible output yet.
    Thinking,
    /// A tool call is running.
    Working,
    /// Assistant audio is being produced/played.
    Speaking,
    /// The turn was interrupted; cleanup may still be settling.
    Interrupted,
    /// Terminal (report already emitted).
    Done,
}

/// Projects the concurrent dimension states into a UI phase.
///
/// Precedence (first match wins): capture > STT > interruption pending >
/// tool activity > speaking > thinking > idle. `settled` marks the agent
/// dimension terminal; speaking may continue afterwards while audio
/// drains (D4: dimensions are independent).
#[must_use]
pub fn project_phase(
    capturing: bool,
    transcribing: bool,
    interruption_pending: bool,
    tool_active: bool,
    speaking: bool,
    settled: bool,
) -> VoiceTurnPhase {
    if capturing {
        return VoiceTurnPhase::Capturing;
    }
    if transcribing {
        return VoiceTurnPhase::Transcribing;
    }
    if interruption_pending {
        return VoiceTurnPhase::Interrupted;
    }
    if tool_active {
        return VoiceTurnPhase::Working;
    }
    if speaking {
        return VoiceTurnPhase::Speaking;
    }
    if !settled {
        return VoiceTurnPhase::Thinking;
    }
    if settled && !speaking {
        return VoiceTurnPhase::Done;
    }
    VoiceTurnPhase::Idle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_dominates_projection() {
        assert_eq!(
            project_phase(true, true, true, true, true, false),
            VoiceTurnPhase::Capturing
        );
    }

    #[test]
    fn speaking_survives_agent_settlement() {
        assert_eq!(
            project_phase(false, false, false, false, true, true),
            VoiceTurnPhase::Speaking
        );
    }

    #[test]
    fn settled_without_speech_is_done() {
        assert_eq!(
            project_phase(false, false, false, false, false, true),
            VoiceTurnPhase::Done
        );
    }

    #[test]
    fn interruption_outranks_tools_and_speech() {
        assert_eq!(
            project_phase(false, false, true, true, true, false),
            VoiceTurnPhase::Interrupted
        );
    }

    #[test]
    fn transcribing_outranks_interruption_pending() {
        // An interrupt during STT cancels STT; until it settles the
        // projection shows the still-running work.
        assert_eq!(
            project_phase(false, true, true, false, false, false),
            VoiceTurnPhase::Transcribing
        );
    }

    #[test]
    fn default_idle() {
        assert_eq!(
            project_phase(false, false, false, false, false, false),
            VoiceTurnPhase::Thinking
        );
    }
}
