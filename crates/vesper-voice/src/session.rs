//! VoiceSession orchestration (VRO-17 PR-3).
//!
//! A **reducer-style state machine** over [`HostEvent`]s producing typed
//! [`VoiceEffect`]s and [`VoiceClientEvent`]s — following the
//! repository's state/effect ownership pattern (the host executes all
//! runtime/device effects; the core owns decisions and correlation).
//! There is no generic event bus and no I/O here: pure transitions,
//! bounded state, deterministic tests.
//!
//! Ownership (frozen):
//! - The core **never** imports runtime/provider types; hosts translate
//!   at their boundary (PR-0 D1).
//! - The core never dispatches tools, approves anything, owns history,
//!   or replays operations. It requests exactly one runtime turn per
//!   accepted voice turn through [`VoiceEffect::SubmitTurn`], and one
//!   cancellation through [`VoiceEffect::CancelRuntimeTurn`] — both
//!   executed by the host under the existing transactional contract.
//! - Speech work runs through the PR-2 hygiene gate **exactly once**;
//!   STT finals through the injected [`VoiceStt`] port with PR-1
//!   provenance/failover intact; synthesis is *requested* via effects
//!   (the host drives the TTS port in PR-4 wiring; the doubles here
//!   record and acknowledge).
//!
//! Five concurrent dimensions (PR-0 D4), never an exclusive phase
//! sequence: capture, STT, agent, synthesis, playback. The projected
//! [`VoiceTurnPhase`] is derived, not stored.
//!
//! Timing: an injected [`SessionClock`] (metadata-only, bounded;
//! unobserved stages stay absent).

use std::sync::Arc;

use vesper_domain::{BoundedString, TurnId};

use crate::audio::PcmFrame;
use crate::config::CaptureBudget;
use crate::error::VoiceError;
use crate::events::{
    AgentActivity, AgentSettlement, DeviceFailureClass, HostEvent, PlayState, SpeakUnit,
    SpeechSegmentId, VoiceClientEvent, VoiceTurnPhase, project_phase,
};
use crate::hygiene::{GatedSentence, HygieneError, HygieneGate, HygieneMarker};
use crate::ports::{SttTranscript, VoiceStt};
use crate::report::{ReportMarker, VoiceTurnReport};

/// Effects the core requests; the host executes and acknowledges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceEffect {
    /// Submit exactly one normal runtime turn with this input. The host
    /// uses the existing submit contract (request identity/idempotency
    /// are the host's; the core never resubmits).
    SubmitTurn {
        /// Voice-turn correlation number.
        turn: u64,
        /// Finalized user input (possibly with the bounded interruption
        /// note as ordinary content, clearly labeled).
        input: BoundedString<8192>,
    },
    /// Cancel the active runtime turn (established transactional path).
    CancelRuntimeTurn {
        /// Turn the cancellation targets.
        turn: u64,
    },
    /// Begin synthesis for a speech unit (host drives the TTS port).
    SynthesizeUnit {
        /// Unit identity.
        segment: SpeechSegmentId,
        /// Hygiene-passed text.
        text: BoundedString<8192>,
    },
    /// Cancel in-flight synthesis for these segments (urgent: never
    /// queued behind other effects).
    CancelSynthesis { segments: Vec<SpeechSegmentId> },
    /// Stop and flush playback now (urgent).
    StopPlayback,
}

/// Clock abstraction for report ordering (a fake proves ordering, not
/// device latency — never reported as one).
pub trait SessionClock: Send + Sync {
    /// Monotonic milliseconds for stage stamps.
    fn now_ms(&self) -> u64;
}

/// Wall-ish clock for hosts.
#[derive(Debug, Clone, Copy, Default)]
pub struct HostClock;

impl SessionClock for HostClock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// How STT finals reach the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SttFinal {
    /// A final transcript from the injected port.
    Transcript(SttTranscript),
    /// The port failed before submission (no runtime turn).
    Failed(VoiceError),
}

/// One dimension-set for a voice turn.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TurnState {
    number: u64,
    // capture
    capturing: bool,
    // stt
    stt_running: bool,
    stt_final: Option<SttFinal>,
    // agent
    runtime_turn: Option<TurnId>,
    submitted: bool,
    settlement: Option<AgentSettlement>,
    cancel_requested: bool,
    /// Cancellation intent recorded before identity assignment.
    cancel_before_identity: bool,
    // synthesis/playback
    units: Vec<UnitRecord>,
    // report
    interrupted: bool,
    start_ms: u64,
    stages: Vec<(BoundedString<32>, u64)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UnitRecord {
    segment: SpeechSegmentId,
    text: BoundedString<8192>,
    play: PlayState,
    /// Sanitized text retained for the interruption note (hygiene-passed).
    acked_bytes: u64,
}

impl UnitRecord {
    fn completed(&self) -> bool {
        matches!(self.play, PlayState::Acked { through_bytes } if through_bytes >= self.text_len() as u64)
    }
    fn text_len(&self) -> usize {
        self.text.as_str().len()
    }
}

/// Bounded voice session orchestrator.
pub struct VoiceSession<S: VoiceStt + ?Sized> {
    stt: Arc<S>,
    budget: CaptureBudget,
    /// Turn counter (accepted turns).
    turns: u64,
    /// Active turn, if any.
    active: Option<TurnState>,
    /// Pending input awaiting settlement (at most one).
    pending_next: Option<BoundedString<8192>>,
    /// Interruption note staged for the next submitted turn.
    staged_note: Option<BoundedString<2048>>,
    /// Hygiene gate for the active turn's assistant stream.
    gate: Option<HygieneGate>,
    /// Next speech-unit id (global monotone).
    next_segment: SpeechSegmentId,
    /// Captured frames for the active turn (bounded).
    capture: Vec<PcmFrame>,
    /// Bounded event-log of recent client events (diagnostics cap).
    max_events: usize,
}

impl<S: VoiceStt + ?Sized> VoiceSession<S> {
    /// Creates a session over the given STT port.
    #[must_use]
    pub fn new(stt: Arc<S>, budget: CaptureBudget) -> Self {
        Self {
            stt,
            budget,
            turns: 0,
            active: None,
            pending_next: None,
            staged_note: None,
            gate: None,
            next_segment: 0,
            capture: Vec::new(),
            max_events: 256,
        }
    }

    /// Projects the current phase for UI (derived, not stored).
    #[must_use]
    pub fn phase(&self) -> VoiceTurnPhase {
        let Some(turn) = &self.active else {
            return if self.pending_next.is_some() {
                VoiceTurnPhase::Interrupted
            } else {
                VoiceTurnPhase::Idle
            };
        };
        let speaking = turn
            .units
            .iter()
            .any(|unit| matches!(unit.play, PlayState::Draining | PlayState::Acked { .. }))
            && !turn
                .units
                .iter()
                .all(|unit| unit.play == PlayState::Stopped);
        project_phase(
            turn.capturing,
            turn.stt_running,
            turn.cancel_requested,
            turn.settlement.is_none() && turn.submitted,
            speaking,
            turn.settlement.is_some(),
        )
    }

    /// Handles one host event: returns effects to execute and client
    /// events to surface. Pure; the host remains the only executor.
    pub fn handle(
        &mut self,
        event: HostEvent,
        clock: &dyn SessionClock,
    ) -> (Vec<VoiceEffect>, Vec<VoiceClientEvent>) {
        match event {
            HostEvent::CaptureStarted => self.on_capture_started(clock),
            HostEvent::CapturedAudio(frame) => self.on_captured_audio(frame),
            HostEvent::CaptureStopped => self.on_capture_stopped(clock),
            HostEvent::AgentText { text } => self.on_agent_text(text),
            HostEvent::AgentTurnIdentity { turn_id } => self.on_identity(turn_id, clock),
            HostEvent::AgentTurnSettled { outcome } => self.on_settled(outcome, clock),
            HostEvent::PlaybackAck {
                segment,
                through_bytes,
            } => self.on_playback_ack(segment, through_bytes),
            HostEvent::PlaybackUnavailable => self.on_playback_unavailable(),
            HostEvent::DeviceOrProviderFailure { class } => self.on_failure(class, clock),
            HostEvent::BargeIn => self.on_interrupt(true, clock),
            HostEvent::StopRequested => self.on_interrupt(false, clock),
        }
    }

    fn on_capture_started(
        &mut self,
        clock: &dyn SessionClock,
    ) -> (Vec<VoiceEffect>, Vec<VoiceClientEvent>) {
        let mut effects = Vec::new();
        let mut events = Vec::new();
        // Barge-in semantics: starting capture during an active,
        // not-yet-interrupted turn is an interruption (urgent playback
        // stop first). An ALREADY interrupted turn awaiting runtime
        // settlement takes at most one bounded pending input with a
        // visible rejection; further input is rejected, never queued
        // unboundedly or silently lost.
        if let Some(active) = &self.active {
            let interrupted_awaiting_settlement = active.interrupted && active.settlement.is_none();
            if interrupted_awaiting_settlement {
                if self.pending_next.is_some() {
                    events.push(VoiceClientEvent::Failed(VoiceError::ResourceExhausted(
                        "input rejected: previous input still awaiting settlement".into(),
                    )));
                    // No new turn starts: the request is refused loudly.
                    return (effects, events);
                }
                self.pending_next = Some(BoundedString::new("").expect("fits"));
                events.push(VoiceClientEvent::Failed(VoiceError::ResourceExhausted(
                    "input queued (bounded): awaiting settlement of the interrupted turn".into(),
                )));
                return (effects, events);
            }
            if active.settlement.is_none() {
                let (interrupt_effects, interrupt_events) = self.on_interrupt(true, clock);
                effects.extend(interrupt_effects);
                events.extend(interrupt_events);
            }
        }
        self.turns += 1;
        self.active = Some(TurnState {
            number: self.turns,
            capturing: true,
            stt_running: false,
            stt_final: None,
            runtime_turn: None,
            submitted: false,
            settlement: None,
            cancel_requested: false,
            cancel_before_identity: false,
            units: Vec::new(),
            interrupted: false,
            start_ms: clock.now_ms(),
            stages: Vec::new(),
        });
        self.capture.clear();
        self.gate = None;
        self.next_segment = 0;
        (effects, events)
    }

    fn on_captured_audio(&mut self, frame: PcmFrame) -> (Vec<VoiceEffect>, Vec<VoiceClientEvent>) {
        let used: usize = self.capture.iter().map(|f| f.bytes().len()).sum();
        if used + frame.bytes().len() > self.budget.max_capture_bytes as usize {
            // Loud, bounded: capture stops accepting; the user is told.
            return (
                Vec::new(),
                vec![VoiceClientEvent::Failed(VoiceError::ResourceExhausted(
                    "capture budget exceeded; stop capture and retry shorter".into(),
                ))],
            );
        }
        self.capture.push(frame);
        (Vec::new(), Vec::new())
    }

    fn on_capture_stopped(
        &mut self,
        clock: &dyn SessionClock,
    ) -> (Vec<VoiceEffect>, Vec<VoiceClientEvent>) {
        let Some(turn) = self.active.as_mut() else {
            return (
                Vec::new(),
                vec![VoiceClientEvent::Failed(VoiceError::InvalidInput(
                    "capture stopped without an active turn".into(),
                ))],
            );
        };
        turn.capturing = false;
        turn.stt_running = true;
        turn.stages.push((
            BoundedString::new("capture_end").expect("fits"),
            clock.now_ms(),
        ));
        // Final transcription runs through the injected port (blocking
        // work is the host's; here we drive it deterministically via the
        // test-side wrapper — production hosts call `finalize_with`).
        (Vec::new(), Vec::new())
    }

    /// Supplies the STT final for the active turn (host calls this after
    /// driving the port; the session never blocks on inference here).
    pub fn submit_stt_final(
        &mut self,
        final_result: SttFinal,
        clock: &dyn SessionClock,
    ) -> (Vec<VoiceEffect>, Vec<VoiceClientEvent>) {
        let Some(turn) = self.active.as_mut() else {
            return (Vec::new(), Vec::new());
        };
        turn.stt_running = false;
        turn.stages.push((
            BoundedString::new("stt_final").expect("fits"),
            clock.now_ms(),
        ));
        match final_result {
            SttFinal::Failed(error) => {
                turn.stt_final = Some(SttFinal::Failed(error.clone()));
                // No runtime turn is created: terminal outcome now.
                let report = self.close_turn(clock, |turn| {
                    turn.settlement = None;
                });
                let events = vec![
                    VoiceClientEvent::Failed(error),
                    VoiceClientEvent::TurnDone(report),
                ];
                self.active = None;
                self.capture.clear();
                #[allow(clippy::needless_return)]
                return (Vec::new(), events);
            }
            SttFinal::Transcript(transcript) => {
                turn.stt_final = Some(SttFinal::Transcript(transcript.clone()));
                let text = transcript.text.clone();
                let provenance = transcript.provenance;
                let mut events = vec![VoiceClientEvent::Transcript { text: text.clone() }];
                // Provenance semantics (PR-1 D21): legacy empties and
                // VAD silence do not submit a turn; inferred text does.
                let submit = match provenance {
                    crate::ports::TranscriptProvenance::InferredText => {
                        !text.as_str().trim().is_empty()
                    }
                    crate::ports::TranscriptProvenance::VadConfirmedSilence
                    | crate::ports::TranscriptProvenance::LegacyEmptyResponse => false,
                };
                if !submit {
                    let report = self.close_turn(clock, |turn| {
                        turn.settlement = None;
                    });
                    events.push(VoiceClientEvent::TurnDone(report));
                    self.active = None;
                    self.capture.clear();
                    return (Vec::new(), events);
                }
                // Compose input: interruption note (once) + transcript.
                let mut input = String::new();
                if let Some(note) = self.staged_note.take() {
                    input.push_str(note.as_str());
                    input.push('\n');
                }
                input.push_str(text.as_str());
                let input = BoundedString::new(input)
                    .unwrap_or_else(|_| BoundedString::new("input exceeded bound").expect("fits"));
                turn.submitted = true;
                let effect = VoiceEffect::SubmitTurn {
                    turn: turn.number,
                    input,
                };
                self.gate = Some(HygieneGate::new(self.budget));
                (vec![effect], events)
            }
        }
    }

    fn on_agent_text(
        &mut self,
        text: BoundedString<4096>,
    ) -> (Vec<VoiceEffect>, Vec<VoiceClientEvent>) {
        let Some(gate) = self.gate.as_mut() else {
            // Text without a submitted turn: not accepted, no fabricate.
            return (
                Vec::new(),
                vec![VoiceClientEvent::Failed(VoiceError::InvalidInput(
                    "assistant text without an accepted turn".into(),
                ))],
            );
        };
        match gate.push(text.as_str()) {
            Ok(units) => self.emit_units(units),
            Err(HygieneError::BudgetExceeded { .. }) => (
                Vec::new(),
                vec![VoiceClientEvent::Failed(VoiceError::ResourceExhausted(
                    "pending speech budget exceeded; units flushed at last validated boundary"
                        .into(),
                ))],
            ),
            Err(HygieneError::Finalized) => (
                Vec::new(),
                vec![VoiceClientEvent::Failed(VoiceError::InvalidInput(
                    "text after finalization".into(),
                ))],
            ),
        }
    }

    fn emit_units(
        &mut self,
        units: Vec<GatedSentence>,
    ) -> (Vec<VoiceEffect>, Vec<VoiceClientEvent>) {
        let mut effects = Vec::new();
        let mut events = Vec::new();
        if let Some(turn) = self.active.as_mut() {
            for unit in units {
                let segment = self.next_segment;
                self.next_segment += 1;
                let speak = SpeakUnit {
                    segment,
                    text: unit.text.clone(),
                    markers: unit
                        .markers
                        .iter()
                        .map(|marker| match marker {
                            HygieneMarker::SkippedSpan { class } => {
                                BoundedString::new(format!("skipped:{class}")).expect("fits")
                            }
                            HygieneMarker::Redacted => {
                                BoundedString::new("redacted").expect("fits")
                            }
                            HygieneMarker::TruncatedByBudget => {
                                BoundedString::new("truncated:pending-budget").expect("fits")
                            }
                        })
                        .collect(),
                };
                turn.units.push(UnitRecord {
                    segment,
                    text: unit.text.clone(),
                    play: PlayState::Draining,
                    acked_bytes: 0,
                });
                effects.push(VoiceEffect::SynthesizeUnit {
                    segment,
                    text: speak.text.clone(),
                });
                events.push(VoiceClientEvent::Speak(speak));
            }
        }
        (effects, events)
    }

    /// Flushes the hygiene gate's validated tail (host calls at a
    /// legitimate message/turn boundary).
    pub fn flush_speech(
        &mut self,
        clock: &dyn SessionClock,
    ) -> (Vec<VoiceEffect>, Vec<VoiceClientEvent>) {
        let _ = clock;
        let Some(gate) = self.gate.as_mut() else {
            return (Vec::new(), Vec::new());
        };
        match gate.finalize() {
            Ok(units) => self.emit_units(units),
            Err(HygieneError::Finalized) => (Vec::new(), Vec::new()),
            Err(_) => (
                Vec::new(),
                vec![VoiceClientEvent::Failed(VoiceError::ResourceExhausted(
                    "pending speech budget exceeded at flush".into(),
                ))],
            ),
        }
    }

    fn on_identity(
        &mut self,
        turn_id: TurnId,
        _clock: &dyn SessionClock,
    ) -> (Vec<VoiceEffect>, Vec<VoiceClientEvent>) {
        let mut effects = Vec::new();
        if let Some(turn) = self.active.as_mut()
            && turn.runtime_turn.is_none()
            && turn.submitted
        {
            turn.runtime_turn = Some(turn_id.clone());
            if turn.cancel_before_identity {
                // Cancellation was requested before identity: apply now
                // to the matching run (never a later one).
                turn.cancel_before_identity = false;
                turn.cancel_requested = true;
                effects.push(VoiceEffect::CancelRuntimeTurn { turn: turn.number });
            }
        }
        (effects, Vec::new())
    }

    fn on_settled(
        &mut self,
        outcome: AgentSettlement,
        clock: &dyn SessionClock,
    ) -> (Vec<VoiceEffect>, Vec<VoiceClientEvent>) {
        let Some(turn) = self.active.as_mut() else {
            return (Vec::new(), Vec::new());
        };
        if turn.settlement.is_some() {
            // Duplicate settlement: no second report.
            return (Vec::new(), Vec::new());
        }
        turn.settlement = Some(outcome.clone());
        turn.stages.push((
            BoundedString::new("runtime_settled").expect("fits"),
            clock.now_ms(),
        ));
        // Flush validated speech tail at the turn boundary.
        let (mut effects, mut events) = self.flush_speech(clock);
        // Runtime settlement does not end the voice turn: speech may
        // still be pending. If nothing is outstanding, close now.
        if self.active.as_ref().is_some_and(|turn| turn.units.iter().all(|unit| matches!(unit.play, PlayState::Stopped | PlayState::Acked { .. } if unit.completed()) ) && !turn.units.iter().any(|u| matches!(u.play, PlayState::Draining)))
        {
            let report = self.close_turn(clock, |_| {});
            events.push(VoiceClientEvent::TurnDone(report));
            self.active = None;
            self.gate = None;
            self.capture.clear();
        } else {
            events.push(VoiceClientEvent::AgentActivity(AgentActivity::Speaking));
        }
        effects.append(&mut Vec::new());
        let _ = &mut effects;
        (effects, events)
    }

    fn on_playback_ack(
        &mut self,
        segment: SpeechSegmentId,
        through_bytes: u64,
    ) -> (Vec<VoiceEffect>, Vec<VoiceClientEvent>) {
        let Some(turn) = self.active.as_mut() else {
            return (Vec::new(), Vec::new());
        };
        // Identity + generation + monotonic validation.
        let Some(unit) = turn.units.iter_mut().find(|unit| unit.segment == segment) else {
            return (Vec::new(), Vec::new()); // stale/unknown segment
        };
        if through_bytes < unit.acked_bytes {
            return (Vec::new(), Vec::new()); // non-monotonic: ignore
        }
        unit.acked_bytes = through_bytes;
        unit.play = PlayState::Acked { through_bytes };
        (Vec::new(), Vec::new())
    }

    fn on_playback_unavailable(&mut self) -> (Vec<VoiceEffect>, Vec<VoiceClientEvent>) {
        if let Some(turn) = self.active.as_mut() {
            for unit in &mut turn.units {
                if unit.play == PlayState::Draining {
                    unit.play = PlayState::Unknown;
                }
            }
        }
        (Vec::new(), Vec::new())
    }

    fn on_failure(
        &mut self,
        class: DeviceFailureClass,
        clock: &dyn SessionClock,
    ) -> (Vec<VoiceEffect>, Vec<VoiceClientEvent>) {
        match class {
            DeviceFailureClass::Playback | DeviceFailureClass::Provider(_) => {
                // Speech-side failure: stop affected speech, report a
                // voice failure; never fabricate runtime failure.
                let mut events = Vec::new();
                if let Some(turn) = self.active.as_mut() {
                    for unit in &mut turn.units {
                        if matches!(unit.play, PlayState::Draining | PlayState::Acked { .. }) {
                            unit.play = PlayState::Stopped;
                        }
                    }
                    if turn.settlement.is_some() {
                        let report = self.close_turn(clock, |_| {});
                        events.push(VoiceClientEvent::Failed(VoiceError::Inference(
                            "playback failed; textual result preserved by the host".into(),
                        )));
                        events.push(VoiceClientEvent::TurnDone(report));
                        self.active = None;
                    } else {
                        events.push(VoiceClientEvent::Failed(VoiceError::Inference(
                            "playback failed; runtime turn continues".into(),
                        )));
                    }
                }
                (Vec::new(), events)
            }
            DeviceFailureClass::Capture => (
                Vec::new(),
                vec![VoiceClientEvent::Failed(VoiceError::Unavailable {
                    provider: vesper_domain::ProviderId::new("host-capture").expect("fits"),
                    reason: BoundedString::new("capture device failed").expect("fits"),
                })],
            ),
        }
    }

    fn on_interrupt(
        &mut self,
        barge_in: bool,
        clock: &dyn SessionClock,
    ) -> (Vec<VoiceEffect>, Vec<VoiceClientEvent>) {
        let mut effects: Vec<VoiceEffect> = Vec::new();
        let mut events = Vec::new();
        #[allow(unused_assignments)]
        let mut can_close = false;
        let Some(turn) = self.active.as_mut() else {
            // Idempotent no-op without an active turn.
            return (effects, events);
        };
        if turn.interrupted
            || (turn.settlement.is_some()
                && turn
                    .units
                    .iter()
                    .all(|unit| unit.play == PlayState::Stopped))
        {
            // Already interrupted (idempotent even before settlement
            // arrives) or fully stopped-and-settled: no duplicate effects.
            return (effects, events);
        }
        // Snapshot decisions from the turn, then release the borrow so
        // self-methods (note staging, close) may run.
        let turn_number;
        let needs_runtime_cancel;
        {
            // Urgent effects FIRST: playback stop and synthesis cancel
            // are never queued behind runtime cancellation.
            effects.push(VoiceEffect::StopPlayback);
            let draining: Vec<SpeechSegmentId> = turn
                .units
                .iter()
                .filter(|unit| matches!(unit.play, PlayState::Draining | PlayState::Acked { .. }))
                .map(|unit| unit.segment)
                .collect();
            if !draining.is_empty() {
                effects.push(VoiceEffect::CancelSynthesis { segments: draining });
            }
            for unit in turn.units.iter_mut() {
                if !matches!(unit.play, PlayState::Unknown) {
                    unit.play = PlayState::Stopped;
                }
            }
            turn.interrupted = true;
            turn_number = turn.number;
            needs_runtime_cancel =
                turn.settlement.is_none() && turn.submitted && turn.runtime_turn.is_some();
            if turn.settlement.is_none() && turn.submitted && turn.runtime_turn.is_none() {
                // Cancellation before identity: preserve intent.
                turn.cancel_before_identity = true;
            }
            let settled_now = turn.settlement.is_some();
            let all_stopped = turn
                .units
                .iter()
                .all(|unit| unit.play == PlayState::Stopped);
            can_close = settled_now && all_stopped;
        }
        // Freeze the interruption cutoff NOW: the note is composed once
        // from acknowledged playback evidence at this instant.
        self.stage_interruption_note();
        // Speech work ends; the gate is finalized (no further text).
        if let Some(gate) = self.gate.as_mut() {
            let _ = gate.finalize();
        }
        if needs_runtime_cancel {
            if let Some(turn) = self.active.as_mut() {
                turn.cancel_requested = true;
            }
            effects.push(VoiceEffect::CancelRuntimeTurn { turn: turn_number });
        }
        let _ = barge_in; // Capture restart is a host gesture (PR-4).
        events.push(VoiceClientEvent::Interrupted {
            heard_through: self.staged_note.clone(),
        });
        if can_close {
            let report = self.close_turn(clock, |_| {});
            events.push(VoiceClientEvent::TurnDone(report));
            self.active = None;
            self.gate = None;
            self.capture.clear();
        }
        (effects, events)
    }

    /// Composes the bounded interruption note exactly once from
    /// acknowledged playback evidence (sanitized unit text only).
    fn stage_interruption_note(&mut self) {
        if self.staged_note.is_some() {
            return; // one-time
        }
        let Some(turn) = &self.active else { return };
        let completed: Vec<&UnitRecord> =
            turn.units.iter().filter(|unit| unit.completed()).collect();
        let note = if completed.is_empty() {
            // No unit was acknowledged at all: the honest claim is
            // that progress was NOT CONFIRMED (missing evidence), not
            // that nothing completed — some audio may have played.
            String::from(
                "[context: the previous spoken reply was interrupted; playback progress was not confirmed.]",
            )
        } else {
            let last = completed.last().expect("nonempty checked");
            format!(
                "[context: the user interrupted the spoken reply after: {}]",
                last.text.as_str()
            )
        };
        self.staged_note = BoundedString::new(note).ok();
    }

    fn close_turn(
        &mut self,
        clock: &dyn SessionClock,
        extra: impl FnOnce(&mut TurnState),
    ) -> VoiceTurnReport {
        let end = clock.now_ms();
        let turn = self
            .active
            .as_mut()
            .expect("close_turn called with an active turn");
        extra(turn);
        turn.stages
            .push((BoundedString::new("turn_end").expect("fits"), end));
        let mut markers = Vec::new();
        if turn
            .units
            .iter()
            .any(|unit| unit.play == PlayState::Unknown)
        {
            markers.push(ReportMarker::PlaybackUnknown);
        }
        VoiceTurnReport {
            turn_number: turn.number,
            runtime_turn: turn.runtime_turn.as_ref().map(|id| {
                BoundedString::new(id.as_str())
                    .unwrap_or_else(|_| BoundedString::new("unprintable").expect("fits"))
            }),
            stt_provider: match &turn.stt_final {
                Some(SttFinal::Transcript(transcript)) => Some(transcript.provider.clone()),
                _ => None,
            },
            tts_provider: None,
            settlement: turn.settlement.clone(),
            interrupted: turn.interrupted,
            segments: turn.units.iter().map(|unit| unit.segment).collect(),
            playback: turn.units.iter().map(|unit| unit.play).collect(),
            stage_ms: turn.stages.clone(),
            markers,
            failure: None,
        }
    }

    /// Number of accepted turns (diagnostics).
    #[must_use]
    pub fn accepted_turns(&self) -> u64 {
        self.turns
    }

    /// The session's STT port (hosts drive finals through this and
    /// deliver results via [`Self::submit_stt_final`]; the session
    /// never blocks on inference inside an event handler).
    #[must_use]
    pub fn stt(&self) -> &S {
        &self.stt
    }

    /// Bounded recent-event retention cap (diagnostics bound).
    #[must_use]
    pub fn max_events(&self) -> usize {
        self.max_events
    }
}
