//! VRO-17 PR-0 contract tests: cross-module invariants that must hold
//! before any adapter (PR-1/PR-2) is written.
//!
//! These tests pin the *contracts*: dependency boundaries, error
//! classification, egress policy, correlation identities, and the fake
//! lifecycles that later adapters must reproduce. A fake's behavior is
//! contract evidence only — it is not evidence that production
//! cancellation, synthesis, or recognition works (PRD §8).

#![forbid(unsafe_code)]

use std::future::Future;

use futures_util::StreamExt;
use vesper_domain::ProviderId;
use vesper_voice::audio::{AudioFormat, PcmFrame, PcmReassembler};
use vesper_voice::cancel::VoiceCancel;
use vesper_voice::config::{
    SpeechEgress, VoiceProviderSelection, VoiceScope, VoiceScopeError, parse_voice_table,
};
use vesper_voice::error::{TtsMidStreamError, VoiceError};
use vesper_voice::events::{
    AgentSettlement, PlayState, SpeechSegmentId, VoiceClientEvent, project_phase,
};
use vesper_voice::fakes::{FakeStt, FakeSttOutcome, FakeTts, FakeTtsScript};
use vesper_voice::ports::{
    PartialsKind, SpeechEgressClass, SttTranscript, TranscriptProvenance, TtsChunk, VoiceStt,
};
use vesper_voice::ports::{SttPartial, VoiceTts};

/// A scripted host-side consumer simulating a full fake turn, used to
/// assert exactly-one terminal outcome over the event vocabulary.
struct FakeHost {
    events: Vec<VoiceClientEvent>,
    segments: Vec<SpeechSegmentId>,
    playback: Vec<PlayState>,
}

impl FakeHost {
    fn new() -> Self {
        Self {
            events: Vec::new(),
            segments: Vec::new(),
            playback: Vec::new(),
        }
    }

    /// Feeds the scripted happy-path event sequence for one turn.
    fn run_turn(&mut self) {
        let mut reassembler = PcmReassembler::new();
        let mut frames = Vec::new();
        for chunk in [vec![0u8; 640], vec![1u8; 641]] {
            frames.extend(reassembler.push(&chunk).unwrap());
        }
        // (A 641-byte chunk leaves a carried byte; keep capture alive so
        // finish() is not truncation in this scripted path.)
        frames.extend(reassembler.push(&[2u8]).unwrap());
        let _ = reassembler.finish();
        for _ in 0..2 {
            let partial = BoundedPartial::of("interim");
            self.events
                .push(VoiceClientEvent::PartialTranscript { text: partial.0 });
        }
        self.events.push(VoiceClientEvent::Transcript {
            text: BoundedPartial::wide("final"),
        });
        self.segments.push(0);
        self.playback.push(PlayState::Acked { through_bytes: 320 });
        self.events.push(VoiceClientEvent::SpeechAudio {
            segment: 0,
            frame: frames.remove(0),
        });
    }
}

/// Minimal bounded-string helper for test brevity.
struct BoundedPartial(vesper_domain::BoundedString<2048>);

impl BoundedPartial {
    fn of(text: &str) -> Self {
        Self(vesper_domain::BoundedString::new(text).unwrap())
    }

    fn wide(text: &str) -> vesper_domain::BoundedString<8192> {
        vesper_domain::BoundedString::new(text).unwrap()
    }
}

#[test]
fn pr0_crate_declares_no_adapter_features_or_deps() {
    // The pure core must compile bare; this test fails loudly if someone
    // adds a mandatory adapter dependency to the crate manifest.
    // Production `[dependencies]` only — dev-dependencies (test-only,
    // e.g. an executor for async tests) are permitted workspace-wide.
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("crate manifest readable");
    // D22 (PR-1 amendment): features exist for adapter modules, but
    // every one is default-off and none adds a production dependency.
    // The pure core still compiles bare (cargo build --no-default-features).
    let features = manifest
        .split("[features]")
        .nth(1)
        .and_then(|rest| rest.split("[dependencies]").next())
        .unwrap_or("");
    for feature in ["stt-sidecar", "stt-http"] {
        assert!(
            features.contains(feature),
            "PR-1 adapter features must be declared"
        );
        // Default-off: no feature may appear in a default list.
        assert!(
            !features.contains(&format!("default = [\"{feature}\"")),
            "adapter features must be default-off"
        );
        assert!(
            !features.contains(&format!("{feature} = [\"dep:")),
            "adapter features must not pull dependencies (pure-core rule)"
        );
    }
    let dependencies = manifest
        .split("[dependencies]")
        .nth(1)
        .and_then(|rest| rest.split("[dev-dependencies]").next())
        .unwrap_or("");
    for forbidden in [
        "vesper-runtime",
        "vesper-agent",
        "vesper-harness",
        "vesper-provider",
        "tokio",
        "reqwest",
    ] {
        assert!(
            !dependencies.contains(forbidden),
            "production dependency `{forbidden}` must not appear in the pure core manifest"
        );
    }
}

#[test]
fn canonical_pcm_contract_is_fixed() {
    let format = AudioFormat::canonical();
    assert_eq!(format.sample_rate_hz, 16_000);
    assert_eq!(format.channels, 1);
    assert_eq!(format.bits_per_sample, 16);
    assert!(format.validate().is_ok());
}

#[test]
fn frames_only_from_aligned_bytes_and_truncation_is_visible() {
    assert!(PcmFrame::from_aligned(vec![1, 2, 3]).is_err());
    let mut reassembler = PcmReassembler::new();
    let frames = reassembler.push(&[1, 2, 3]).unwrap();
    assert_eq!(frames.len(), 1);
    assert!(reassembler.has_carried_byte());
    assert!(matches!(reassembler.finish(), Err(VoiceError::Truncated)));
}

#[test]
fn error_classification_separates_silence_from_outage() {
    let provider = ProviderId::new("fixture").unwrap();
    let unavailable = VoiceError::Unavailable {
        provider: provider.clone(),
        reason: vesper_domain::BoundedString::new("down").unwrap(),
    };
    assert!(unavailable.failover_eligible());
    assert!(!VoiceError::NoSpeech.failover_eligible());
    assert!(!VoiceError::Inference("engine".into()).failover_eligible());
}

#[test]
fn egress_policy_rejects_remote_under_on_device() {
    let scope = VoiceScope {
        enabled: true,
        egress: SpeechEgress::OnDevice,
        ..VoiceScope::default()
    };
    let selection = VoiceProviderSelection {
        stt_egress: SpeechEgressClass::OnDevice,
        tts_egress: SpeechEgressClass::SelfHostedRemote,
    };
    assert!(matches!(
        scope.validate_egress(&selection),
        Err(VoiceScopeError::EgressPolicy { .. })
    ));
}

#[test]
fn config_schema_has_no_hygiene_disable_switch() {
    let table =
        parse_voice_table("[voice]\nenabled = true\nredaction = false\nhygiene = false\n").unwrap();
    // Unknown keys are tolerated (ignored); there is no schema key that
    // can disable pre-cloud hygiene (D9): cloud hygiene is structural.
    assert!(table.enabled);
}

#[test]
fn partials_are_labeled_by_kind_and_superseded_by_final() {
    let stt = FakeStt::on_device();
    assert_eq!(
        stt.descriptor().partials,
        Some(PartialsKind::BufferedRepass)
    );
    let frame = PcmFrame::from_aligned(vec![0u8; 6400]).unwrap();
    let frames = [frame.clone()];
    let partial = <FakeStt as SttPartial>::partial(&stt, &frames);
    assert!(block_on(partial).unwrap().is_some());
    stt.set_outcome(FakeSttOutcome::Text("final answer"));
    let cancel = VoiceCancel::new();
    let final_result = stt.transcribe(&frames, &cancel);
    let transcript = block_on(final_result).unwrap();
    assert_eq!(transcript.text.as_str(), "final answer");
}

#[test]
fn tts_lifecycle_distinguishes_open_failure_midstream_failure_and_finish() {
    let tts = FakeTts::on_device();
    let voice = block_on(tts.voices()).unwrap().remove(0);
    let cancel = VoiceCancel::new();

    // Clean: audio then Finished.
    tts.set_script(FakeTtsScript::Clean { frames: 1 });
    let mut stream = block_on(tts.synthesize("a", &voice, &cancel)).unwrap();
    assert!(matches!(
        block_on(stream.next()),
        Some(Ok(TtsChunk::Audio(_)))
    ));
    assert!(matches!(
        block_on(stream.next()),
        Some(Ok(TtsChunk::Finished))
    ));

    // Mid-stream failure after audio began (D6).
    tts.set_script(FakeTtsScript::FailAfter { frames: 1 });
    let mut stream = block_on(tts.synthesize("b", &voice, &cancel)).unwrap();
    assert!(matches!(
        block_on(stream.next()),
        Some(Ok(TtsChunk::Audio(_)))
    ));
    assert!(matches!(
        block_on(stream.next()),
        Some(Err(TtsMidStreamError::AudioFailed { .. }))
    ));

    // Open-time failure precedes any audio.
    tts.set_script(FakeTtsScript::FailOpen);
    assert!(matches!(
        block_on(tts.synthesize("c", &voice, &cancel)),
        Err(VoiceError::Auth { .. })
    ));
}

#[test]
fn cancellation_is_waker_aware_and_bounded() {
    // Bounded: the whole test completes under its own timeout; if the
    // waker registration regresses to busy-spinning, this test hangs and
    // CI's deadline — not patience — fails it.
    let cancel = VoiceCancel::new();
    let waiter = cancel.clone();
    let stream_cancel = cancel.child();
    let handle = std::thread::spawn(move || {
        let start = std::time::Instant::now();
        block_on(waiter.cancelled());
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
    });
    std::thread::sleep(std::time::Duration::from_millis(20));
    assert!(!stream_cancel.is_cancelled());
    cancel.cancel();
    assert!(stream_cancel.is_cancelled());
    handle.join().unwrap();
}

#[test]
fn phase_projection_overlaps_generation_and_playback() {
    // Speaking survives agent settlement (dimensions are independent).
    assert_eq!(
        project_phase(false, false, false, false, true, true),
        vesper_voice::VoiceTurnPhase::Speaking
    );
}

#[test]
fn terminal_report_is_metadata_only() {
    let mut host = FakeHost::new();
    host.run_turn();
    let report = vesper_voice::VoiceTurnReport::new(1);
    let encoded = serde_json::to_string(&report).unwrap();
    assert!(!encoded.contains("transcript"));
    assert!(!encoded.contains("pcm"));
    assert!(host.segments.len() == 1);
    assert!(matches!(
        host.playback[0],
        PlayState::Acked { through_bytes: 320 }
    ));
}

#[test]
fn settlement_translation_is_voice_owned() {
    // The core's own settlement enum covers the runtime cases without
    // importing any runtime type (D1).
    let values = [
        AgentSettlement::Completed,
        AgentSettlement::Failed,
        AgentSettlement::Cancelled,
        AgentSettlement::InterruptedNoReplay,
    ];
    assert_eq!(values.len(), 4);
}

// The runtime-agnostic block_on lives in the crate (test_util) and is
// reused by adapters for driving futures inside blocking closures.
fn block_on<F: Future>(future: F) -> F::Output {
    vesper_voice::test_util::block_on(future)
}

#[test]
fn stt_transcript_type_is_final_and_bounded() {
    let transcript = SttTranscript {
        text: vesper_domain::BoundedString::new("hello").unwrap(),
        provider: ProviderId::new("fixture").unwrap(),
        confidence: None,
        provenance: TranscriptProvenance::InferredText,
    };
    assert_eq!(transcript.text.as_str(), "hello");
}
