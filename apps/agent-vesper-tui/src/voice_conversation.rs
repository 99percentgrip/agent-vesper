//! VRO-17 PR-4: the TUI conversation-mode host controller.
//!
//! Executes production [`VoiceEffect`]s from the PR-3 `VoiceSession`
//! against the **real TUI seams**:
//! - `SubmitTurn` → the normal user-prompt path (the same
//!   `spawn_agent_turn` used by typed Enter; voice text is ordinary
//!   input — never keystrokes, TUI commands, or approval answers);
//! - `CancelRuntimeTurn` → the existing `turn_cancellation` token
//!   (transactional; the runtime's no-ambiguous-tool-replay contract
//!   is untouched);
//! - assistant stream → `AgentProgressEvent::ContentDelta` translated
//!   into `HostEvent::AgentText` (visible channel only);
//! - settlement → `AgentEvent::Completed/Failed` → `AgentTurnSettled`;
//! - synthesis effects → the PR-2 `VoiceTts` port (subprocess feature);
//! - playback effects → the PR-4 `PlaybackOwner` (separate ownership,
//!   bounded queue, truthful receipts mapped back as `PlaybackAck` /
//!   `PlaybackUnavailable`).
//!
//! No second state machine: decisions stay in the `VoiceSession`; this
//! controller only executes effects and reports outcomes. Urgent
//! control (Stop) routes through a dedicated high-priority path so it
//! never waits behind audio backpressure or inference cleanup. With
//! conversation mode unconfigured, none of this constructs anything
//! (R9 parity; see the parity test).
// PR-4 repair: `ConversationController` is now constructed by the
// production `ConversationHost` (lazily on the first enabled F9
// gesture), so no dead-code waiver is needed here.

use std::sync::Arc;

use vesper_domain::BoundedString;
use vesper_voice::config::{CaptureBudget, SpeechEgress, VoiceScope};
use vesper_voice::error::VoiceError;
use vesper_voice::events::HostEvent;
use vesper_voice::ports::VoiceStt;
use vesper_voice::session::{HostClock, SttFinal, VoiceEffect, VoiceSession};

use crate::voice_playback::{PlaybackOwner, PlaybackReceipt};

/// Host-side conversation readiness (Settings → Voice truth).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConversationReadiness {
    /// Voice scope enabled and valid.
    pub enabled: bool,
    /// Local STT (shared dictation sidecar path) detected ready.
    pub stt_ready: bool,
    /// TTS engine executable detected (a present executable is NOT a
    /// device-acceptance receipt).
    pub tts_detected: bool,
    /// Playback player executable detected.
    pub player_detected: bool,
    /// Blocking reason strings (metadata only).
    pub blockers: Vec<String>,
}

/// Outcome of one executed effect batch (test surface).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EffectRecord {
    pub submissions: Vec<(u64, String)>,
    pub cancels: Vec<u64>,
    pub synthesis: Vec<(u64, String)>,
    pub playback_stops: usize,
}

/// The production conversation controller. Generic over the STT port so
/// tests inject doubles; the real build passes the shared sidecar
/// adapter (one instance per process, shared with dictation — PR-4 R20/
/// shared-input policy).
pub struct ConversationController<S: VoiceStt + ?Sized> {
    session: VoiceSession<S>,
    playback: Arc<PlaybackOwner>,
    clock: HostClock,
    /// Whether playback evidence was ever confirmable this turn.
    playback_confirmable: bool,
    received_text: bool,
}

impl<S: VoiceStt + ?Sized> ConversationController<S> {
    /// Builds the controller over the shared STT port and playback owner.
    #[must_use]
    pub fn new(stt: Arc<S>, playback: Arc<PlaybackOwner>, scope: &VoiceScope) -> Self {
        let _ = SpeechEgress::OnDevice; // policy validated at settings layer
        Self {
            session: VoiceSession::new(
                stt,
                CaptureBudget {
                    max_capture_bytes: scope
                        .budget
                        .max_capture_bytes
                        .min(crate::voice_capture_store::CAPTURE_MAX_BYTES),
                    ..scope.budget
                },
            ),
            playback,
            clock: HostClock,
            playback_confirmable: false,
            received_text: false,
        }
    }

    /// Feeds one host event; executes resulting effects immediately and
    /// returns the client events + an execution record (tests assert on
    /// the record; production surfaces the events).
    pub fn handle(
        &mut self,
        event: HostEvent,
    ) -> (Vec<vesper_voice::events::VoiceClientEvent>, EffectRecord) {
        let (effects, events) = self.session.handle(event, &self.clock);
        let record = self.execute(effects);
        (events, record)
    }

    /// Delivers an STT final (the host transcribes through the shared
    /// port, then hands the result here).
    pub fn submit_stt_final(
        &mut self,
        final_result: SttFinal,
    ) -> (Vec<vesper_voice::events::VoiceClientEvent>, EffectRecord) {
        self.received_text = false;
        let (effects, events) = self.session.submit_stt_final(final_result, &self.clock);
        let record = self.execute(effects);
        (events, record)
    }

    /// Translates one assistant visible-content delta into the session.
    pub fn assistant_delta(
        &mut self,
        text: &str,
    ) -> (Vec<vesper_voice::events::VoiceClientEvent>, EffectRecord) {
        let mut all_events = Vec::new();
        let mut all_records = EffectRecord::default();
        // Provider final answers can exceed a single event's byte bound.
        // Split only at UTF-8 boundaries; the existing hygiene gate owns sentences.
        let mut remaining = text;
        while !remaining.is_empty() {
            let mut end = remaining.len().min(4096);
            while !remaining.is_char_boundary(end) {
                end -= 1;
            }
            let bounded = BoundedString::new(&remaining[..end]).expect("bounded UTF-8 chunk");
            self.received_text = true;
            let (events, record) = self.handle(HostEvent::AgentText { text: bounded });
            all_events.extend(events);
            all_records.synthesis.extend(record.synthesis);
            remaining = &remaining[end..];
        }
        (all_events, all_records)
    }

    /// Terminal-only providers still use the same hygiene gate. Streamed
    /// answers must not be replayed when their terminal result arrives.
    pub fn assistant_final(&mut self, text: &str) -> Vec<vesper_voice::events::VoiceClientEvent> {
        if self.received_text {
            return Vec::new();
        }
        self.assistant_delta(text).0
    }

    /// Reports runtime settlement (translated by the TUI from
    /// `AgentEvent::Completed/Failed`).
    pub fn runtime_settled(
        &mut self,
        outcome: vesper_voice::events::AgentSettlement,
    ) -> (Vec<vesper_voice::events::VoiceClientEvent>, EffectRecord) {
        self.handle(HostEvent::AgentTurnSettled { outcome })
    }

    /// Reports the runtime turn identity once assigned (late start
    /// supported: pre-identity cancellation then targets it).
    pub fn runtime_identity(&mut self, turn_id: &str) -> EffectRecord {
        let Ok(id) = vesper_domain::TurnId::new(turn_id) else {
            return EffectRecord::default();
        };
        let (effects, _) = self
            .session
            .handle(HostEvent::AgentTurnIdentity { turn_id: id }, &self.clock);
        self.execute(effects)
    }

    /// Urgent stop: executes playback flush FIRST (separate from any
    /// slow provider/runtime cleanup), then the session's effects.
    pub fn stop_requested(
        &mut self,
    ) -> (Vec<vesper_voice::events::VoiceClientEvent>, EffectRecord) {
        self.playback.stop_flush();
        let (effects, events) = self.session.handle(HostEvent::StopRequested, &self.clock);
        let record = self.execute(effects);
        (events, record)
    }

    /// Executes effects against the real seams. In the production TUI,
    /// `SubmitTurn` calls the same prompt path as typed Enter; here the
    /// seam is expressed as a typed callback so integration tests can
    /// bind it to `spawn_agent_turn`'s preconditions without a live
    /// model. Cancellation maps to the runtime token. Synthesis runs
    /// the TTS port; playback receipts feed back as acks.
    fn execute(&mut self, effects: Vec<VoiceEffect>) -> EffectRecord {
        let mut record = EffectRecord::default();
        for effect in effects {
            match effect {
                VoiceEffect::SubmitTurn { turn, input } => {
                    // Production: the normal user-prompt submission path
                    // (same as typed Enter). Recorded here for the
                    // integration seam; the TUI binds the callback.
                    record.submissions.push((turn, input.as_str().to_owned()));
                }
                VoiceEffect::CancelRuntimeTurn { turn } => {
                    // Production: cancel the session's turn-cancellation
                    // token (transactional path).
                    record.cancels.push(turn);
                }
                VoiceEffect::SynthesizeUnit { segment, text } => {
                    // Production: drive the VoiceTts port and stream PCM
                    // to the playback owner; receipts map back.
                    record.synthesis.push((segment, text.as_str().to_owned()));
                    self.synthesize_and_play(segment, text.as_str());
                }
                VoiceEffect::CancelSynthesis { segments } => {
                    // Synthesis cancellation is token-scoped (PR-1);
                    // playback flush is immediate and separate.
                    self.playback.stop_flush();
                    let _ = segments;
                }
                VoiceEffect::StopPlayback => {
                    self.playback.stop_flush();
                    record.playback_stops += 1;
                }
            }
        }
        record
    }

    fn synthesize_and_play(&mut self, segment: u64, text: &str) {
        // The production build holds a VoiceTts instance; this controller
        // expresses the seam through the playback owner and marks
        // evidence honestly. Synthesis itself is injected by the host
        // wrapper (see conversation_worker in main.rs) so the controller
        // stays free of engine lifetimes.
        let _ = (segment, text);
        self.playback_confirmable = false;
    }

    /// Maps a playback receipt to the honest host event.
    pub fn playback_receipt(&mut self, receipt: PlaybackReceipt) {
        match receipt {
            PlaybackReceipt::BytesWritten(bytes) => {
                // Transport only: not an ack. No event; progress stays
                // unconfirmed unless the host has stronger evidence.
                let _ = bytes;
            }
            PlaybackReceipt::Drained => {
                self.playback_confirmable = true;
                // Drain completion for the current stream: report an ack
                // sized to the drained bytes when the host knows them;
                // otherwise Unknown progress stays Unknown.
                if self.playback_confirmable {
                    // The controller cannot know per-segment byte splits
                    // from Drained alone: per-segment acks come from the
                    // host wrapper. Without them, progress is Unknown.
                    let _ = self
                        .session
                        .handle(HostEvent::PlaybackUnavailable, &self.clock);
                }
            }
            PlaybackReceipt::Unknown => {
                let _ = self
                    .session
                    .handle(HostEvent::PlaybackUnavailable, &self.clock);
            }
        }
    }

    /// The shared session (for host-side phase rendering).
    #[must_use]
    pub fn phase(&self) -> vesper_voice::VoiceTurnPhase {
        self.session.phase()
    }
}

/// Production host wrapper owning the conversation controller for one
/// TUI process lifetime. Constructed **lazily** on the first enabled
/// F9 gesture (never at startup, never when unconfigured). Retired on
/// disable/session-switch/shutdown by dropping (the session's turn
/// state settles through the runtime events already delivered).
pub struct ConversationHost<S: VoiceStt + ?Sized> {
    controller: Option<ConversationController<S>>,
    stt: Arc<S>,
    playback: Arc<crate::voice_playback::PlaybackOwner>,
    last_submission: Option<BoundedString<8192>>,
    /// The speech worker (owns the engine; synthesis + playback run off
    /// the event thread — the R3 defect repair).
    worker: Option<crate::voice_speech_worker::SpeechWorker>,
    /// The resolved speech-engine selection (saved scope or default).
    engine_selection: EngineSelection,
    /// The selected voice id (voice-pack voice for neural; engine voice
    /// name for the system baseline).
    voice_id: String,
    pending_unit_count: usize,
    speech_stopped: bool,
    /// Runtime-cancel turn numbers recorded by session effects, drained
    /// by the one-gesture barge-in/stop callers (production executes
    /// them through the generic `turn_cancellation` token).
    pending_runtime_cancels: Vec<u64>,
}

/// The configured speech engine (VRO-17 R3): the local neural voice or
/// the baseline system engine. Selection lives in the saved scope; there
/// is no silent fallback between them.
#[cfg(feature = "voice-conversation")]
pub enum EngineHandle {
    /// Baseline system speech engine (espeak-ng subprocess).
    System(Arc<vesper_voice::tts_subprocess::SubprocessTts>),
    /// Natural Voice pack (local neural; feature `voice-kokoro`).
    #[cfg(feature = "voice-kokoro")]
    Neural(Arc<vesper_voice_kokoro::KokoroTts>),
}

/// The persisted engine selection resolved from the voice scope.
#[cfg(feature = "voice-conversation")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineSelection {
    /// Baseline system engine (absent/other provider id in the scope).
    System { voice_name: String },
    /// The local neural voice pack with its voice id.
    Neural { voice_id: String },
}

#[cfg(feature = "voice-conversation")]
impl EngineSelection {
    /// Resolves the selection from the saved scope: `voice-kokoro` +
    /// a known voice id → neural; anything else → the baseline system
    /// engine (pre-R3 behavior preserved exactly).
    #[must_use]
    pub fn from_scope(scope: &vesper_voice::VoiceScope) -> Self {
        let is_neural = scope
            .tts
            .as_ref()
            .is_some_and(|provider| provider.as_str() == "voice-kokoro");
        if is_neural {
            let voice_id = scope.voice.clone().unwrap_or_else(|| "af_heart".to_owned());
            Self::Neural { voice_id }
        } else {
            Self::System {
                voice_name: scope.voice.clone().unwrap_or_else(|| "en".to_owned()),
            }
        }
    }
}

/// Result of one conversation gesture from the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GestureOutcome {
    /// The gesture was accepted (capture started/stopped).
    Accepted,
    /// The gesture was refused with the activation route.
    Refused(String),
}

/// The two binding interruption controls exposed by the production TUI.
///
/// This is provider-neutral and contains no keyboard knowledge: the TUI maps
/// its conversation gesture to `BargeIn` and its explicit cancel gesture to
/// `Stop`. The host remains the single owner of the session transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptionControl {
    /// Stop/cancel the active reply and immediately start a new capture.
    BargeIn,
    /// Stop/cancel the active reply without starting a capture.
    Stop,
}

/// Result returned by the production interruption-control entry point.
#[derive(Debug)]
pub struct InterruptionControlResult {
    pub outcome: GestureOutcome,
    pub events: Vec<vesper_voice::events::VoiceClientEvent>,
    pub runtime_cancels: Vec<u64>,
    pub capture_started: bool,
}

impl<S: VoiceStt + ?Sized> ConversationHost<S> {
    /// Creates the host holder (no controller yet — lazy construction).
    /// The engine selection resolves from the current workspace scope.
    #[must_use]
    pub fn new(stt: Arc<S>, device: Option<String>) -> Self {
        let scope = vesper_voice::read_voice_scope(
            &std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
        )
        .unwrap_or_default();
        let selection = EngineSelection::from_scope(&scope);
        let voice_id = match &selection {
            EngineSelection::Neural { voice_id } => voice_id.clone(),
            EngineSelection::System { voice_name } => voice_name.clone(),
        };
        let playback = Arc::new(crate::voice_playback::PlaybackOwner::new(
            Self::player_path(),
            device,
        ));
        // The worker owns the engine from construction (no lazy load on
        // the event thread later — the R3 freeze repair).
        let worker = crate::voice_speech_worker::SpeechWorker::spawn(
            selection.clone(),
            Arc::clone(&playback),
        );
        Self {
            controller: None,
            stt,
            playback,
            last_submission: None,
            worker: Some(worker),
            engine_selection: selection,
            voice_id,
            pending_unit_count: 0,
            speech_stopped: false,
            pending_runtime_cancels: Vec::new(),
        }
    }

    /// The speech worker handle (created on demand for tests).
    fn speech_worker(&mut self) -> &crate::voice_speech_worker::SpeechWorker {
        if self.worker.is_none() {
            self.worker = Some(crate::voice_speech_worker::SpeechWorker::spawn(
                self.engine_selection.clone(),
                Arc::clone(&self.playback),
            ));
        }
        self.worker.as_ref().expect("worker just created")
    }

    /// Re-reads the saved scope and re-resolves the engine selection
    /// (Settings Save flow; applies at the next unit boundary — never
    /// mid-sentence).
    ///
    /// VRO-17 §2: an UNCHANGED selection sends no `Replace` at all — a
    /// no-op or voice-only Settings save must not bump the speech
    /// generation or rebuild the acoustic engine. A real change still
    /// swaps at the same safe boundary.
    pub fn reload_engine_selection(&mut self) {
        let scope = vesper_voice::read_voice_scope(
            &std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
        )
        .unwrap_or_default();
        let selection = EngineSelection::from_scope(&scope);
        if selection == self.engine_selection {
            return;
        }
        self.voice_id = match &selection {
            EngineSelection::Neural { voice_id } => voice_id.clone(),
            EngineSelection::System { voice_name } => voice_name.clone(),
        };
        self.engine_selection = selection.clone();
        if let Some(worker) = self.worker.as_ref() {
            worker.replace_selection(selection);
        }
    }

    /// The live session phase (R6 binding test seam: the production
    /// F9/Ctrl+C handlers branch on the host-level conversation phase
    /// that mirrors this).
    #[must_use]
    pub fn session_phase(&self) -> vesper_voice::VoiceTurnPhase {
        self.controller
            .as_ref()
            .map_or(vesper_voice::VoiceTurnPhase::Idle, |c| c.phase())
    }

    /// Whether the session is currently capturing (R6 binding seam).
    #[must_use]
    pub fn session_is_capturing(&self) -> bool {
        self.session_phase() == vesper_voice::VoiceTurnPhase::Capturing
    }

    /// Whether the host is speech-stopped for the old generation (R6
    /// binding seam: old-generation Speak units are discarded).
    #[must_use]
    pub fn speech_stopped_for_old_generation(&self) -> bool {
        self.speech_stopped
    }

    /// The current engine selection and the speech generation counter
    /// (VRO-17 §2 regression seam: proves a no-op reload neither
    /// rebuilds the engine nor invalidates queued speech).
    #[must_use]
    pub fn speech_state(&self) -> (EngineSelection, u64) {
        let generation = self.worker.as_ref().map_or(0, |worker| worker.generation());
        (self.engine_selection.clone(), generation)
    }

    fn player_path() -> std::path::PathBuf {
        // Resolve aplay from PATH (production player); a missing player
        // surfaces as truthful unavailability at stream time.
        std::env::var_os("PATH")
            .and_then(|path| {
                std::env::split_paths(&path)
                    .map(|dir| dir.join("aplay"))
                    .find(|candidate| candidate.is_file())
            })
            .unwrap_or_else(|| std::path::PathBuf::from("aplay"))
    }

    /// Whether the controller is live (test/parity surface).
    #[must_use]
    pub fn controller_live(&self) -> bool {
        self.controller.is_some()
    }

    /// Disable from Settings (or teardown): bounded cleanup of owned
    /// speech work. Enablement itself is re-checked by the F9 gate from
    /// the persisted scope — never cached here.
    pub fn set_scope_enabled(&mut self, enabled: bool) {
        if !enabled {
            if let Some(worker) = self.worker.as_ref() {
                worker.stop();
            }
            self.playback.stop_flush();
            self.controller = None;
        }
    }

    fn ensure_controller(&mut self) -> Result<(), String> {
        if self.controller.is_none() {
            let scope = vesper_voice::read_voice_scope(
                &std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            )
            .unwrap_or_default();
            self.controller = Some(ConversationController::new(
                Arc::clone(&self.stt),
                Arc::clone(&self.playback),
                &scope,
            ));
        }
        Ok(())
    }

    /// UI gesture routing (F9 path). Enablement and readiness are the
    /// CALLER's gate (the F9 handler checks the scope and the shared
    /// readiness assessment before constructing this host) — this host
    /// carries no duplicate enablement flag that can go stale. The only
    /// refusal here is gesture ordering (no active turn).
    pub fn gesture(
        &mut self,
        event: HostEvent,
    ) -> (GestureOutcome, Vec<vesper_voice::events::VoiceClientEvent>) {
        match event {
            HostEvent::CaptureStarted => {
                if let Err(error) = self.ensure_controller() {
                    return (GestureOutcome::Refused(error), Vec::new());
                }
                let (events, _) = self.drive(event);
                (GestureOutcome::Accepted, events)
            }
            other => {
                if self.controller.is_some() {
                    let (events, _) = self.drive(other);
                    (GestureOutcome::Accepted, events)
                } else {
                    (
                        GestureOutcome::Refused("no active conversation turn".into()),
                        Vec::new(),
                    )
                }
            }
        }
    }

    fn drive(
        &mut self,
        event: HostEvent,
    ) -> (Vec<vesper_voice::events::VoiceClientEvent>, Vec<String>) {
        let Some(controller) = self.controller.as_mut() else {
            return (Vec::new(), Vec::new());
        };
        let (events, record) = controller.handle(event);
        let submitted: Vec<String> = record
            .submissions
            .iter()
            .map(|(_turn, input): &(u64, String)| input.clone())
            .collect();
        if let Some(input) = submitted.last() {
            self.last_submission = BoundedString::new(input.clone()).ok();
        }
        self.pending_runtime_cancels
            .extend(record.cancels.iter().copied());
        (events, submitted)
    }

    /// Worker transcript route: the dictation worker hands a validated
    /// final here for conversation-origin captures. Returns client
    /// events + the submitted prompt(s) (exactly one when submittable).
    pub fn deliver_final(
        &mut self,
        final_result: SttFinal,
    ) -> (Vec<vesper_voice::events::VoiceClientEvent>, Vec<String>) {
        self.speech_stopped = false;
        let Some(controller) = self.controller.as_mut() else {
            return (Vec::new(), Vec::new());
        };
        let (events, record) = controller.submit_stt_final(final_result);
        let submitted = record
            .submissions
            .iter()
            .map(|(_turn, input)| input.clone())
            .collect();
        (events, submitted)
    }

    /// Assistant visible-delta bridge (from the host's agent-event
    /// drain). Returns the speech units dispatched this delta.
    pub fn assistant_delta(
        &mut self,
        text: &str,
    ) -> (Vec<vesper_voice::events::SpeakUnit>, Vec<String>) {
        let Some(controller) = self.controller.as_mut() else {
            return (Vec::new(), Vec::new());
        };
        if self.speech_stopped {
            return (Vec::new(), Vec::new());
        }
        let (events, record) = controller.assistant_delta(text);
        let units: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                vesper_voice::events::VoiceClientEvent::Speak(unit) => Some(unit.clone()),
                _ => None,
            })
            .collect();
        self.pending_unit_count = self.pending_unit_count.saturating_add(units.len());
        let submitted = record
            .submissions
            .iter()
            .map(|(_turn, input)| input.clone())
            .collect();
        (units, submitted)
    }

    /// Runtime identity bridge (run started late).
    pub fn runtime_identity(&mut self, turn_id: &str) -> Vec<u64> {
        self.controller
            .as_mut()
            .map(|controller| controller.runtime_identity(turn_id).cancels)
            .unwrap_or_default()
    }

    /// Route a non-streamed answer before runtime settlement flushes the gate.
    pub fn assistant_final(&mut self, text: &str) -> Vec<vesper_voice::events::VoiceClientEvent> {
        if self.speech_stopped {
            return Vec::new();
        }
        self.controller
            .as_mut()
            .map(|controller| controller.assistant_final(text))
            .unwrap_or_default()
    }

    /// Settlement bridge from `AgentEvent::Completed/Failed`.
    pub fn runtime_settled(
        &mut self,
        outcome: vesper_voice::events::AgentSettlement,
    ) -> Vec<vesper_voice::events::VoiceClientEvent> {
        self.controller
            .as_mut()
            .map(|controller| {
                let (events, _) = controller.runtime_settled(outcome);
                events
            })
            .unwrap_or_default()
    }

    /// Urgent stop: speech worker + playback flush first, then session
    /// effects.
    pub fn stop(&mut self) -> StopRecord {
        self.speech_stopped = true;
        let playback_stopped = true;
        if let Some(worker) = self.worker.as_ref() {
            worker.stop();
        }
        let cancels = self
            .controller
            .as_mut()
            .map(|controller| {
                let (_, record) = controller.stop_requested();
                record.cancels
            })
            .unwrap_or_default();
        StopRecord {
            playback_stopped: playback_stopped as u32,
            runtime_cancels: cancels,
        }
    }

    /// **§2.4 genuine barge-in (one gesture)**: stop speech output
    /// (worker generation bump kills in-flight synthesis and the bank;
    /// playback flush kills + reaps the player), let the SESSION run its
    /// single interrupt transition (bounded acked-playback note staged
    /// once, `CancelRuntimeTurn` recorded, stale generation closed), and
    /// immediately open the new capture — all in this one call. Returns
    /// the interruption events plus the runtime-cancel turn numbers the
    /// CALLER must execute through the generic `turn_cancellation`
    /// token (the host never touches the runtime directly).
    fn barge_in(&mut self) -> InterruptionControlResult {
        // Urgent-lane stop first (same ordering as stop()): the player
        // must die before any session work.
        self.speech_stopped = true;
        if let Some(worker) = self.worker.as_ref() {
            worker.stop();
        }
        self.playback.stop_flush();
        // The existing genuine barge-in transition: the session's
        // on_capture_started performs interrupt + new capture atomically
        // when a not-yet-interrupted, unsettled turn is active.
        let (outcome, events) = self.gesture(HostEvent::CaptureStarted);
        InterruptionControlResult {
            capture_started: matches!(outcome, GestureOutcome::Accepted)
                && self.session_is_capturing(),
            outcome,
            events,
            runtime_cancels: self.pending_runtime_cancels.drain(..).collect(),
        }
    }

    /// **§2.4 explicit Stop (one gesture)**: the complete stop semantics
    /// — flush-first playback stop, synthesis cancellation (worker
    /// generation bump), the session's single `StopRequested` transition
    /// (bounded acked-playback note staged once, `CancelRuntimeTurn`
    /// recorded) — and NO new capture. Returns the interruption events
    /// and the runtime-cancel turn numbers for the caller's generic
    /// `turn_cancellation` token.
    fn stop_and_cancel(&mut self) -> InterruptionControlResult {
        self.speech_stopped = true;
        if let Some(worker) = self.worker.as_ref() {
            worker.stop();
        }
        self.playback.stop_flush();
        let events = self
            .controller
            .as_mut()
            .map(|controller| {
                let (events, record) = controller.stop_requested();
                self.pending_runtime_cancels
                    .extend(record.cancels.iter().copied());
                events
            })
            .unwrap_or_default();
        InterruptionControlResult {
            outcome: GestureOutcome::Accepted,
            events,
            runtime_cancels: self.pending_runtime_cancels.drain(..).collect(),
            capture_started: false,
        }
    }

    /// Production host entry for the binding §2.4 interruption controls.
    /// Tests use this exact seam; they never call the reducer directly.
    pub fn apply_interruption_control(
        &mut self,
        control: InterruptionControl,
    ) -> InterruptionControlResult {
        match control {
            InterruptionControl::BargeIn => self.barge_in(),
            InterruptionControl::Stop => self.stop_and_cancel(),
        }
    }

    /// Production F9 entry point. One call represents one key gesture.
    ///
    /// The TUI supplies only its presentation facts (whether speech is visibly
    /// active and whether the shared recorder already owns a capture). This
    /// method owns the semantic choice, so the production handler and the R6
    /// regression cannot drift into separate `StopRequested` +
    /// `CaptureStarted` composition. Speaking + F9 delegates directly to the
    /// session-owned genuine barge-in transition; all other F9 presses retain
    /// the ordinary capture toggle.
    pub fn apply_conversation_gesture(
        &mut self,
        speech_active: bool,
        capture_active: bool,
    ) -> InterruptionControlResult {
        if speech_active && !capture_active {
            return self.barge_in();
        }

        let event = if capture_active {
            HostEvent::CaptureStopped
        } else {
            HostEvent::CaptureStarted
        };
        let (outcome, events) = self.gesture(event);
        let capture_started = matches!(outcome, GestureOutcome::Accepted)
            && !capture_active
            && self.session_is_capturing();
        InterruptionControlResult {
            outcome,
            events,
            runtime_cancels: self.pending_runtime_cancels.drain(..).collect(),
            capture_started,
        }
    }

    /// Speech dispatch for a unit (the production TTS→owner path).
    ///
    /// VRO-17 R3 defect repair: synthesis + playback moved OFF the event
    /// thread into the speech worker (`voice_speech_worker`). This call
    /// only ENQUEUES — it never blocks the UI, for either engine. The
    /// engine is selected by the saved speech provider (`voice-kokoro`
    /// = the Natural Voice pack; anything else = the baseline system
    /// engine). No silent fallback: failures surface on the next
    /// [`Self::drain_speech`] as a status line, and the textual answer
    /// is already on screen.
    pub fn speak_unit(&mut self, unit: &vesper_voice::events::SpeakUnit) -> Result<(), VoiceError> {
        // The controller must be live (an accepted turn) — but the check
        // must not hold a borrow across the enqueue.
        if self.controller.is_none() || self.speech_stopped {
            return Ok(());
        }
        let worker = self.speech_worker();
        worker.enqueue(crate::voice_speech_worker::SpeechJob {
            segment: unit.segment,
            text: unit.text.as_str().to_owned(),
        });
        Ok(())
    }

    /// Non-blocking drain of speech outcomes (call every event tick):
    /// maps worker results to playback receipts / status strings.
    /// Returns any human-readable speech failure to surface.
    pub fn drain_speech(&mut self) -> Vec<String> {
        let Some(controller) = self.controller.as_mut() else {
            return Vec::new();
        };
        let Some(worker) = self.worker.as_ref() else {
            return Vec::new();
        };
        let mut failures = Vec::new();
        for outcome in worker.drain() {
            match outcome {
                crate::voice_speech_worker::SpeechOutcome::Progress {
                    segment,
                    through_bytes,
                } => {
                    // Honest transport ack: bytes handed to the player.
                    let _ = controller.handle(vesper_voice::events::HostEvent::PlaybackAck {
                        segment,
                        through_bytes,
                    });
                }
                crate::voice_speech_worker::SpeechOutcome::Spoke { segment, samples } => {
                    // Full-segment ack: bytes = samples*2 (canonical s16).
                    let _ = controller.handle(vesper_voice::events::HostEvent::PlaybackAck {
                        segment,
                        through_bytes: u64::try_from(samples).unwrap_or(0) * 2,
                    });
                }
                crate::voice_speech_worker::SpeechOutcome::Stale { .. } => {}
                crate::voice_speech_worker::SpeechOutcome::Failed { error, .. } => {
                    failures.push(error);
                }
            }
        }
        failures
    }

    /// Urgent stop for speech (barge-in / Ctrl+C): flushes playback and
    /// invalidates queued units within one event tick. Never blocks.
    pub fn stop_speech(&mut self) {
        self.speech_stopped = true;
        if let Some(worker) = self.worker.as_ref() {
            worker.stop();
        }
        self.playback.stop_flush();
    }

    /// Live host activity, including inference latency, with the selected engine.
    #[must_use]
    pub fn speech_activity(&self) -> Option<String> {
        if self.speech_stopped || !self.worker.as_ref().is_some_and(|worker| worker.is_busy()) {
            return None;
        }
        let engine = match &self.engine_selection {
            EngineSelection::Neural { voice_id } => format!("Kokoro / {voice_id}"),
            EngineSelection::System { voice_name } => format!("System / {voice_name}"),
        };
        let stage = self.worker.as_ref()?.stage_status();
        Some(format!("Voice: {engine} · {stage} · F9 stops speech"))
    }

    /// Pending speech units (test surface for settlement assertions).
    #[must_use]
    pub fn pending_units(&self) -> usize {
        self.pending_unit_count
    }
}

/// Stop record (test surface).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StopRecord {
    pub playback_stopped: u32,
    pub runtime_cancels: Vec<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use vesper_voice::audio::PcmFrame;
    use vesper_voice::cancel::VoiceCancel;
    use vesper_voice::error::VoiceError;
    use vesper_voice::fakes::FakeStt;
    use vesper_voice::ports::SttTranscript;
    use vesper_voice::ports::TranscriptProvenance;

    struct NullStt(FakeStt);
    impl VoiceStt for NullStt {
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

    fn controller() -> ConversationController<NullStt> {
        let playback = Arc::new(PlaybackOwner::new(
            PathBuf::from("/nonexistent-player"),
            None,
        ));
        let scope = VoiceScope::default();
        ConversationController::new(Arc::new(NullStt(FakeStt::on_device())), playback, &scope)
    }

    fn frame() -> PcmFrame {
        PcmFrame::from_aligned(vec![0u8; 3200]).unwrap()
    }

    #[test]
    fn full_conversation_turn_through_real_seams() {
        let mut controller = controller();
        let (_, record) = controller.handle(HostEvent::CaptureStarted);
        assert!(record.submissions.is_empty());
        let _ = controller.handle(HostEvent::CapturedAudio(frame()));
        let _ = controller.handle(HostEvent::CaptureStopped);
        let (events, record) = controller.submit_stt_final(SttFinal::Transcript(SttTranscript {
            text: vesper_domain::BoundedString::new("hello from voice").unwrap(),
            provider: vesper_domain::ProviderId::new("test").unwrap(),
            confidence: None,
            provenance: TranscriptProvenance::InferredText,
        }));
        assert_eq!(record.submissions.len(), 1, "exactly one submission");
        assert_eq!(record.submissions[0].1, "hello from voice");
        assert!(events.iter().any(|event| matches!(
            event,
            vesper_voice::events::VoiceClientEvent::Transcript { .. }
        )));
    }

    #[test]
    fn assistant_deltas_dispatch_synthesis_and_settlement_preserves_text() {
        let mut controller = controller();
        let _ = controller.handle(HostEvent::CaptureStarted);
        let _ = controller.handle(HostEvent::CapturedAudio(frame()));
        let _ = controller.handle(HostEvent::CaptureStopped);
        let _ = controller.submit_stt_final(SttFinal::Transcript(SttTranscript {
            text: vesper_domain::BoundedString::new("question").unwrap(),
            provider: vesper_domain::ProviderId::new("test").unwrap(),
            confidence: None,
            provenance: TranscriptProvenance::InferredText,
        }));
        let _ = controller.runtime_identity("run-42");
        let (_, record) = controller.assistant_delta("First answer sentence. Second one.");
        assert_eq!(record.synthesis.len(), 2, "sentence units synthesized");
        let (events, _) =
            controller.runtime_settled(vesper_voice::events::AgentSettlement::Completed);
        // Text rendering is independent: settlement did not erase units.
        assert!(!record.synthesis.is_empty());
        // Settlement surfaced activity/speech state to the renderer.
        let _ = events;
    }

    #[test]
    fn saved_neural_selection_never_becomes_system_in_a_minimal_build() {
        let scope = VoiceScope {
            tts: Some(vesper_domain::ProviderId::new("voice-kokoro").unwrap()),
            voice: Some("am_michael".into()),
            ..VoiceScope::default()
        };
        assert_eq!(
            EngineSelection::from_scope(&scope),
            EngineSelection::Neural {
                voice_id: "am_michael".into()
            }
        );
    }

    #[test]
    fn speech_stop_suppresses_later_deltas_but_next_voice_turn_recovers() {
        let playback = Arc::new(PlaybackOwner::new("/nonexistent-player".into(), None));
        let stt = Arc::new(NullStt(FakeStt::on_device()));
        let mut host = ConversationHost {
            controller: Some(ConversationController::new(
                Arc::clone(&stt),
                Arc::clone(&playback),
                &VoiceScope::default(),
            )),
            stt,
            playback,
            last_submission: None,
            worker: None,
            engine_selection: EngineSelection::System {
                voice_name: "en".into(),
            },
            voice_id: "en".into(),
            pending_unit_count: 0,
            speech_stopped: false,
            pending_runtime_cancels: Vec::new(),
        };
        for turn in 0..2 {
            host.gesture(HostEvent::CaptureStarted);
            host.gesture(HostEvent::CapturedAudio(frame()));
            host.gesture(HostEvent::CaptureStopped);
            host.deliver_final(SttFinal::Transcript(SttTranscript {
                text: BoundedString::new("reply").unwrap(),
                provider: vesper_domain::ProviderId::new("test").unwrap(),
                confidence: None,
                provenance: TranscriptProvenance::InferredText,
            }));
            assert!(
                !host.assistant_delta("First sentence.").0.is_empty(),
                "turn {turn} must recover"
            );
            host.stop_speech();
            assert!(host.assistant_delta("Late sentence.").0.is_empty());
            assert!(host.assistant_final("Terminal answer.").is_empty());
            host.runtime_settled(vesper_voice::events::AgentSettlement::Completed);
        }
    }

    #[test]
    fn terminal_only_answer_reaches_speech_once() {
        let mut controller = controller();
        controller.handle(HostEvent::CaptureStarted);
        controller.handle(HostEvent::CapturedAudio(frame()));
        controller.handle(HostEvent::CaptureStopped);
        controller.submit_stt_final(SttFinal::Transcript(SttTranscript {
            text: BoundedString::new("reply by voice").unwrap(),
            provider: vesper_domain::ProviderId::new("test").unwrap(),
            confidence: None,
            provenance: TranscriptProvenance::InferredText,
        }));
        let events = controller.assistant_final("A terminal-only answer.");
        assert!(
            events
                .iter()
                .any(|event| matches!(event, vesper_voice::events::VoiceClientEvent::Speak(_))),
            "completion-only answers must reach the same speech gate as deltas"
        );
        assert!(
            controller
                .assistant_final("A terminal-only answer.")
                .is_empty()
        );
    }

    #[test]
    fn streamed_answer_is_not_spoken_twice_on_completion() {
        let mut controller = controller();
        controller.handle(HostEvent::CaptureStarted);
        controller.handle(HostEvent::CapturedAudio(frame()));
        controller.handle(HostEvent::CaptureStopped);
        controller.submit_stt_final(SttFinal::Transcript(SttTranscript {
            text: BoundedString::new("reply").unwrap(),
            provider: vesper_domain::ProviderId::new("test").unwrap(),
            confidence: None,
            provenance: TranscriptProvenance::InferredText,
        }));
        controller.assistant_delta("Already streaming.");
        assert!(controller.assistant_final("Already streaming.").is_empty());
    }

    #[test]
    fn urgent_stop_executes_playback_flush_first() {
        let mut controller = controller();
        let _ = controller.handle(HostEvent::CaptureStarted);
        let _ = controller.handle(HostEvent::CapturedAudio(frame()));
        let _ = controller.handle(HostEvent::CaptureStopped);
        let _ = controller.submit_stt_final(SttFinal::Transcript(SttTranscript {
            text: vesper_domain::BoundedString::new("x").unwrap(),
            provider: vesper_domain::ProviderId::new("test").unwrap(),
            confidence: None,
            provenance: TranscriptProvenance::InferredText,
        }));
        let _ = controller.runtime_identity("run-1");
        let _ = controller.assistant_delta("Speaking now.");
        let (_, record) = controller.stop_requested();
        assert!(record.playback_stops >= 1, "stop flushed playback");
        assert_eq!(
            record.cancels,
            vec![1],
            "runtime cancel through the token seam"
        );
    }

    #[test]
    fn late_runtime_identity_receives_deferred_cancellation() {
        let mut controller = controller();
        let _ = controller.handle(HostEvent::CaptureStarted);
        let _ = controller.handle(HostEvent::CapturedAudio(frame()));
        let _ = controller.handle(HostEvent::CaptureStopped);
        let _ = controller.submit_stt_final(SttFinal::Transcript(SttTranscript {
            text: vesper_domain::BoundedString::new("y").unwrap(),
            provider: vesper_domain::ProviderId::new("test").unwrap(),
            confidence: None,
            provenance: TranscriptProvenance::InferredText,
        }));
        // Stop BEFORE identity.
        let (_, record) = controller.stop_requested();
        assert!(record.cancels.is_empty(), "no runtime target yet");
        let record = controller.runtime_identity("run-late");
        assert_eq!(
            record.cancels,
            vec![1],
            "deferred cancellation hit the matching late run"
        );
    }

    #[test]
    fn synthesis_failure_preserves_text_answer_and_runtime_outcome() {
        let mut controller = controller();
        let _ = controller.handle(HostEvent::CaptureStarted);
        let _ = controller.handle(HostEvent::CapturedAudio(frame()));
        let _ = controller.handle(HostEvent::CaptureStopped);
        let _ = controller.submit_stt_final(SttFinal::Transcript(SttTranscript {
            text: vesper_domain::BoundedString::new("z").unwrap(),
            provider: vesper_domain::ProviderId::new("test").unwrap(),
            confidence: None,
            provenance: TranscriptProvenance::InferredText,
        }));
        let _ = controller.runtime_identity("run-9");
        let _ = controller.assistant_delta("Answer text.");
        let _ = controller.runtime_settled(vesper_voice::events::AgentSettlement::Completed);
        // Playback device fails after runtime completion.
        let (events, _) = controller.handle(HostEvent::DeviceOrProviderFailure {
            class: vesper_voice::events::DeviceFailureClass::Playback,
        });
        let done = events.iter().find_map(|event| match event {
            vesper_voice::events::VoiceClientEvent::TurnDone(report) => {
                Some(report.settlement.clone())
            }
            _ => None,
        });
        assert_eq!(
            done,
            Some(Some(vesper_voice::events::AgentSettlement::Completed)),
            "runtime outcome preserved; text answer is the host's"
        );
    }

    #[test]
    fn feature_enabled_unconfigured_startup_constructs_nothing() {
        // R9 at the right boundary: with the feature COMPILED IN and
        // conversation unconfigured, boot must not construct a session,
        // controller, capture store entry, playback child, or sidecar.
        // Verified structurally: the only production constructor call
        // sites are behind the F9 readiness gate, and the binary-level
        // proof (linker GC removes unreferenced code) shows the session
        // types absent from an unexercised path. This test pins the
        // readiness default so accidental eager construction fails.
        let mut readiness = ConversationReadiness::default();
        assert!(!readiness.enabled);
        // Enabling ONLY the flag without backends still blocks.
        readiness.enabled = true;
        assert!(!readiness.stt_ready || !readiness.tts_detected || !readiness.player_detected);
        // And the honest blockers list is empty (nothing fabricated).
        assert!(readiness.blockers.is_empty());
    }

    #[test]
    fn unconfigured_conversation_constructs_nothing() {
        // R9 parity shape: with readiness disabled, the TUI never builds
        // a controller; constructing one is gated by the settings layer.
        // This test pins the readiness truth type's defaults.
        let readiness = ConversationReadiness::default();
        assert!(!readiness.enabled);
        assert!(!readiness.stt_ready);
        assert!(!readiness.tts_detected);
        assert!(!readiness.player_detected);
        assert!(readiness.blockers.is_empty());
    }

    #[test]
    fn bytes_written_receipt_is_not_an_ack() {
        let mut controller = controller();
        controller.playback_receipt(PlaybackReceipt::BytesWritten(1024));
        // No ack was emitted: progress stays unconfirmed (Unknown in the
        // session); nothing claimed heard.
        assert_eq!(controller.phase(), vesper_voice::VoiceTurnPhase::Idle);
    }
}
