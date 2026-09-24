//! VRO-17 R6 §2.4 binding repair: production-host tests for one-gesture
//! barge-in and explicit Stop.
//!
//! RED-first on the pre-repair binding:
//! - Speaking+F9 called `stop_speech()` and returned, so no session interrupt,
//!   runtime cancel, interruption note, or new capture occurred.
//! - Ctrl+C cancelled only the runtime and never entered the host Stop path.
//!
//! These tests use the same `ConversationHost::apply_conversation_gesture`
//! entry as production F9 and `apply_interruption_control` entry as production
//! Ctrl+C. Doubles exist only at the STT/player boundaries; no reducer is called
//! directly.

#![cfg(feature = "voice-conversation")]
#![forbid(unsafe_code)]

use std::sync::Arc;

use agent_vesper_tui::voice_conversation::{ConversationHost, GestureOutcome, InterruptionControl};
use vesper_voice::audio::PcmFrame;
use vesper_voice::cancel::VoiceCancel;
use vesper_voice::error::VoiceError;
use vesper_voice::events::{AgentSettlement, HostEvent, VoiceClientEvent, VoiceTurnPhase};
use vesper_voice::fakes::FakeStt;
use vesper_voice::ports::{SttTranscript, TranscriptProvenance, VoiceStt};
use vesper_voice::session::SttFinal;

/// STT double at the external boundary.
pub struct DoubleStt(FakeStt);

impl DoubleStt {
    pub fn new() -> Self {
        Self(FakeStt::on_device())
    }
}

impl Default for DoubleStt {
    fn default() -> Self {
        Self::new()
    }
}

impl VoiceStt for DoubleStt {
    fn transcribe<'a>(
        &'a self,
        audio: &'a [PcmFrame],
        cancel: &'a VoiceCancel,
    ) -> vesper_voice::ports::VoiceFuture<'a, Result<SttTranscript, VoiceError>> {
        self.0.transcribe(audio, cancel)
    }

    fn descriptor(&self) -> &vesper_voice::ports::SttDescriptor {
        self.0.descriptor()
    }
}

fn host() -> ConversationHost<DoubleStt> {
    ConversationHost::new(Arc::new(DoubleStt::new()), None)
}

fn transcript(text: &str) -> SttFinal {
    SttFinal::Transcript(SttTranscript {
        text: vesper_domain::BoundedString::new(text.to_owned()).unwrap(),
        provider: vesper_domain::ProviderId::new("double").unwrap(),
        confidence: None,
        provenance: TranscriptProvenance::InferredText,
    })
}

/// Complete the currently-open capture and drive the ordinary agent-output
/// bridge until the production host is in Speaking.
fn captured_turn_to_speaking(host: &mut ConversationHost<DoubleStt>, turn: usize) -> Vec<String> {
    let (outcome, _) = host.gesture(HostEvent::CaptureStopped);
    assert!(matches!(outcome, GestureOutcome::Accepted));
    let (_, submitted) = host.deliver_final(transcript(&format!("question {turn}")));
    assert_eq!(submitted.len(), 1, "exactly one ordinary submission");
    host.runtime_identity(&format!("run-{turn}"));
    let (units, duplicate_submissions) = host.assistant_delta("Rust ownership prevents bugs.");
    assert!(!units.is_empty(), "assistant text must dispatch speech");
    assert!(
        duplicate_submissions.is_empty(),
        "speech never resubmits input"
    );
    // The production event surface maps this Speak unit to its host-level
    // `ConversationPhase::Speaking`; the core projection remains `Working`
    // while the runtime turn is also active because dimensions overlap.
    submitted
}

fn begin_speaking_turn(host: &mut ConversationHost<DoubleStt>, turn: usize) {
    let (outcome, _) = host.gesture(HostEvent::CaptureStarted);
    assert!(matches!(outcome, GestureOutcome::Accepted));
    let _ = captured_turn_to_speaking(host, turn);
}

/// TEST 1 — one production conversation gesture performs the complete §2.4
/// barge-in transition. The replacement capture later submits exactly once and
/// carries at most one bounded interruption note.
#[test]
fn speaking_f9_is_one_gesture_genuine_barge_in() {
    let mut host = host();
    begin_speaking_turn(&mut host, 1);
    let generation_before = host.speech_state().1;

    // Exact one-call entry used by the production F9 handler: host-visible
    // Speaking with no active recorder.
    let result = host.apply_conversation_gesture(true, false);

    assert!(matches!(result.outcome, GestureOutcome::Accepted));
    assert_eq!(
        result
            .events
            .iter()
            .filter(|event| matches!(event, VoiceClientEvent::Interrupted { .. }))
            .count(),
        1,
        "exactly one interruption transition"
    );
    assert_eq!(result.runtime_cancels.len(), 1, "one runtime cancel");
    assert!(result.capture_started, "the same gesture opens capture");
    assert!(host.session_is_capturing());
    assert!(host.speech_stopped_for_old_generation());
    assert_ne!(
        host.speech_state().1,
        generation_before,
        "generation bump makes old synthesis stale"
    );

    let submitted = captured_turn_to_speaking(&mut host, 2);
    assert_eq!(submitted.len(), 1, "no duplicate replacement turn");
    assert_eq!(
        submitted[0].matches("[context:").count(),
        1,
        "bounded interruption context is staged exactly once"
    );
}

/// TEST 2 — explicit Stop executes stop/cancel without opening capture, and a
/// later manually initiated F9 capture remains available after settlement.
#[test]
fn explicit_stop_stops_everything_and_opens_no_capture() {
    let mut host = host();
    begin_speaking_turn(&mut host, 1);
    let generation_before = host.speech_state().1;

    let result = host.apply_interruption_control(InterruptionControl::Stop);

    assert!(matches!(result.outcome, GestureOutcome::Accepted));
    assert_eq!(
        result
            .events
            .iter()
            .filter(|event| matches!(event, VoiceClientEvent::Interrupted { .. }))
            .count(),
        1
    );
    assert_eq!(result.runtime_cancels.len(), 1);
    assert!(!result.capture_started, "Stop must not start capture");
    assert!(!host.session_is_capturing());
    assert!(host.speech_stopped_for_old_generation());
    assert_ne!(host.speech_state().1, generation_before);

    let _ = host.runtime_settled(AgentSettlement::Cancelled);
    let (outcome, _) = host.gesture(HostEvent::CaptureStarted);
    assert!(matches!(outcome, GestureOutcome::Accepted));
    assert!(host.session_is_capturing(), "later manual F9 still works");
}

/// TEST 3 — repeated one-gesture barge-ins do not latch or duplicate effects.
#[test]
fn repeated_barge_in_cycles_recover() {
    let mut host = host();
    begin_speaking_turn(&mut host, 1);

    for cycle in 1..=3 {
        let result = host.apply_conversation_gesture(true, false);
        assert!(
            matches!(result.outcome, GestureOutcome::Accepted),
            "cycle {cycle}"
        );
        assert_eq!(result.runtime_cancels.len(), 1, "cycle {cycle}: one cancel");
        assert_eq!(
            result
                .events
                .iter()
                .filter(|event| matches!(event, VoiceClientEvent::Interrupted { .. }))
                .count(),
            1,
            "cycle {cycle}: one interruption"
        );
        assert!(result.capture_started, "cycle {cycle}: recaptured");
        let submitted = captured_turn_to_speaking(&mut host, cycle + 1);
        assert_eq!(submitted.len(), 1, "cycle {cycle}: no replay");
    }
}

/// TEST 4 — the host Stop control is not invoked for a non-voice gesture. The
/// generic Ctrl+C regression itself remains in the binary unit suite; this
/// assertion pins that an untouched host has no cancel/capture side effects.
#[test]
fn non_voice_ctrl_c_leaves_voice_host_untouched() {
    let host = host();
    assert_eq!(host.session_phase(), VoiceTurnPhase::Idle);
    assert!(!host.session_is_capturing());
    assert!(!host.speech_stopped_for_old_generation());
}
