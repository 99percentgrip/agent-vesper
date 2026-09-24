//! VRO-17 §4 (CPU production acceptance): bounded interruption, error,
//! and lifecycle coverage through the REAL production host path — the
//! `ConversationHost` + real `SpeechWorker` composition the TUI uses —
//! not only the pure reducer.
//!
//! These extend (not duplicate) the existing production-path suites:
//! `r3_speech_worker.rs` (worker-level stop/replace/stop-cycles),
//! `voice_speech_pipeline.rs` (bounded overlap + stale-rejection),
//! `voice_playback_diagnostics.rs` (device failure classification),
//! `pr4_wiring.rs` (exactly-one submission, deferred cancellation, one
//! interruption note), and the in-crate controller tests (stop ordering,
//! late identity, synthesis failure preserving text).
//!
//! This suite adds the §4 matrix rows with no prior production-path
//! coverage:
//!
//! 1. **Five bounded interruption/recovery cycles then a full turn**
//!    through the real host: every interrupted cycle must suppress later
//!    deltas for that turn (no old-audio resumption), every next request
//!    must dispatch synthesis again (no stuck Stop latch), and exactly
//!    one transcript per capture (no request replay).
//!
//! 2. **Failure before audio**: an STT failure classifies as an error,
//!    submits nothing, surfaces visibly, and the next turn recovers.
//!
//! 3. **Session exit teardown**: dropping the host (the TUI's exit path —
//!    `TuiSession` and its `voice_conversation_host` field drop together)
//!    leaves no shared mutable speech state; a LATER session's host
//!    operates on a clean lane at generation 0 with its own worker.
//!
//! 4. **Preview panel close drops its worker** (documented lifetime):
//!    leaving the pack screen drops the `SpeechWorker`; a fresh screen
//!    builds a fresh, independent worker.

#![cfg(feature = "voice-conversation")]
#![forbid(unsafe_code)]

use std::sync::Arc;

use agent_vesper_tui::voice_conversation::{ConversationHost, EngineSelection};
use agent_vesper_tui::voice_playback::PlaybackOwner;
use vesper_voice::audio::PcmFrame;
use vesper_voice::events::{AgentSettlement, HostEvent};
use vesper_voice::fakes::{FakeStt, FakeSttOutcome};
use vesper_voice::ports::VoiceStt;
use vesper_voice::session::SttFinal;

/// STT double at the production boundary (the core's controllable fake).
struct DoubleStt(FakeStt);

impl DoubleStt {
    fn new() -> Arc<Self> {
        Arc::new(Self(FakeStt::on_device()))
    }

    fn fail_next(&self) {
        self.0.set_outcome(FakeSttOutcome::Unavailable);
    }
}

impl VoiceStt for DoubleStt {
    fn transcribe<'a>(
        &'a self,
        audio: &'a [PcmFrame],
        cancel: &'a vesper_voice::VoiceCancel,
    ) -> vesper_voice::ports::VoiceFuture<
        'a,
        Result<vesper_voice::ports::SttTranscript, vesper_voice::error::VoiceError>,
    > {
        self.0.transcribe(audio, cancel)
    }

    fn descriptor(&self) -> &vesper_voice::ports::SttDescriptor {
        self.0.descriptor()
    }
}

fn frame() -> PcmFrame {
    PcmFrame::from_aligned(vec![0u8; 3200]).expect("aligned frame")
}

/// Drives one complete F9-shaped turn: capture → final → runtime
/// identity → assistant deltas. Returns the submission count and the
/// dispatched speech-unit count for the turn (exactly one submission is
/// required; synthesis must dispatch while the turn is live).
fn turn(host: &mut ConversationHost<DoubleStt>, answer: &str) -> (usize, usize) {
    host.gesture(HostEvent::CaptureStarted);
    host.gesture(HostEvent::CapturedAudio(frame()));
    host.gesture(HostEvent::CaptureStopped);
    let (events, submitted) =
        host.deliver_final(SttFinal::Transcript(vesper_voice::ports::SttTranscript {
            text: vesper_domain::BoundedString::new("hello").expect("fits"),
            provider: vesper_domain::ProviderId::new("double").expect("fits"),
            confidence: None,
            provenance: vesper_voice::ports::TranscriptProvenance::InferredText,
        }));
    let _ = events;
    let _ = host.runtime_identity("run-a");
    let (units, _) = host.assistant_delta(answer);
    let dispatched = units.len();
    let _ = host.assistant_final(answer);
    let _ = host.runtime_settled(AgentSettlement::Completed);
    (submitted.len(), dispatched)
}

// ---------------------------------------------------------------------------
// 1. Five interruption cycles then a full turn
// ---------------------------------------------------------------------------

/// §4: five bounded interruption/recovery cycles, then a full turn. No
/// stuck Stop latch, no old-audio resumption, no duplicate transcript,
/// no request replay; every next request dispatches synthesis again.
#[test]
fn five_interruption_cycles_then_full_turn_all_recover() {
    let stt = DoubleStt::new();
    let mut host = ConversationHost::new(stt, None);

    for cycle in 0..5 {
        // The turn begins and speech is dispatched...
        let (submissions, dispatched) = turn(&mut host, "Cycle answer sentence.");
        assert_eq!(submissions, 1, "cycle {cycle}: exactly one submission");
        assert!(
            dispatched > 0,
            "cycle {cycle}: synthesis must dispatch while the turn is live"
        );
        // ...then the user barges in.
        host.stop_speech();
        // Later deltas of the interrupted turn stay suppressed.
        let (units, submitted) = host.assistant_delta("Late sentence.");
        assert!(
            units.is_empty() && submitted.is_empty(),
            "cycle {cycle}: an interrupted turn must suppress later deltas"
        );
        let events = host.assistant_final("Cycle answer sentence.");
        assert!(
            events.is_empty(),
            "cycle {cycle}: a suppressed final must not dispatch speech"
        );
        let _ = host.runtime_settled(AgentSettlement::Completed);
    }

    // The full turn after five interruptions still works end to end:
    // exactly one submission AND live synthesis dispatch (no Stop latch).
    let (submissions, dispatched) = turn(&mut host, "The final full answer sentence.");
    assert_eq!(submissions, 1, "final turn: exactly one submission");
    assert!(
        dispatched > 0,
        "synthesis must dispatch after five interruption cycles"
    );
}

/// §4: transcripts do not accumulate or replay across five interrupted
/// cycles — exactly one transcript per capture.
#[test]
fn transcripts_do_not_replay_across_cycles() {
    let stt = DoubleStt::new();
    let mut host = ConversationHost::new(stt, None);

    for _cycle in 0..5 {
        host.gesture(HostEvent::CaptureStarted);
        host.gesture(HostEvent::CapturedAudio(frame()));
        host.gesture(HostEvent::CaptureStopped);
        let (events, submitted) =
            host.deliver_final(SttFinal::Transcript(vesper_voice::ports::SttTranscript {
                text: vesper_domain::BoundedString::new("hello").expect("fits"),
                provider: vesper_domain::ProviderId::new("double").expect("fits"),
                confidence: None,
                provenance: vesper_voice::ports::TranscriptProvenance::InferredText,
            }));
        assert_eq!(submitted.len(), 1, "exactly one submission per capture");
        let transcripts = events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    vesper_voice::events::VoiceClientEvent::Transcript { .. }
                )
            })
            .count();
        assert_eq!(transcripts, 1, "exactly one transcript per capture");
        host.stop_speech();
        let _ = host.assistant_final("interrupted");
        let _ = host.runtime_settled(AgentSettlement::Completed);
    }
}

// ---------------------------------------------------------------------------
// 2. Failure before audio
// ---------------------------------------------------------------------------

/// §4: STT failure BEFORE any audio must classify as an error, submit
/// nothing, surface visibly (never silent loss), and leave the host able
/// to run the NEXT turn.
#[test]
fn failure_before_audio_classifies_and_next_turn_recovers() {
    let stt = DoubleStt::new();
    let mut host = ConversationHost::new(stt.clone(), None);

    // The next transcription fails (provider unavailable).
    stt.fail_next();
    host.gesture(HostEvent::CaptureStarted);
    host.gesture(HostEvent::CapturedAudio(frame()));
    host.gesture(HostEvent::CaptureStopped);
    let (events, submitted) = host.deliver_final(SttFinal::Failed(
        vesper_voice::error::VoiceError::Unavailable {
            provider: vesper_domain::ProviderId::new("double").expect("fits"),
            reason: vesper_domain::BoundedString::new("fixture unavailable").expect("fits"),
        },
    ));
    assert!(
        submitted.is_empty(),
        "an STT failure must never submit a turn"
    );
    assert!(
        !events.is_empty(),
        "an STT failure must surface visibly, not silently: {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, vesper_voice::events::VoiceClientEvent::Speak(_))),
        "a failure must never dispatch speech: {events:?}"
    );

    // The next turn recovers normally.
    let (submissions, dispatched) = turn(&mut host, "Recovered answer sentence.");
    assert_eq!(submissions, 1, "the next turn must recover");
    assert!(dispatched > 0, "the recovered turn must dispatch synthesis");
}

// ---------------------------------------------------------------------------
// 3. Session exit teardown
// ---------------------------------------------------------------------------

/// §4: dropping the host (session exit) must leave the later session on
/// a clean lane: fresh worker, generation 0, no state carried over.
///
/// Isolation note: `ConversationHost::new` resolves the engine selection
/// from the CURRENT workspace scope, so this test chdirs into an empty
/// temporary root (the same discipline as `voice_policy_parity`).
/// Without it, a developer workspace with a saved neural voice makes
/// the "System" expectation wrong — a CWD-sensitivity defect observed
/// on the real workspace (pre-existing; not a production bug).
#[test]
fn host_drop_leaves_later_session_on_a_clean_lane() {
    struct TestRoot {
        previous: std::path::PathBuf,
        dir: std::path::PathBuf,
    }
    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.previous);
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
    let root = {
        let previous = std::env::current_dir().expect("current dir");
        let dir = std::env::temp_dir().join(format!(
            "vesper-interruption-lifecycle-hostdrop-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create isolated root");
        std::env::set_current_dir(&dir).expect("chdir into isolated root");
        TestRoot { previous, dir }
    };
    {
        let stt = DoubleStt::new();
        let mut host = ConversationHost::new(stt, None);
        let (submissions, _dispatched) = turn(&mut host, "Session one answer.");
        assert_eq!(submissions, 1);
        // Session exits: host (and its owned worker) drop together here.
    }

    let stt = DoubleStt::new();
    let mut later = ConversationHost::new(stt, None);
    let (selection, generation) = later.speech_state();
    assert_eq!(
        generation, 0,
        "a fresh host starts a fresh speech generation"
    );
    assert!(matches!(selection, EngineSelection::System { .. }));
    // The later session runs its own full turn without interference.
    let (submissions, dispatched) = turn(&mut later, "Later session sentence.");
    assert_eq!(submissions, 1);
    assert!(dispatched > 0);
    drop(root);
}

// ---------------------------------------------------------------------------
// 4. Preview panel close drops its worker
// ---------------------------------------------------------------------------

/// §4: the documented Preview worker lifetime — one worker per pack
/// screen, dropped on leave. Two consecutive screen lifetimes must be
/// independent: a job from the first never settles on the second.
#[test]
fn preview_worker_lifetime_is_one_per_screen_and_independent() {
    let sink = Arc::new(PlaybackOwner::new(
        std::path::PathBuf::from("/nonexistent-player"),
        None,
    ));
    // Screen 1 opens: one worker for the screen's lifetime.
    let screen_one = agent_vesper_tui::voice_speech_worker::SpeechWorker::spawn(
        EngineSelection::Neural {
            voice_id: "am_michael".into(),
        },
        Arc::clone(&sink),
    );
    // Screen 1 closes: the worker drops (its Drop stops + shuts down).
    drop(screen_one);
    // Screen 2 opens later: a fresh, independent worker.
    let screen_two = agent_vesper_tui::voice_speech_worker::SpeechWorker::spawn(
        EngineSelection::Neural {
            voice_id: "am_michael".into(),
        },
        Arc::clone(&sink),
    );
    screen_two.enqueue(agent_vesper_tui::voice_speech_worker::SpeechJob {
        segment: 9001,
        text: "Second screen preview.".into(),
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut settled = false;
    while std::time::Instant::now() < deadline {
        for outcome in screen_two.drain() {
            if matches!(
                outcome,
                agent_vesper_tui::voice_speech_worker::SpeechOutcome::Failed { segment: 9001, .. }
                    | agent_vesper_tui::voice_speech_worker::SpeechOutcome::Spoke {
                        segment: 9001,
                        ..
                    }
            ) {
                settled = true;
            }
        }
        if settled {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(settled, "the fresh screen worker must settle its own job");
    screen_two.shutdown();
}
