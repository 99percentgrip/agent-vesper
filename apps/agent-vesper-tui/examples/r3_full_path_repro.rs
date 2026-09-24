//! R3 defect reproduction #4: the full production speech path, both
//! engines, exactly as the TUI drives it — capture gesture constructs
//! the controller, `assistant_delta` produces hygiene-gated speak units,
//! `speak_unit` synthesizes + plays synchronously (as the event loop
//! calls it). A watchdog aborts a wedged call and names the stage.
//!
//! NOTE: plays audio through the default device (aplay).

use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_vesper_tui::voice_conversation::{ConversationHost, EngineSelection};
use agent_vesper_tui::voice_shared_stt::SharedSidecarStt;
use vesper_voice::events::HostEvent;

/// Mirrors the bin's `DictationSharedStt` (same shared sidecar).
struct SharedStt {
    inner: SharedSidecarStt,
}
impl vesper_voice::ports::VoiceStt for SharedStt {
    fn transcribe<'a>(
        &'a self,
        audio: &'a [vesper_voice::audio::PcmFrame],
        cancel: &'a vesper_voice::VoiceCancel,
    ) -> vesper_voice::ports::VoiceFuture<
        'a,
        Result<vesper_voice::ports::SttTranscript, vesper_voice::error::VoiceError>,
    > {
        self.inner.transcribe(audio, cancel)
    }
    fn descriptor(&self) -> &vesper_voice::ports::SttDescriptor {
        self.inner.descriptor()
    }
}

fn main() {
    println!("repro #4: full ConversationHost speech path (both engines)");
    println!("NOTE: plays audio through the default device (aplay).");
    let mut host: ConversationHost<SharedStt> = ConversationHost::new(
        Arc::new(SharedStt {
            inner: SharedSidecarStt::new(),
        }),
        None,
    );

    let scope = vesper_voice::read_voice_scope(
        &std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
    )
    .unwrap_or_default();
    println!(
        "resolved selection from saved scope: {:?}",
        EngineSelection::from_scope(&scope)
    );

    // Construct the controller through the real gesture (as F9 does).
    let (outcome, _) = host.gesture(HostEvent::CaptureStarted);
    println!("capture gesture: {outcome:?}");

    // Watchdog: report if the synchronous speak path wedges (the TUI's
    // event loop has no watchdog — this is the diagnostic the TUI lacks).
    let dead = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let dead_watch = dead.clone();
    std::thread::spawn(move || {
        for second in 1..=180u64 {
            std::thread::sleep(Duration::from_secs(1));
            if dead_watch.load(std::sync::atomic::Ordering::Acquire) {
                return;
            }
            if second % 10 == 0 {
                eprintln!("[watchdog] main thread still inside speak: {second}s");
            }
        }
        eprintln!("[watchdog] 180 s inside speak — treating as WEDGED; aborting");
        std::process::abort();
    });

    // Production order: capture → final (submits the turn, creates the
    // hygiene gate) → deltas produce units → settle.
    let final_result =
        vesper_voice::session::SttFinal::Transcript(vesper_voice::ports::SttTranscript {
            text: vesper_domain::BoundedString::<8192>::new(
                "Do not use any tools. Reply with one sentence.".to_owned(),
            )
            .expect("fits"),
            provider: vesper_domain::ProviderId::new("shared-sidecar").expect("fits"),
            confidence: None,
            provenance: vesper_voice::ports::TranscriptProvenance::InferredText,
        });
    let (_events, submitted) = host.deliver_final(final_result);
    println!("deliver_final submitted: {} input(s)", submitted.len());

    let reply = "Understood. The build finished in four seconds and all checks passed.";
    let started = Instant::now();
    let (units, _) = host.assistant_delta(reply);
    println!(
        "[t+{:?}] delta produced {} speak unit(s)",
        started.elapsed(),
        units.len()
    );
    if units.is_empty() {
        println!("NO SPEAK UNITS — the hygiene gate produced nothing; neither engine would speak");
        return;
    }
    for (index, unit) in units.iter().enumerate() {
        let unit_started = Instant::now();
        match host.speak_unit(unit) {
            Ok(()) => println!(
                "[t+{:?}] unit {index} spoke + drained in {:?}",
                started.elapsed(),
                unit_started.elapsed()
            ),
            Err(error) => println!("unit {index} FAILED: {error}"),
        }
    }
    dead.store(true, std::sync::atomic::Ordering::Release);
    let events = host.runtime_settled(vesper_voice::events::AgentSettlement::Completed);
    println!("settled: {} events; repro complete", events.len());
}
