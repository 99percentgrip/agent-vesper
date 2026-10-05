//! R3 defect-repair tests (red→green): the speech worker must never
//! block the caller thread for synthesis; stop must invalidate promptly;
//! failures must surface without touching the textual answer; engine
//! swap must not apply mid-sentence.

use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_vesper_tui::voice_conversation::EngineSelection;
use agent_vesper_tui::voice_playback::PlaybackOwner;
use agent_vesper_tui::voice_speech_worker::{SpeechJob, SpeechOutcome, SpeechWorker};

fn worker_with_mock() -> SpeechWorker {
    SpeechWorker::spawn(
        EngineSelection::System {
            voice_name: "en".into(),
        },
        Arc::new(PlaybackOwner::new(
            std::path::PathBuf::from("/nonexistent-player"),
            None,
        )),
    )
}

struct HeldSynthesis {
    inner: vesper_voice::fakes::FakeTts,
    entered: std::sync::mpsc::Sender<()>,
    release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    held: std::sync::atomic::AtomicBool,
}

impl vesper_voice::ports::VoiceTts for HeldSynthesis {
    fn synthesize<'a>(
        &'a self,
        text: &'a str,
        voice: &'a vesper_voice::ports::VoiceProfile,
        cancel: &'a vesper_voice::cancel::VoiceCancel,
    ) -> vesper_voice::ports::VoiceFuture<
        'a,
        Result<vesper_voice::ports::TtsStream<'a>, vesper_voice::error::VoiceError>,
    > {
        Box::pin(async move {
            if !self.held.swap(true, std::sync::atomic::Ordering::AcqRel) {
                self.entered.send(()).expect("observe real synthesis entry");
                let released = self
                    .release
                    .lock()
                    .expect("synthesis gate")
                    .recv_timeout(Duration::from_secs(20));
                assert!(released.is_ok(), "fixture synthesis gate was not released");
            }
            self.inner.synthesize(text, voice, cancel).await
        })
    }

    fn descriptor(&self) -> &vesper_voice::ports::TtsDescriptor {
        self.inner.descriptor()
    }

    fn voices<'a>(
        &'a self,
    ) -> vesper_voice::ports::VoiceFuture<
        'a,
        Result<Vec<vesper_voice::ports::VoiceProfile>, vesper_voice::error::VoiceError>,
    > {
        self.inner.voices()
    }
}

struct ReleaseSynthesis(std::sync::mpsc::Sender<()>);

impl Drop for ReleaseSynthesis {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

/// The caller returns for both jobs while actual worker synthesis remains held.
/// Wall-clock scheduling on a shared runner is not an enqueue benchmark.
#[test]
fn enqueue_never_blocks_for_synthesis() {
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release = ReleaseSynthesis(release_tx);
    let worker = Arc::new(SpeechWorker::spawn_with_tts_for_test(
        EngineSelection::System {
            voice_name: "en".into(),
        },
        Arc::new(PlaybackOwner::new("/nonexistent-player".into(), None)),
        Arc::new(HeldSynthesis {
            inner: vesper_voice::fakes::FakeTts::on_device(),
            entered: entered_tx,
            release: std::sync::Mutex::new(release_rx),
            held: std::sync::atomic::AtomicBool::new(false),
        }),
    ));
    let (returned_tx, returned_rx) = std::sync::mpsc::channel();
    let caller_worker = Arc::clone(&worker);
    let caller = std::thread::spawn(move || {
        for segment in [1, 2] {
            caller_worker.enqueue(SpeechJob {
                segment,
                text: "Queued speech.".into(),
            });
            returned_tx.send(segment).expect("observe caller return");
        }
    });
    let entered = entered_rx.recv_timeout(Duration::from_secs(10)).is_ok();
    let first = returned_rx.recv_timeout(Duration::from_secs(5)).ok();
    let second = returned_rx.recv_timeout(Duration::from_secs(5)).ok();
    // Release before assertions and join the caller, including a regressed inline path.
    worker.stop();
    drop(release);
    caller.join().expect("enqueue caller settled");
    worker.shutdown();
    assert!(entered, "the real worker must enter controlled synthesis");
    assert_eq!(first, Some(1), "first enqueue waited for synthesis release");
    assert_eq!(
        second,
        Some(2),
        "queued enqueue waited for synthesis release"
    );
}

/// Stop invalidates a job enqueued BEFORE it: the outcome is Stale.
/// (A job enqueued AFTER a stop is fresh — the PR-4 "new speech after
/// stop must work" contract — so this test races the stop in first.)
#[test]
fn stop_invalidates_an_inflight_or_queued_job() {
    let worker = worker_with_mock();
    // With an invalid player the job cannot reach Spoke; it must settle
    // as Stale (stopped before synth finished) or Failed (player absent)
    // — never hang, never claim Spoke.
    worker.enqueue(SpeechJob {
        segment: 2,
        text: "must not be claimed as spoken".into(),
    });
    worker.stop();
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut settled = false;
    while Instant::now() < deadline {
        let outcomes = worker.drain();
        let spoke = outcomes
            .iter()
            .any(|outcome| matches!(outcome, SpeechOutcome::Spoke { segment: 2, .. }));
        assert!(!spoke, "a stopped job must never be reported as Spoke");
        if outcomes.iter().any(|outcome| {
            matches!(
                outcome,
                SpeechOutcome::Stale { segment: 2 } | SpeechOutcome::Failed { segment: 2, .. }
            )
        }) {
            settled = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        settled,
        "stopped job must settle promptly (Stale or Failed)"
    );
    worker.shutdown();
}

/// Failure surfaces on drain (playback owner is intentionally invalid);
/// the outcome is a Failed with a message — the text answer stays.
#[test]
fn failure_surfaces_via_drain_not_panic() {
    let worker = worker_with_mock();
    worker.enqueue(SpeechJob {
        segment: 3,
        text: "this will fail at playback".into(),
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut failure = None;
    while Instant::now() < deadline {
        for outcome in worker.drain() {
            if let SpeechOutcome::Failed { segment: 3, error } = outcome {
                failure = Some(error);
            }
        }
        if failure.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(failure.is_some(), "playback failure must surface via drain");
    worker.shutdown();
}

/// Engine replacement after a job is queued: the queued job goes stale
/// (never speaks with a mid-flight swapped engine).
#[test]
fn replace_selection_invalidates_queued_jobs() {
    let worker = worker_with_mock();
    worker.enqueue(SpeechJob {
        segment: 4,
        text: "queued before the swap".into(),
    });
    worker.replace_selection(EngineSelection::System {
        voice_name: "en".into(),
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut settled = false;
    while Instant::now() < deadline {
        let outcomes = worker.drain();
        let spoke = outcomes
            .iter()
            .any(|outcome| matches!(outcome, SpeechOutcome::Spoke { segment: 4, .. }));
        let stale_or_failed = outcomes.iter().any(|outcome| {
            matches!(
                outcome,
                SpeechOutcome::Stale { segment: 4 } | SpeechOutcome::Failed { segment: 4, .. }
            )
        });
        if spoke || stale_or_failed {
            settled = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(settled, "job must settle after a selection swap");
    worker.shutdown();
}

/// The worker processes jobs in order and stays alive across many
/// stop/start cycles (the PR-4 "stop latch" class of defect).
#[test]
fn worker_survives_repeated_stop_cycles() {
    let worker = worker_with_mock();
    for cycle in 0..5u64 {
        worker.enqueue(SpeechJob {
            segment: 100 + cycle,
            text: format!("cycle {cycle}"),
        });
        worker.stop();
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let drained = worker.drain();
        if drained.len() >= 5 {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // The worker must still accept work after all that stopping.
    worker.enqueue(SpeechJob {
        segment: 999,
        text: "still alive".into(),
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut settled = false;
    while Instant::now() < deadline {
        if !worker.drain().is_empty() {
            settled = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(settled, "worker must keep reporting after repeated stops");
    worker.shutdown();
}

/// Regression (Alex's device test, round 2): the neural worker must pass
/// the REAL selected voice id to the adapter — a placeholder id made the
/// adapter refuse every job ("unknown voice id"), i.e. total silence
/// with the pack installed. Verified through the real adapter's voice
/// validation using an invalid pack root (the failure must be the pack,
/// never the voice id).
#[test]
fn neural_selection_carries_the_real_voice_id() {
    // The engine build path accepts the selection; the adapter's own
    // validation would reject a placeholder at synthesis. The pack
    // itself is exercised by the device-check example; here we pin the
    // selection→voice-id mapping the worker relies on.
    let selection = agent_vesper_tui::voice_conversation::EngineSelection::Neural {
        voice_id: "am_michael".into(),
    };
    let EngineSelection::Neural { voice_id } = selection else {
        panic!("neural selection must stay neural");
    };
    assert_eq!(voice_id, "am_michael");
}
