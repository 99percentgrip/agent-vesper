//! Bounded speech pipeline: synthesis overlaps the preceding unit's playback.
//!
//! The event thread only enqueues and drains outcomes. A synthesis worker owns
//! the selected engine; a playback lane owns delivery. Their zero-capacity
//! rendezvous allows at most one prepared lookahead while one unit plays.
//! Stop and selection replacement invalidate generations and flush the player;
//! stream admission is serialized with Stop, never pipe writes or drain waits.
//! In-flight native inference is non-preemptible; stale output is discarded.
//! Text remains in chat. Transport/drain outcomes never prove human audibility.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use vesper_voice::audio::CANONICAL_SAMPLE_RATE_HZ;

use super::voice_conversation::{EngineHandle, EngineSelection};
use super::voice_playback::PlaybackOwner;

/// One unit of speech work.
#[derive(Debug, Clone)]
pub struct SpeechJob {
    /// The session's speech segment id.
    pub segment: u64,
    /// Hygiene-passed text to speak.
    pub text: String,
}

/// What the worker reports per job (drained by the event loop).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeechOutcome {
    /// Transport progress: bytes of this segment handed to the player's
    /// stdin (an honest transport ack, not a heard-proof).
    Progress { segment: u64, through_bytes: u64 },
    /// Synthesized and streamed into the playback owner to completion.
    Spoke { segment: u64, samples: usize },
    /// Superseded by stop / selection change (dropped, not an error).
    Stale { segment: u64 },
    /// Synthesis failed; the textual answer stays on screen.
    Failed { segment: u64, error: String },
}

/// Current software stage; timing never asserts acoustic onset.
type StageState = Arc<Mutex<(&'static str, std::time::Instant)>>;

#[derive(Clone)]
struct WorkerFeedback {
    results: std::sync::mpsc::Sender<SpeechOutcome>,
    stage: StageState,
}
impl WorkerFeedback {
    fn send(
        &self,
        outcome: SpeechOutcome,
    ) -> Result<(), std::sync::mpsc::SendError<SpeechOutcome>> {
        self.results.send(outcome)
    }
    fn stage(&self, label: &'static str) {
        if let Ok(mut state) = self.stage.lock() {
            *state = (label, std::time::Instant::now());
        }
    }
}

#[derive(Default)]
struct SpeechControl {
    generation: AtomicU64,
    // Serializes stream admission with Stop, never pipe writes or drain waits.
    admission: Mutex<()>,
}

impl SpeechControl {
    fn stop(&self, playback: &PlaybackOwner) {
        let _guard = self.admission.lock();
        self.generation.fetch_add(1, Ordering::AcqRel);
        playback.stop_flush();
    }
}

enum Command {
    Speak(SpeechJob, u64),
    Stop,
    Replace(EngineSelection),
    Shutdown,
}

/// The speech worker: owns the engine; synthesis + playback run here.
pub struct SpeechWorker {
    sender: Mutex<std::sync::mpsc::Sender<Command>>,
    results: Mutex<std::sync::mpsc::Receiver<SpeechOutcome>>,
    feedback: std::sync::mpsc::Sender<SpeechOutcome>,
    stage: StageState,
    playback_stage: StageState,
    queued: Arc<AtomicUsize>,
    generation: Arc<SpeechControl>,
    dead: Arc<AtomicBool>,
    /// The shared playback owner (worker and stop path both use it).
    playback: Arc<PlaybackOwner>,
}

impl SpeechWorker {
    /// Spawns the worker with the current selection and playback owner.
    #[must_use]
    pub fn spawn(selection: EngineSelection, playback: Arc<PlaybackOwner>) -> Self {
        Self::spawn_inner(selection, playback, None)
    }

    /// Spawns the real worker pipeline with deterministic synthesis.
    ///
    /// This integration-test seam keeps playback, queueing, generation,
    /// and recovery behavior in production while removing any dependency
    /// on a developer machine's installed speech engine.
    #[doc(hidden)]
    #[must_use]
    pub fn spawn_with_tts_for_test(
        selection: EngineSelection,
        playback: Arc<PlaybackOwner>,
        tts: Arc<dyn vesper_voice::ports::VoiceTts>,
    ) -> Self {
        Self::spawn_inner(selection, playback, Some(tts))
    }

    fn spawn_inner(
        selection: EngineSelection,
        playback: Arc<PlaybackOwner>,
        initial_tts: Option<Arc<dyn vesper_voice::ports::VoiceTts>>,
    ) -> Self {
        let (command_tx, command_rx) = std::sync::mpsc::channel::<Command>();
        let (result_tx, result_rx) = std::sync::mpsc::channel::<SpeechOutcome>();
        let feedback = result_tx.clone();
        let stage = Arc::new(Mutex::new((
            "Preparing voice runtime",
            std::time::Instant::now(),
        )));
        let worker_feedback = WorkerFeedback {
            results: result_tx,
            stage: Arc::clone(&stage),
        };
        let queued = Arc::new(AtomicUsize::new(0));
        let worker_queued = Arc::clone(&queued);
        let generation = Arc::new(SpeechControl::default());
        let dead = Arc::new(AtomicBool::new(false));
        let worker_generation = Arc::clone(&generation);
        let worker_dead = Arc::clone(&dead);
        let worker_playback = Arc::clone(&playback);
        let playback_stage = Arc::new(Mutex::new(("Idle", std::time::Instant::now())));
        let player_feedback = WorkerFeedback {
            results: feedback.clone(),
            stage: Arc::clone(&playback_stage),
        };
        let spawned = std::thread::Builder::new()
            .name("vesper-speech".into())
            .spawn(move || {
                run_worker(
                    command_rx,
                    worker_feedback,
                    selection,
                    initial_tts,
                    worker_playback,
                    worker_generation,
                    worker_dead,
                    worker_queued,
                    player_feedback,
                );
            });
        if spawned.is_err() {
            dead.store(true, Ordering::Release);
        }
        Self {
            sender: Mutex::new(command_tx),
            results: Mutex::new(result_rx),
            feedback,
            stage,
            playback_stage,
            queued,
            generation,
            dead,
            playback,
        }
    }

    /// Actual worker stage and elapsed time within it (not heard evidence).
    #[must_use]
    pub fn stage_status(&self) -> String {
        [&self.stage, &self.playback_stage]
            .iter()
            .filter_map(|stage| {
                stage.lock().ok().and_then(|state| {
                    (state.0 != "Idle")
                        .then(|| format!("{} · {:.1}s", state.0, state.1.elapsed().as_secs_f64()))
                })
            })
            .collect::<Vec<_>>()
            .join(" / ")
    }

    /// The shared playback owner (the stop path flushes through it).
    #[must_use]
    pub fn playback(&self) -> &Arc<PlaybackOwner> {
        &self.playback
    }

    /// Enqueues one unit (never blocks). Outcomes surface on `drain`.
    pub fn enqueue(&self, job: SpeechJob) {
        let segment = job.segment;
        if job.text.len() > 8192
            || self
                .queued
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                    (count < 32).then_some(count + 1)
                })
                .is_err()
        {
            let _ = self.feedback.send(SpeechOutcome::Failed {
                segment,
                error: "speech queue capacity exceeded; text retained".into(),
            });
            return;
        }
        let sent = self.sender.lock().ok().is_some_and(|sender| {
            sender
                .send(Command::Speak(
                    job,
                    self.generation.generation.load(Ordering::Acquire),
                ))
                .is_ok()
        });
        if !sent {
            self.queued.fetch_sub(1, Ordering::AcqRel);
            let _ = self.feedback.send(SpeechOutcome::Failed {
                segment,
                error: "speech worker unavailable".into(),
            });
        }
    }

    /// Stops speech now: bumps the generation (queued jobs go stale),
    /// flushes playback immediately, and the worker abandons any
    /// in-flight stream at its next chunk. Never blocks.
    pub fn stop(&self) {
        self.generation.stop(&self.playback);
        if let Ok(sender) = self.sender.lock() {
            let _ = sender.send(Command::Stop);
        }
    }

    /// Stops old playback and swaps the engine after a Settings save.
    /// In-flight inference retains its old engine but its result goes stale.
    pub fn replace_selection(&self, selection: EngineSelection) {
        if let Ok(sender) = self.sender.lock() {
            self.generation.stop(&self.playback);
            let _ = sender.send(Command::Replace(selection));
        }
    }

    /// Queued plus currently synthesizing/playing units (not audibility).
    #[must_use]
    pub fn is_busy(&self) -> bool {
        self.queued.load(Ordering::Acquire) > 0
    }

    /// The current speech generation (VRO-17 §2 regression seam: a
    /// no-op engine reload must not bump it).
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation.generation.load(Ordering::Acquire)
    }

    /// Drains outcomes (never blocks). A dead worker reports once.
    #[must_use]
    pub fn drain(&self) -> Vec<SpeechOutcome> {
        let mut outcomes = Vec::new();
        if let Ok(results) = self.results.lock() {
            while let Ok(outcome) = results.try_recv() {
                outcomes.push(outcome);
            }
        }
        if self.dead.swap(false, Ordering::AcqRel) {
            outcomes.push(SpeechOutcome::Failed {
                segment: 0,
                error: "the speech worker exited unexpectedly".into(),
            });
        }
        outcomes
    }

    /// Shuts the worker down (host teardown); safe to call once.
    pub fn shutdown(&self) {
        if let Ok(sender) = self.sender.lock() {
            let _ = sender.send(Command::Shutdown);
        }
    }
}

impl Drop for SpeechWorker {
    fn drop(&mut self) {
        self.stop();
        self.shutdown();
    }
}

#[derive(Clone)]
enum SpeechEngine {
    Selected(Arc<EngineHandle>),
    Injected(Arc<dyn vesper_voice::ports::VoiceTts>),
}

impl SpeechEngine {
    fn tts(&self) -> &dyn vesper_voice::ports::VoiceTts {
        match self {
            Self::Selected(selected) => match selected.as_ref() {
                EngineHandle::System(system) => system.as_ref(),
                #[cfg(feature = "voice-kokoro")]
                EngineHandle::Neural(neural) => neural.as_ref(),
            },
            Self::Injected(tts) => tts.as_ref(),
        }
    }
}

/// Owns synthesis; hands one prepared unit at a time to the playback lane.
#[allow(clippy::too_many_arguments)] // worker composition boundary
fn run_worker(
    commands: std::sync::mpsc::Receiver<Command>,
    results: WorkerFeedback,
    selection: EngineSelection,
    initial_tts: Option<Arc<dyn vesper_voice::ports::VoiceTts>>,
    playback: Arc<PlaybackOwner>,
    generation: Arc<SpeechControl>,
    dead: Arc<AtomicBool>,
    queued: Arc<AtomicUsize>,
    player_feedback: WorkerFeedback,
) {
    // Rendezvous handoff: while the player owns one unit, the producer can
    // synthesize ahead into a bounded two-unit bank. Depth 2 is the
    // measured remedy for the boundary starvation Alex's recording shows:
    // with depth 0 the producer sat pinned through the lane's whole
    // write+drain window, so any successor synthesis slower than the
    // previous unit's playback landed as dead air (reproduced at
    // 1.019 s in the paced fixture; multi-second with production Kokoro).
    // Depth 1 is still insufficient (the unit behind the bank overruns
    // identically); two banked units absorb a cold synthesis transient
    // of roughly one unit's audio. This is a bounded prebuffer, never a
    // turn buffer: admission stays at 32 units of at most 8192 bytes,
    // each piece stays under the 16 MiB prepared cap, and stale banked
    // audio is discarded by the generation check without reopening the
    // player.
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<PreparedSpeech>(2);
    let player_control = Arc::clone(&generation);
    let player_queued = Arc::clone(&queued);
    let spawned = std::thread::Builder::new()
        .name("vesper-speech-playback".into())
        .spawn(move || {
            let mut sequence = PlaybackSequence::default();
            while let Ok(prepared) = ready_rx.recv() {
                if let Some(outcome) = play_prepared(
                    prepared,
                    &playback,
                    &player_control,
                    &player_feedback,
                    &mut sequence,
                ) {
                    let _ = player_feedback.send(outcome);
                    player_queued.fetch_sub(1, Ordering::AcqRel);
                }
                player_feedback.stage("Idle");
            }
        });
    if spawned.is_err() {
        dead.store(true, Ordering::Release);
        // Still consume commands; send failure reports each admitted job.
    }
    let mut selection = selection;
    let mut voice_id = selection_voice_id(&selection);
    let mut engine = initial_tts
        .map(SpeechEngine::Injected)
        .or_else(|| build_engine(&selection).map(SpeechEngine::Selected));
    let mut onset_piece_used = false;
    if engine.is_none() {
        let _ = results.send(SpeechOutcome::Failed {
            segment: 0,
            error: engine_error(&selection),
        });
    }
    results.stage("Idle");
    loop {
        let command = match commands.recv() {
            Ok(command) => command,
            Err(_) => return, // host dropped the worker
        };
        match command {
            Command::Stop | Command::Replace(_) => {
                // Generation was already bumped by the caller. A Replace
                // also rebuilds the engine for subsequent jobs.
                onset_piece_used = false;
                if let Command::Replace(new_selection) = command {
                    results.stage("Preparing voice runtime");
                    selection = new_selection;
                    voice_id = selection_voice_id(&selection);
                    engine = build_engine(&selection).map(SpeechEngine::Selected);
                    if engine.is_none() {
                        let _ = results.send(SpeechOutcome::Failed {
                            segment: 0,
                            error: engine_error(&selection),
                        });
                    }
                }
                results.stage("Idle");
                continue;
            }
            Command::Shutdown => return,
            Command::Speak(job, job_generation) => {
                if job_generation != generation.generation.load(Ordering::Acquire) {
                    let _ = results.send(SpeechOutcome::Stale {
                        segment: job.segment,
                    });
                    queued.fetch_sub(1, Ordering::AcqRel);
                    continue;
                }
                let Some(engine) = engine.clone() else {
                    let _ = results.send(SpeechOutcome::Failed {
                        segment: job.segment,
                        error: engine_error(&selection),
                    });
                    queued.fetch_sub(1, Ordering::AcqRel);
                    continue;
                };
                // Segment ids restart at zero for each VoiceSession turn.
                // The latency onset cut belongs to the TURN, not to every
                // hygiene sentence: repeating it per segment creates a short
                // cover followed by a large successor and reintroduces
                // stop-resume speech throughout long answers.
                if job.segment == 0 {
                    onset_piece_used = false;
                }
                let pieces = speech_pieces_for_position(&job.text, !onset_piece_used);
                onset_piece_used = true;
                // Keep the accepted long-form buffering margin intact. The
                // reproduced defect is confined to short Kokoro units, where
                // fixed model padding dominates the spoken content; trimming
                // long units shortened their playback cover enough to expose
                // slower successor inference in the paced regression.
                let trims_kokoro_piece_edges = should_trim_kokoro_unit(&selection, &job.text);
                let failed = Arc::new(AtomicBool::new(false));
                for (index, text) in pieces.iter().enumerate() {
                    let part = SpeechJob {
                        segment: job.segment,
                        text: (*text).to_owned(),
                    };
                    let prepared = if failed.load(Ordering::Acquire) {
                        Err(SpeechOutcome::Failed {
                            segment: job.segment,
                            error: "speech delivery stopped; text retained".into(),
                        })
                    } else {
                        prepare_one(
                            engine.tts(),
                            &part,
                            job_generation,
                            &generation.generation,
                            &results,
                            &voice_id,
                        )
                    };
                    let (mut pcm, failure) = match prepared {
                        Ok(pcm) => (pcm, None),
                        Err(outcome) => (Vec::new(), Some(outcome)),
                    };
                    if failure.is_none() {
                        pcm = prepare_piece_pcm(
                            pcm,
                            trims_kokoro_piece_edges,
                            job.segment,
                            index,
                            pieces.len(),
                        );
                    }
                    let final_piece = index + 1 == pieces.len() || failure.is_some();
                    results.stage("Waiting for playback slot");
                    if ready_tx
                        .send(PreparedSpeech {
                            segment: job.segment,
                            generation: job_generation,
                            pcm,
                            first_piece: index == 0,
                            final_piece,
                            failure,
                            failed: Arc::clone(&failed),
                        })
                        .is_err()
                    {
                        let _ = results.send(SpeechOutcome::Failed {
                            segment: job.segment,
                            error: "speech playback worker unavailable".into(),
                        });
                        queued.fetch_sub(1, Ordering::AcqRel);
                        break;
                    }
                    if final_piece {
                        break;
                    }
                }
                results.stage("Idle");
            }
        }
    }
}

// At most two prepared units exist: the playing unit and one lookahead.
// Match the bounded local subprocess output budget; never collect a full turn.
const MAX_PREPARED_BYTES: usize = 16 * 1024 * 1024;

// Kokoro pads every independent inference with a low-amplitude leading and
// trailing envelope. At a worker piece or hygiene-unit boundary those two
// envelopes stack, so the listener hears a pause even though the player never
// starved. Trimming is neural-only and retains a longer guard at real sentence
// boundaries than at artificial onset splits.
const KOKORO_QUIET_ABS_THRESHOLD: i32 = 400;
const KOKORO_SHORT_UNIT_MAX_CHARS: usize = 64;
const KOKORO_ARTIFICIAL_GUARD_SAMPLES: usize = 800; // 50 ms at canonical 16 kHz
const KOKORO_SENTENCE_GUARD_SAMPLES: usize = 1_600; // 100 ms at canonical 16 kHz

fn should_trim_kokoro_unit(selection: &EngineSelection, text: &str) -> bool {
    matches!(selection, EngineSelection::Neural { .. })
        && text.chars().count() <= KOKORO_SHORT_UNIT_MAX_CHARS
}

fn prepare_piece_pcm(
    pcm: Vec<u8>,
    trims_kokoro_piece_edges: bool,
    segment: u64,
    piece_index: usize,
    piece_count: usize,
) -> Vec<u8> {
    if !trims_kokoro_piece_edges {
        return pcm;
    }
    let leading_guard = if piece_index > 0 {
        Some(KOKORO_ARTIFICIAL_GUARD_SAMPLES)
    } else if segment > 0 {
        Some(KOKORO_SENTENCE_GUARD_SAMPLES)
    } else {
        None
    };
    let trailing_guard = Some(if piece_index + 1 < piece_count {
        KOKORO_ARTIFICIAL_GUARD_SAMPLES
    } else {
        KOKORO_SENTENCE_GUARD_SAMPLES
    });
    trim_kokoro_boundary(pcm, leading_guard, trailing_guard)
}

fn trim_kokoro_boundary(
    pcm: Vec<u8>,
    leading_guard: Option<usize>,
    trailing_guard: Option<usize>,
) -> Vec<u8> {
    if (leading_guard.is_none() && trailing_guard.is_none())
        || pcm.len() < 2
        || !pcm.len().is_multiple_of(2)
    {
        return pcm;
    }
    let samples = pcm.len() / 2;
    let amplitude = |bytes: &[u8]| i32::from(i16::from_le_bytes([bytes[0], bytes[1]])).abs();
    let first_active = pcm
        .chunks_exact(2)
        .position(|bytes| amplitude(bytes) >= KOKORO_QUIET_ABS_THRESHOLD);
    let last_active = pcm
        .chunks_exact(2)
        .rposition(|bytes| amplitude(bytes) >= KOKORO_QUIET_ABS_THRESHOLD);
    let (Some(first_active), Some(last_active)) = (first_active, last_active) else {
        // A wholly quiet/very-soft result has no evidence for a safe speech
        // boundary. Keep it intact so the trim can never manufacture empty
        // audio or erase low-level speech.
        return pcm;
    };
    let start = leading_guard.map_or(0, |guard| first_active.saturating_sub(guard));
    let end = trailing_guard.map_or(samples, |guard| (last_active + 1 + guard).min(samples));
    if start == 0 && end == samples {
        pcm
    } else {
        pcm[start * 2..end * 2].to_vec()
    }
}

struct PreparedSpeech {
    segment: u64,
    generation: u64,
    pcm: Vec<u8>,
    first_piece: bool,
    final_piece: bool,
    failure: Option<SpeechOutcome>,
    failed: Arc<AtomicBool>,
}

/// Split only already hygiene-approved text, losslessly at UTF-8 word/clause
/// boundaries. Short utterances stay whole; no unvalidated provider fragments
/// are admitted. No punctuation or filler is invented, and chat is unchanged.
#[cfg(test)]
fn speech_pieces(text: &str) -> Vec<&str> {
    speech_pieces_for_position(text, true)
}

fn speech_pieces_for_position(mut text: &str, allow_onset_cut: bool) -> Vec<&str> {
    // VRO-17 continuity repair (supersedes the 28/48 micro-cut decision):
    // the reconnaissance measured that per-piece model fade-in/out quiet
    // runs (~0.25–0.46 s each side) CONCATENATE at every artificial
    // mid-sentence boundary — 7.2 s of inserted silence across 11
    // 28/48-char cuts in a 41 s passage, at commas/spaces where no
    // natural pause exists. Sentence-level pieces stack the same fades
    // only at natural sentence pauses (measured ≈ 0 added silence) at
    // the same aggregate RTF and onset (2.67 → 2.73 s).
    //
    // Policy:
    // - The FIRST piece stays clause-sized for onset (a genuine clause
    //   boundary when available: `,`/`;`/`:`/`—` + whitespace), with the
    //   existing bounded word fallback — never an arbitrary four-word cut.
    // - Successors are SENTENCE-LEVEL: a whole remaining sentence (or the
    //   bounded fallback when a single sentence exceeds the model/audio
    //   limits).
    // - Limits are unchanged: the piece must fit the phoneme-context and
    //   waveform budgets; a long sentence falls back to clause cuts only
    //   where those real limits require it.
    if text.chars().count() <= 32 {
        return vec![text];
    }
    let mut pieces = Vec::new();
    loop {
        let remaining = text.chars().count();
        if pieces.is_empty()
            && ((allow_onset_cut && remaining <= 32)
                || (!allow_onset_cut && remaining <= SENTENCE_TARGET_CHARS))
        {
            break;
        }
        let is_first = allow_onset_cut && pieces.is_empty();
        let after_onset = allow_onset_cut && pieces.len() == 1;
        let target = if is_first { 28 } else { SENTENCE_TARGET_CHARS };
        let minimum = if is_first { 12 } else { SENTENCE_MINIMUM_CHARS };
        let cut = if is_first {
            // Prefer the first genuine bounded clause. The former search
            // stopped at the 28-character fallback target, so ordinary
            // openings with a comma at 50–90 characters were cut at an
            // arbitrary word and left too little playable cover for the
            // successor inference. If no bounded clause exists, preserve
            // the old 28-character word fallback and its prompt onset.
            const ONSET_CLAUSE_MAX_CHARS: usize = 64;
            let mut clause_cut = None;
            let mut word_cut = None;
            for (chars, (index, ch)) in text.char_indices().enumerate() {
                if chars > ONSET_CLAUSE_MAX_CHARS {
                    break;
                }
                if ch.is_whitespace() && chars >= minimum {
                    if chars <= target {
                        word_cut = Some(index + ch.len_utf8());
                    }
                    if text[..index].ends_with([',', ';', ':', '—']) {
                        clause_cut = Some(index + ch.len_utf8());
                        break;
                    }
                }
            }
            clause_cut.or(word_cut)
        } else if after_onset {
            // The immediate successor must be ready before the short onset
            // audio drains. Prefer the next real clause boundary; if prose
            // has no bounded punctuation, use one honest word-boundary
            // fallback. Later successors remain sentence-level.
            onset_successor_cut(text)
        } else {
            // Sentence-level successor: cut at the next sentence end.
            sentence_cut(text, minimum, target)
        };
        let Some(cut) = cut else {
            break;
        };
        pieces.push(&text[..cut]);
        text = &text[cut..];
    }
    if !text.is_empty() {
        pieces.push(text);
    }
    pieces
}

/// Sentence-level successor target: the whole next sentence when it fits
/// the budget, else its clause boundaries, else the bounded word fallback.
/// A sentence end is `.`, `!`, `?`, or `。` followed by whitespace/end.
const SENTENCE_TARGET_CHARS: usize = 510;
const SENTENCE_MINIMUM_CHARS: usize = 24;

fn sentence_cut(text: &str, minimum: usize, target: usize) -> Option<usize> {
    // Find the first sentence terminator after `minimum` characters.
    let mut sentence_end = None;
    for (chars, (index, ch)) in text.char_indices().enumerate() {
        if chars < minimum {
            continue;
        }
        if chars > target {
            break;
        }
        if matches!(ch, '.' | '!' | '?' | '。') {
            let after = text[index + ch.len_utf8()..].chars().next();
            if after.is_none_or(char::is_whitespace) {
                sentence_end = Some(index + ch.len_utf8());
                break;
            }
        }
    }
    if let Some(end) = sentence_end {
        return Some(end);
    }
    // No sentence end in range: clause boundary, then word fallback.
    let mut clause_cut = None;
    let mut word_cut = None;
    for (chars, (index, ch)) in text.char_indices().enumerate() {
        if chars > target {
            break;
        }
        if ch.is_whitespace() && chars >= minimum {
            word_cut = Some(index + ch.len_utf8());
            if text[..index].ends_with([',', ';', ':', '—']) {
                clause_cut = word_cut;
            }
        }
    }
    clause_cut.or(word_cut)
}

fn onset_successor_cut(text: &str) -> Option<usize> {
    const CLAUSE_MAX_CHARS: usize = 96;
    const WORD_FALLBACK_CHARS: usize = 48;
    let mut word_cut = None;
    for (chars, (index, ch)) in text.char_indices().enumerate() {
        if chars > CLAUSE_MAX_CHARS {
            break;
        }
        if ch.is_whitespace() {
            if (SENTENCE_MINIMUM_CHARS..=WORD_FALLBACK_CHARS).contains(&chars) {
                word_cut = Some(index + ch.len_utf8());
            }
            if chars >= 12 && text[..index].ends_with([',', ';', ':', '—']) {
                return Some(index + ch.len_utf8());
            }
        }
        if matches!(ch, '.' | '!' | '?' | '。') {
            let after = text[index + ch.len_utf8()..].chars().next();
            if after.is_none_or(char::is_whitespace) {
                return Some(index + ch.len_utf8());
            }
        }
    }
    word_cut.or_else(|| sentence_cut(text, SENTENCE_MINIMUM_CHARS, SENTENCE_TARGET_CHARS))
}

#[derive(Default)]
struct PlaybackSequence {
    samples: usize,
    failure: Option<SpeechOutcome>,
}

/// Synthesizes one bounded unit without waiting for the player's device drain.
fn prepare_one(
    inner: &dyn vesper_voice::ports::VoiceTts,
    job: &SpeechJob,
    expected_generation: u64,
    generation: &AtomicU64,
    results: &WorkerFeedback,
    voice_id: &str,
) -> Result<Vec<u8>, SpeechOutcome> {
    let profile = vesper_voice::ports::VoiceProfile {
        voice_id: vesper_domain::BoundedString::<128>::new(voice_id.to_owned()).unwrap_or_else(
            |_| vesper_domain::BoundedString::<128>::new("af_heart".to_owned()).expect("fits"),
        ),
        label: vesper_domain::BoundedString::<128>::new(voice_id.to_owned()).unwrap_or_else(|_| {
            vesper_domain::BoundedString::<128>::new("voice".to_owned()).expect("fits")
        }),
        sample_rate_hz: CANONICAL_SAMPLE_RATE_HZ,
    };
    let stale = || SpeechOutcome::Stale {
        segment: job.segment,
    };
    if generation.load(Ordering::Acquire) != expected_generation {
        return Err(stale());
    }
    results.stage("Synthesizing voice");
    let cancel = vesper_voice::VoiceCancel::new();
    let stream =
        match vesper_voice::test_util::block_on(inner.synthesize(&job.text, &profile, &cancel)) {
            Ok(stream) => stream,
            Err(error) => {
                if generation.load(Ordering::Acquire) != expected_generation {
                    return Err(stale());
                }
                return Err(SpeechOutcome::Failed {
                    segment: job.segment,
                    error: error.to_string(),
                });
            }
        };
    if generation.load(Ordering::Acquire) != expected_generation {
        return Err(stale());
    }
    collect_pcm(stream, job.segment, expected_generation, generation)
}

fn collect_pcm(
    mut stream: vesper_voice::ports::TtsStream<'_>,
    segment: u64,
    expected_generation: u64,
    generation: &AtomicU64,
) -> Result<Vec<u8>, SpeechOutcome> {
    let stale = || SpeechOutcome::Stale { segment };
    use futures_util::StreamExt as _;
    let mut pcm = Vec::new();
    loop {
        if generation.load(Ordering::Acquire) != expected_generation {
            return Err(stale());
        }
        match vesper_voice::test_util::block_on(stream.next()) {
            Some(Ok(vesper_voice::ports::TtsChunk::Audio(frame))) => {
                if frame.bytes().is_empty() {
                    continue;
                }
                if frame.bytes().len() > MAX_PREPARED_BYTES - pcm.len() {
                    return Err(SpeechOutcome::Failed {
                        segment,
                        error: "prepared speech exceeds memory budget; text retained".into(),
                    });
                }
                pcm.extend_from_slice(frame.bytes());
            }
            Some(Err(error)) => {
                return Err(SpeechOutcome::Failed {
                    segment,
                    error: error.to_string(),
                });
            }
            Some(Ok(vesper_voice::ports::TtsChunk::Finished)) | None => break,
        }
    }
    if generation.load(Ordering::Acquire) != expected_generation {
        return Err(stale());
    }
    if pcm.is_empty() {
        return Err(SpeechOutcome::Failed {
            segment,
            error: "synthesis returned no audio; text retained".into(),
        });
    }
    Ok(pcm)
}

fn play_prepared(
    prepared: PreparedSpeech,
    playback: &PlaybackOwner,
    control: &SpeechControl,
    results: &WorkerFeedback,
    sequence: &mut PlaybackSequence,
) -> Option<SpeechOutcome> {
    if prepared.first_piece {
        *sequence = PlaybackSequence::default();
    }
    let segment = prepared.segment;
    if control.generation.load(Ordering::Acquire) != prepared.generation {
        sequence.failure = Some(SpeechOutcome::Stale { segment });
    }
    if sequence.failure.is_none() {
        sequence.failure = prepared.failure;
    }
    if sequence.failure.is_none() {
        match write_piece(
            &prepared.pcm,
            segment,
            prepared.generation,
            playback,
            control,
            results,
        ) {
            Ok(samples) => sequence.samples += samples,
            Err(outcome) => sequence.failure = Some(outcome),
        }
    }
    if sequence.failure.is_some() {
        prepared.failed.store(true, Ordering::Release);
        playback.stop_flush();
    }
    if !prepared.final_piece {
        return None;
    }
    if let Some(outcome) = sequence.failure.take() {
        return Some(outcome);
    }
    results.stage("Waiting for player drain");
    let receipt = playback.end_stream();
    if control.generation.load(Ordering::Acquire) != prepared.generation {
        return Some(SpeechOutcome::Stale { segment });
    }
    Some(match receipt {
        Ok(crate::voice_playback::PlaybackReceipt::Drained) => SpeechOutcome::Spoke {
            segment,
            samples: sequence.samples,
        },
        Ok(_) => SpeechOutcome::Failed {
            segment,
            error: "player completion unconfirmed".into(),
        },
        Err(error) => SpeechOutcome::Failed {
            segment,
            error: error.to_string(),
        },
    })
}

fn write_piece(
    pcm: &[u8],
    segment: u64,
    expected_generation: u64,
    playback: &PlaybackOwner,
    control: &SpeechControl,
    results: &WorkerFeedback,
) -> Result<usize, SpeechOutcome> {
    let stale = || SpeechOutcome::Stale { segment };
    let is_stale = || control.generation.load(Ordering::Acquire) != expected_generation;
    let failed = |error: String| SpeechOutcome::Failed { segment, error };
    {
        let Ok(_admission) = control.admission.lock() else {
            return Err(failed("speech admission unavailable".into()));
        };
        if is_stale() {
            return Err(stale());
        }
        results.stage("Starting audio player");
        if let Err(error) = playback.begin_stream() {
            return Err(failed(error.to_string()));
        }
    }
    for chunk in pcm.chunks(crate::voice_playback::MAX_QUEUED_BYTES) {
        if is_stale() {
            return Err(stale());
        }
        results.stage("Sending PCM to player");
        let receipt = playback.push_pcm(chunk);
        if is_stale() {
            return Err(stale());
        }
        match receipt {
            Ok(crate::voice_playback::PlaybackReceipt::BytesWritten(through_bytes)) => {
                let _ = results.send(SpeechOutcome::Progress {
                    segment,
                    through_bytes,
                });
            }
            other => {
                return Err(failed(other.err().map_or_else(
                    || "playback delivery unconfirmed".into(),
                    |error| error.to_string(),
                )));
            }
        }
    }
    Ok(pcm.len() / 2)
}

/// The selected voice id (pack voice for neural; engine voice name for
/// the system baseline) — the adapter validates it, so the real id must
/// ride every synthesis job ("voice" was the defect: a placeholder the
/// neural adapter rightly refused).
fn selection_voice_id(selection: &EngineSelection) -> String {
    match selection {
        EngineSelection::System { voice_name } => voice_name.clone(),
        EngineSelection::Neural { voice_id } => voice_id.clone(),
    }
}

fn build_engine(selection: &EngineSelection) -> Option<Arc<EngineHandle>> {
    match selection {
        EngineSelection::System { voice_name } => {
            match vesper_voice::tts_subprocess::SubprocessTts::new(
                vesper_voice::tts_subprocess::SubprocessTtsConfig {
                    voice: voice_name.clone(),
                    ..vesper_voice::tts_subprocess::SubprocessTtsConfig::default()
                },
                Arc::new(vesper_voice::composition::blocking::ThreadPoolExecutor::new(1)),
            ) {
                Ok(engine) => Some(Arc::new(EngineHandle::System(Arc::new(engine)))),
                Err(_) => None,
            }
        }
        #[cfg(feature = "voice-kokoro")]
        EngineSelection::Neural { voice_id: _ } => {
            match vesper_voice_kokoro::KokoroTts::new(Arc::new(
                vesper_voice::composition::blocking::ThreadPoolExecutor::new(1),
            )) {
                Ok(engine) => {
                    // Build on this worker while the user records/transcribes,
                    // not after the first provider sentence arrives.
                    if engine.prepare().is_err() {
                        return None;
                    }
                    let _ =
                        vesper_voice_kokoro::setup::write_lease(&vesper_voice_kokoro::pack_root());
                    Some(Arc::new(EngineHandle::Neural(Arc::new(engine))))
                }
                Err(_) => None,
            }
        }
        #[cfg(not(feature = "voice-kokoro"))]
        EngineSelection::Neural { .. } => None,
    }
}

fn engine_error(selection: &EngineSelection) -> String {
    match selection {
        EngineSelection::System { .. } => "system speech engine could not start".into(),
        #[cfg(feature = "voice-kokoro")]
        EngineSelection::Neural { .. } => {
            "the Natural Voice pack could not start; check Settings → Voice".into()
        }
        #[cfg(not(feature = "voice-kokoro"))]
        EngineSelection::Neural { .. } => {
            "this build does not include the Natural Voice pack capability".into()
        }
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    use futures_util::StreamExt as _;

    #[test]
    fn speech_pieces_preserve_text_utf8_and_prefer_natural_boundaries() {
        for text in [
            "Short preview.",
            "",
            "A".repeat(200).as_str(),
            "Words with Unicode naïve élève 日本語 repeated here, so the first phrase has a natural boundary and preserves every character and whitespace without appending any punctuation.",
        ] {
            assert_eq!(speech_pieces(text).concat(), text);
        }
        let text = "Words with Unicode naïve élève 日本語 repeated here, so the first phrase has a natural boundary and preserves every character and whitespace without appending any punctuation.";
        let pieces = speech_pieces(text);
        assert!(pieces.len() > 1);
        assert!(pieces[0].chars().last().is_some_and(char::is_whitespace));
        assert!(pieces[0].chars().count() <= 96);
        assert!(pieces[0].trim_end().ends_with(','));
        assert_eq!(speech_pieces("Short preview."), vec!["Short preview."]);
        // VRO-17 continuity repair (supersedes the old 28/48 test): the
        // Preview phrase now splits ONCE — the clause-sized onset piece
        // plus the remainder (a sentence-level successor), instead of
        // micro-cutting a 39-char phrase. Onset stays protected by the
        // first-piece rule; continuity improves because the remainder is
        // one inference, not two stacked fades.
        let preview = "This is a preview of the selected voice.";
        let preview_pieces = speech_pieces(preview);
        assert_eq!(preview_pieces.concat(), preview);
        assert!(preview_pieces[0].chars().count() <= 28);
        assert!(
            preview_pieces.len() <= 2,
            "a 39-char phrase must not micro-cut: {preview_pieces:?}"
        );
        // The long-answer shape: first piece clause-sized, successors
        // sentence-level (one inference per sentence, not per 48 chars).
        let long = "Here is the summary. The first repair removed the fatal error flag from the player argv, because the default recovers underruns. The second repair preserved the original write error. The third contained dead streams. Together these restored speech.";
        let long_pieces = speech_pieces(long);
        assert_eq!(long_pieces.concat(), long);
        assert!(
            long_pieces.len() >= 4,
            "sentence-level successors: {long_pieces:?}"
        );
        // Every successor beyond the first begins at a sentence start
        // (no mid-sentence micro-cut): each successor starts with a word
        // character, and the piece before it ended with sentence
        // punctuation.
        for window in long_pieces.windows(2).skip(1) {
            let (_, previous) = window[0].rsplit_once(' ').unwrap_or(("", window[0]));
            let _ = previous;
            let trimmed_prev = window[0].trim_end();
            let ended_sentence = trimmed_prev
                .chars()
                .last()
                .is_some_and(|c| matches!(c, '.' | '!' | '?' | '。'))
                || trimmed_prev.ends_with(',');
            let next_word = window[1].trim_start();
            let next_is_list_marker = next_word
                .chars()
                .enumerate()
                .all(|(i, c)| i < 3 && (c.is_ascii_digit() || c == '.'))
                && next_word.starts_with(|c: char| c.is_ascii_digit());
            assert!(
                ended_sentence || next_is_list_marker,
                "successors cut at sentence boundaries or list markers: {:?}",
                long_pieces
            );
        }
    }

    #[test]
    fn kokoro_artificial_boundary_keeps_only_a_short_quiet_guard() {
        fn pcm(samples: impl IntoIterator<Item = i16>) -> Vec<u8> {
            samples.into_iter().flat_map(i16::to_le_bytes).collect()
        }

        let quiet = std::iter::repeat_n(0_i16, 4_800); // 300 ms
        let voiced = std::iter::repeat_n(2_000_i16, 1_600);
        let original = pcm(quiet.clone().chain(voiced).chain(quiet));

        let first = trim_kokoro_boundary(
            original.clone(),
            None,
            Some(KOKORO_ARTIFICIAL_GUARD_SAMPLES),
        );
        let successor = trim_kokoro_boundary(
            original.clone(),
            Some(KOKORO_ARTIFICIAL_GUARD_SAMPLES),
            None,
        );
        let removed_bytes = (4_800 - KOKORO_ARTIFICIAL_GUARD_SAMPLES) * 2;
        assert_eq!(
            first.len(),
            original.len() - removed_bytes,
            "the first piece must shed only its artificial trailing pad"
        );
        assert_eq!(
            successor.len(),
            original.len() - removed_bytes,
            "the successor must shed only its artificial leading pad"
        );
        assert_eq!(
            trim_kokoro_boundary(original.clone(), None, None),
            original.clone(),
            "a whole one-piece unit has no artificial boundary to trim"
        );
        assert_eq!(
            prepare_piece_pcm(original.clone(), false, 0, 0, 2),
            original,
            "the system speech engine must never inherit Kokoro trimming"
        );
        assert!(should_trim_kokoro_unit(
            &EngineSelection::Neural {
                voice_id: "am_michael".into()
            },
            "Please reply now."
        ));
        assert!(!should_trim_kokoro_unit(
            &EngineSelection::Neural {
                voice_id: "am_michael".into()
            },
            &"a".repeat(KOKORO_SHORT_UNIT_MAX_CHARS + 1)
        ));
    }

    #[test]
    fn sentence_level_successors_reduce_artificial_boundaries() {
        // The measured continuity objective: in a multi-sentence passage,
        // the number of mid-sentence artificial boundaries drops to the
        // minimum (long sentences exceeding the budget still split at
        // clause boundaries, reported honestly).
        let long = "Alpha beta gamma delta epsilon. Zeta eta theta iota kappa lambda mu nu xi omicron pi. Rho sigma tau upsilon phi chi psi omega and the sentence continues past the budget here. A final short one.";
        let pieces = speech_pieces(long);
        assert_eq!(pieces.concat(), long);
        // 4 sentences → at most 1 forced split (the over-budget one) →
        // at most 5 pieces.
        assert!(
            pieces.len() <= 5,
            "sentence-level policy must not micro-cut: {pieces:?}"
        );
    }

    #[test]
    fn onset_cut_is_once_per_turn_not_once_per_sentence_segment() {
        let sentence = "The workspace is intact, and nothing is blocking me right now: no pending failures or half-finished work on my side.";
        let opening = speech_pieces_for_position(sentence, true);
        assert!(
            opening.len() > 1,
            "the first turn segment retains bounded early speech: {opening:?}"
        );

        let successor = speech_pieces_for_position(sentence, false);
        assert_eq!(
            successor,
            vec![sentence],
            "later sentence segments must not restart the onset cut"
        );
        assert_eq!(successor.concat(), sentence);
    }

    #[test]
    fn demonstrated_opening_keeps_prompt_onset_then_uses_a_natural_clause_successor() {
        let text = "I'm running inside the Agent Vesper harness with cognitive memory active, so context from past sessions carries over automatically.";
        let pieces = speech_pieces_for_position(text, true);
        assert_eq!(pieces.concat(), text);
        assert!(
            pieces[0].chars().count() <= 29,
            "prompt onset stays bounded: {pieces:?}"
        );
        assert!(pieces[0].chars().last().is_some_and(char::is_whitespace));
        assert!(
            pieces[1].trim_end().ends_with(','),
            "the immediate successor must use the demonstrated passage's natural comma boundary: {pieces:?}"
        );
    }

    #[test]
    fn prepared_pcm_is_bounded_empty_is_failure_and_cancellation_wins() {
        use vesper_voice::{audio::PcmFrame, ports::TtsChunk};
        let generation = AtomicU64::new(0);
        let audio = |bytes| Ok(TtsChunk::Audio(PcmFrame::from_aligned(bytes).unwrap()));
        let collected = collect_pcm(
            Box::pin(futures_util::stream::iter(vec![
                audio(vec![1, 2]),
                audio(vec![3, 4]),
            ])),
            7,
            0,
            &generation,
        )
        .unwrap();
        assert_eq!(collected, vec![1, 2, 3, 4]);
        let empty = collect_pcm(Box::pin(futures_util::stream::empty()), 7, 0, &generation);
        assert!(
            matches!(empty, Err(SpeechOutcome::Failed { error, .. }) if error.contains("no audio"))
        );
        let oversized = collect_pcm(
            Box::pin(futures_util::stream::iter(vec![
                audio(vec![0; MAX_PREPARED_BYTES]),
                audio(vec![0; 2]),
            ])),
            7,
            0,
            &generation,
        );
        assert!(
            matches!(oversized, Err(SpeechOutcome::Failed { error, .. }) if error.contains("memory budget"))
        );
        let during_poll = futures_util::stream::iter(vec![audio(vec![1, 2])]).inspect(|_| {
            generation.store(1, Ordering::Release);
        });
        assert_eq!(
            collect_pcm(Box::pin(during_poll), 7, 0, &generation),
            Err(SpeechOutcome::Stale { segment: 7 })
        );
    }

    #[test]
    fn stopped_prepared_audio_cannot_reopen_player() {
        let control = SpeechControl::default();
        let playback = PlaybackOwner::new("/nonexistent-player".into(), None);
        control.stop(&playback);
        let feedback = WorkerFeedback {
            results: std::sync::mpsc::channel().0,
            stage: Arc::new(Mutex::new(("Idle", std::time::Instant::now()))),
        };
        let result = play_prepared(
            PreparedSpeech {
                segment: 8,
                generation: 0,
                pcm: vec![1, 2],
                first_piece: true,
                final_piece: true,
                failure: None,
                failed: Arc::new(AtomicBool::new(false)),
            },
            &playback,
            &control,
            &feedback,
            &mut PlaybackSequence::default(),
        );
        assert_eq!(result, Some(SpeechOutcome::Stale { segment: 8 }));
    }

    #[test]
    fn stop_rejects_already_queued_generation_before_engine_use() {
        let (tx, rx) = std::sync::mpsc::channel();
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        tx.send(Command::Speak(
            SpeechJob {
                segment: 42,
                text: "never synthesize".into(),
            },
            0,
        ))
        .unwrap();
        tx.send(Command::Shutdown).unwrap();
        // An empty system voice is rejected at construction, without any
        // process, model, cache, or device access. Staleness must win first.
        run_worker(
            rx,
            WorkerFeedback {
                results: result_tx,
                stage: Arc::new(Mutex::new((
                    "Preparing voice runtime",
                    std::time::Instant::now(),
                ))),
            },
            EngineSelection::System {
                voice_name: String::new(),
            },
            None,
            Arc::new(PlaybackOwner::new("/nonexistent-player".into(), None)),
            Arc::new(SpeechControl {
                generation: AtomicU64::new(1),
                admission: Mutex::new(()),
            }),
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicUsize::new(1)),
            WorkerFeedback {
                results: std::sync::mpsc::channel().0,
                stage: Arc::new(Mutex::new(("Idle", std::time::Instant::now()))),
            },
        );
        assert!(
            result_rx
                .try_iter()
                .any(|result| result == SpeechOutcome::Stale { segment: 42 })
        );
    }
}
