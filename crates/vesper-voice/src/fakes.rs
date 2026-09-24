//! Deterministic in-memory fakes for the voice ports (PR-0 only).
//!
//! Following the workspace fake discipline (visible `Fake*` naming,
//! fixture markers, test-only reachability): every type is visibly named
//! `Fake*` and observations carry `"fixture": true` markers, so fake
//! evidence cannot masquerade as adapter evidence.
//! These fakes prove the **contracts** — ordering, budgets, error
//! classification, cancellation visibility, stream lifecycles — not
//! acoustic behavior, provider reliability, or production cancellation.
//! Real adapters (PR-1/PR-2) must pass the same contract tests.

use std::sync::{Arc, Mutex};

use vesper_domain::{BoundedString, ProviderId};

use crate::audio::PcmFrame;
use crate::cancel::VoiceCancel;
use crate::error::{TtsMidStreamError, VoiceError};
use crate::ports::{
    PartialsKind, SpeechEgressClass, SttDescriptor, SttPartial, SttTranscript, TtsChunk,
    TtsDescriptor, TtsStream, VoiceFuture, VoiceProfile, VoiceStt, VoiceTts,
};

/// Scripted behavior for the fake STT's next finalization.
#[derive(Debug, Clone)]
pub enum FakeSttOutcome {
    /// Return this text.
    Text(&'static str),
    /// VAD found no speech (nonfatal).
    NoSpeech,
    /// Provider unavailable (failover-eligible).
    Unavailable,
    /// Engine failure on valid input.
    InferenceFailure,
}

/// Recorded observation of one fake call (metadata only).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FakeObservation {
    /// Marks the observation as fixture evidence, never production.
    pub fixture: bool,
    /// Sample count offered.
    pub samples: usize,
    /// Whether cancellation was already requested at call time.
    pub cancelled_at_entry: bool,
}

/// Deterministic fake STT implementing the port contract.
pub struct FakeStt {
    descriptor: SttDescriptor,
    outcome: Mutex<FakeSttOutcome>,
    partial_calls: Mutex<Vec<usize>>,
    observations: Mutex<Vec<FakeObservation>>,
}

impl FakeStt {
    /// A fake on-device Whisper-class STT with buffered-repass partials.
    pub fn on_device() -> Self {
        Self {
            descriptor: SttDescriptor {
                provider: ProviderId::new("fake-stt").unwrap(),
                model: BoundedString::new("fixture-model").unwrap(),
                egress: SpeechEgressClass::OnDevice,
                partials: Some(PartialsKind::BufferedRepass),
                vad_enabled: true,
                languages: vec![BoundedString::new("en").unwrap()],
            },
            outcome: Mutex::new(FakeSttOutcome::Text("fixture transcript")),
            partial_calls: Mutex::new(Vec::new()),
            observations: Mutex::new(Vec::new()),
        }
    }

    /// Scripts the next finalization outcome.
    pub fn set_outcome(&self, outcome: FakeSttOutcome) {
        *self.outcome.lock().unwrap() = outcome;
    }

    /// Recorded final-call observations.
    pub fn observations(&self) -> Vec<FakeObservation> {
        self.observations.lock().unwrap().clone()
    }

    /// Recorded partial-call sample counts.
    pub fn partial_calls(&self) -> Vec<usize> {
        self.partial_calls.lock().unwrap().clone()
    }
}

impl VoiceStt for FakeStt {
    fn transcribe<'a>(
        &'a self,
        audio: &'a [PcmFrame],
        cancel: &'a VoiceCancel,
    ) -> VoiceFuture<'a, Result<SttTranscript, VoiceError>> {
        let samples: usize = audio.iter().map(PcmFrame::sample_count).sum();
        self.observations.lock().unwrap().push(FakeObservation {
            fixture: true,
            samples,
            cancelled_at_entry: cancel.is_cancelled(),
        });
        let outcome = self.outcome.lock().unwrap().clone();
        let provider = self.descriptor.provider.clone();
        Box::pin(async move {
            if cancel.is_cancelled() {
                return Err(VoiceError::Cancelled);
            }
            match outcome {
                FakeSttOutcome::Text(text) => Ok(SttTranscript {
                    text: BoundedString::new(text).unwrap(),
                    provider,
                    confidence: None,
                    provenance: crate::ports::TranscriptProvenance::InferredText,
                }),
                FakeSttOutcome::NoSpeech => Err(VoiceError::NoSpeech),
                FakeSttOutcome::Unavailable => Err(VoiceError::Unavailable {
                    provider,
                    reason: BoundedString::new("fixture offline").unwrap(),
                }),
                FakeSttOutcome::InferenceFailure => {
                    Err(VoiceError::Inference("fixture engine failure".into()))
                }
            }
        })
    }

    fn partial(&self) -> Option<&dyn SttPartial> {
        Some(self)
    }

    fn descriptor(&self) -> &SttDescriptor {
        &self.descriptor
    }
}

impl SttPartial for FakeStt {
    fn partial<'a>(
        &'a self,
        audio: &'a [PcmFrame],
    ) -> VoiceFuture<'a, Result<Option<BoundedString<2048>>, VoiceError>> {
        let samples: usize = audio.iter().map(PcmFrame::sample_count).sum();
        self.partial_calls.lock().unwrap().push(samples);
        // Best-effort contract: a partial pass never fails the turn.
        let text = if samples >= 1600 {
            Some(BoundedString::new("fixture partial…").unwrap())
        } else {
            None
        };
        Box::pin(async move { Ok(text) })
    }
}

/// Scripted mid-stream failure point for the fake TTS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeTtsScript {
    /// Stream all frames then finish cleanly.
    Clean { frames: usize },
    /// Fail after `frames` audio frames (mid-stream error, D6).
    FailAfter { frames: usize },
    /// Fail at open time (auth/quota/unavailable-class error).
    FailOpen,
    /// Never emit anything; wait for cancellation.
    HangUntilCancelled,
}

/// Deterministic fake TTS implementing the full stream lifecycle.
pub struct FakeTts {
    descriptor: TtsDescriptor,
    script: Mutex<FakeTtsScript>,
    opened_texts: Mutex<Vec<String>>,
}

impl FakeTts {
    /// A fake on-device streaming TTS.
    pub fn on_device() -> Self {
        Self {
            descriptor: TtsDescriptor {
                provider: ProviderId::new("fake-tts").unwrap(),
                model: BoundedString::new("fixture-voice").unwrap(),
                egress: SpeechEgressClass::OnDevice,
                streaming: true,
                requires_hygiene: false,
            },
            script: Mutex::new(FakeTtsScript::Clean { frames: 2 }),
            opened_texts: Mutex::new(Vec::new()),
        }
    }

    /// Scripts the next synthesis stream.
    pub fn set_script(&self, script: FakeTtsScript) {
        *self.script.lock().unwrap() = script;
    }

    /// Texts the fake was asked to open.
    pub fn opened_texts(&self) -> Vec<String> {
        self.opened_texts.lock().unwrap().clone()
    }
}

/// One scripted stream instance.
pub struct FakeTtsStream {
    script: FakeTtsScript,
    emitted: usize,
    cancel: Arc<VoiceCancel>,
}

impl futures_core::Stream for FakeTtsStream {
    type Item = Result<TtsChunk, TtsMidStreamError>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let this = &mut *self;
        if let std::task::Poll::Ready(()) = this.cancel.poll_cancelled(cx) {
            return std::task::Poll::Ready(Some(Err(TtsMidStreamError::Cancelled)));
        }
        match this.script {
            FakeTtsScript::Clean { frames } => {
                if this.emitted >= frames {
                    std::task::Poll::Ready(Some(Ok(TtsChunk::Finished)))
                } else {
                    this.emitted += 1;
                    std::task::Poll::Ready(Some(Ok(TtsChunk::Audio(fixture_frame()))))
                }
            }
            FakeTtsScript::FailAfter { frames } => {
                if this.emitted >= frames {
                    std::task::Poll::Ready(Some(Err(TtsMidStreamError::AudioFailed {
                        reason: BoundedString::new("fixture mid-stream failure").unwrap(),
                    })))
                } else {
                    this.emitted += 1;
                    std::task::Poll::Ready(Some(Ok(TtsChunk::Audio(fixture_frame()))))
                }
            }
            FakeTtsScript::FailOpen | FakeTtsScript::HangUntilCancelled => {
                // FailOpen never yields a stream; a hung stream only ends
                // by cancellation. Pending registers the caller's waker
                // via poll_cancelled above, so cancel() re-polls us.
                std::task::Poll::Pending
            }
        }
    }
}

/// Fixture audio frame (320 bytes = 10 ms).
fn fixture_frame() -> PcmFrame {
    PcmFrame::from_aligned(vec![0u8; 320]).unwrap()
}

impl VoiceTts for FakeTts {
    fn synthesize<'a>(
        &'a self,
        text: &'a str,
        _voice: &'a VoiceProfile,
        cancel: &'a VoiceCancel,
    ) -> VoiceFuture<'a, Result<TtsStream<'a>, VoiceError>> {
        self.opened_texts.lock().unwrap().push(text.to_owned());
        let script = *self.script.lock().unwrap();
        let cancel = Arc::new(cancel.child());
        Box::pin(async move {
            match script {
                FakeTtsScript::FailOpen => Err(VoiceError::Auth {
                    provider: ProviderId::new("fake-tts").unwrap(),
                }),
                _ => {
                    let stream: TtsStream<'a> = Box::pin(FakeTtsStream {
                        script,
                        emitted: 0,
                        cancel,
                    });
                    Ok(stream)
                }
            }
        })
    }

    fn descriptor(&self) -> &TtsDescriptor {
        &self.descriptor
    }

    fn voices<'a>(&'a self) -> VoiceFuture<'a, Result<Vec<VoiceProfile>, VoiceError>> {
        Box::pin(async move {
            Ok(vec![VoiceProfile {
                voice_id: BoundedString::new("fixture-voice").unwrap(),
                label: BoundedString::new("Fixture Voice").unwrap(),
                sample_rate_hz: crate::audio::CANONICAL_SAMPLE_RATE_HZ,
            }])
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    #[tokio::test]
    async fn fake_stt_final_supersedes_and_classifies() {
        let stt = FakeStt::on_device();
        let frame = PcmFrame::from_aligned(vec![0u8; 3200]).unwrap();
        let cancel = VoiceCancel::new();
        let transcript = stt
            .transcribe(std::slice::from_ref(&frame), &cancel)
            .await
            .unwrap();
        assert_eq!(transcript.text.as_str(), "fixture transcript");
        stt.set_outcome(FakeSttOutcome::NoSpeech);
        assert!(matches!(
            stt.transcribe(std::slice::from_ref(&frame), &cancel).await,
            Err(VoiceError::NoSpeech)
        ));
        stt.set_outcome(FakeSttOutcome::Unavailable);
        let error = stt
            .transcribe(std::slice::from_ref(&frame), &cancel)
            .await
            .unwrap_err();
        assert!(error.failover_eligible());
        stt.set_outcome(FakeSttOutcome::InferenceFailure);
        let error = stt
            .transcribe(std::slice::from_ref(&frame), &cancel)
            .await
            .unwrap_err();
        assert!(!error.failover_eligible());
        assert!(
            stt.observations()
                .iter()
                .all(|observation| observation.fixture)
        );
    }

    #[tokio::test]
    async fn fake_stt_cancellation_is_visible() {
        let stt = FakeStt::on_device();
        let cancel = VoiceCancel::new();
        cancel.cancel();
        let frame = PcmFrame::from_aligned(vec![0u8; 320]).unwrap();
        assert!(matches!(
            stt.transcribe(&[frame], &cancel).await,
            Err(VoiceError::Cancelled)
        ));
        assert!(stt.observations()[0].cancelled_at_entry);
    }

    #[tokio::test]
    async fn fake_stt_partial_is_best_effort() {
        let stt = FakeStt::on_device();
        let short: Vec<PcmFrame> = vec![PcmFrame::from_aligned(vec![0u8; 320]).unwrap()];
        let long: Vec<PcmFrame> = vec![PcmFrame::from_aligned(vec![0u8; 6400]).unwrap()];
        assert!(
            <FakeStt as SttPartial>::partial(&stt, &short)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            <FakeStt as SttPartial>::partial(&stt, &long)
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(stt.partial_calls(), vec![160, 3200]);
    }

    #[tokio::test]
    async fn fake_tts_clean_stream_finishes() {
        let tts = FakeTts::on_device();
        tts.set_script(FakeTtsScript::Clean { frames: 2 });
        let voice = tts.voices().await.unwrap().remove(0);
        let cancel = VoiceCancel::new();
        let mut stream = tts.synthesize("hello", &voice, &cancel).await.unwrap();
        assert!(matches!(stream.next().await, Some(Ok(TtsChunk::Audio(_)))));
        assert!(matches!(stream.next().await, Some(Ok(TtsChunk::Audio(_)))));
        assert!(matches!(stream.next().await, Some(Ok(TtsChunk::Finished))));
        assert_eq!(tts.opened_texts(), vec!["hello".to_owned()]);
    }

    #[tokio::test]
    async fn fake_tts_midstream_failure_is_distinct() {
        let tts = FakeTts::on_device();
        tts.set_script(FakeTtsScript::FailAfter { frames: 1 });
        let voice = tts.voices().await.unwrap().remove(0);
        let cancel = VoiceCancel::new();
        let mut stream = tts.synthesize("hello", &voice, &cancel).await.unwrap();
        assert!(matches!(stream.next().await, Some(Ok(TtsChunk::Audio(_)))));
        assert!(matches!(
            stream.next().await,
            Some(Err(TtsMidStreamError::AudioFailed { .. }))
        ));
    }

    #[tokio::test]
    async fn fake_tts_open_failure_precedes_audio() {
        let tts = FakeTts::on_device();
        tts.set_script(FakeTtsScript::FailOpen);
        let voice = tts.voices().await.unwrap().remove(0);
        let cancel = VoiceCancel::new();
        assert!(matches!(
            tts.synthesize("hello", &voice, &cancel).await,
            Err(VoiceError::Auth { .. })
        ));
    }

    #[tokio::test]
    async fn fake_tts_cancellation_mid_stream() {
        let tts = FakeTts::on_device();
        tts.set_script(FakeTtsScript::Clean { frames: 5 });
        let voice = tts.voices().await.unwrap().remove(0);
        let cancel = VoiceCancel::new();
        let mut stream = tts.synthesize("hello", &voice, &cancel).await.unwrap();
        assert!(matches!(stream.next().await, Some(Ok(TtsChunk::Audio(_)))));
        cancel.cancel();
        assert!(matches!(
            stream.next().await,
            Some(Err(TtsMidStreamError::Cancelled))
        ));
    }

    #[tokio::test]
    async fn fake_tts_hang_ends_on_cancel() {
        let tts = FakeTts::on_device();
        tts.set_script(FakeTtsScript::HangUntilCancelled);
        let voice = tts.voices().await.unwrap().remove(0);
        let cancel = VoiceCancel::new();
        let mut stream = tts.synthesize("hello", &voice, &cancel).await.unwrap();
        let canceller = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            canceller.cancel();
        });
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
            .await
            .expect("waker-aware cancel must end the hung stream");
        assert!(matches!(outcome, Some(Err(TtsMidStreamError::Cancelled))));
    }

    #[test]
    fn descriptors_declare_capabilities_honestly() {
        let stt = FakeStt::on_device();
        assert_eq!(
            stt.descriptor().partials,
            Some(PartialsKind::BufferedRepass)
        );
        assert!(stt.descriptor().vad_enabled);
        let tts = FakeTts::on_device();
        assert!(tts.descriptor().streaming);
        assert!(!tts.descriptor().requires_hygiene);
    }
}
