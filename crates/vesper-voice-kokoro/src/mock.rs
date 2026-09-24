//! Development-only deterministic synthesis double (feature
//! `mock-synthesis`, test-only). Replays a fixed bounded PCM pattern so
//! red tests of host wiring (Settings → selection → F9 → playback) run
//! without pack assets, the runtime, or any device.
//!
//! **Never a production path**: the type is visibly named `Mock`, its
//! descriptor is marked, and the feature is not part of any production
//! feature graph. It exists so the defining clean-cache UI sequence can
//! be driven with small deterministic assets per directive §9.

use std::sync::Arc;

use futures_core::Stream;
use vesper_domain::{BoundedString, ProviderId};
use vesper_voice::audio::{CANONICAL_SAMPLE_RATE_HZ, PcmFrame};
use vesper_voice::cancel::VoiceCancel;
use vesper_voice::error::{TtsMidStreamError, VoiceError};
use vesper_voice::ports::{
    SpeechEgressClass, TtsChunk, TtsDescriptor, TtsStream, VoiceFuture, VoiceProfile, VoiceTts,
};

/// The visibly-fake deterministic adapter.
pub struct MockSynthesisTts {
    descriptor: TtsDescriptor,
    /// Deterministic unit tone (a short 440 Hz blip at canonical rate).
    frames: Vec<PcmFrame>,
}

impl MockSynthesisTts {
    /// Creates the double with a deterministic sub-second blip.
    #[must_use]
    pub fn new() -> Self {
        // 100 ms of 440 Hz sine at 16 kHz mono s16.
        let mut samples = Vec::with_capacity(1_600);
        for index in 0..1_600 {
            let value = (index as f32 * 0.172_5).sin() * 8_000.0;
            samples.push(value.round().clamp(-32_768.0, 32_767.0) as i16);
        }
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let frames = vec![PcmFrame::from_aligned(bytes).expect("deterministic blip is aligned")];
        Self {
            descriptor: TtsDescriptor {
                provider: ProviderId::new("voice-kokoro-mock").expect("static id fits"),
                model: BoundedString::new("mock-synthesis:fixture:true").expect("fits"),
                egress: SpeechEgressClass::OnDevice,
                streaming: false,
                requires_hygiene: false,
            },
            frames,
        }
    }
}

impl Default for MockSynthesisTts {
    fn default() -> Self {
        Self::new()
    }
}

impl VoiceTts for MockSynthesisTts {
    fn synthesize<'a>(
        &'a self,
        text: &'a str,
        _voice: &'a VoiceProfile,
        cancel: &'a VoiceCancel,
    ) -> VoiceFuture<'a, Result<TtsStream<'a>, VoiceError>> {
        Box::pin(async move {
            if cancel.is_cancelled() {
                return Err(VoiceError::Cancelled);
            }
            if text.trim().is_empty() {
                return Err(VoiceError::InvalidInput("empty text".into()));
            }
            let stream: TtsStream<'a> = Box::pin(MockStream {
                frames: self.frames.clone(),
                index: 0,
                cancel: cancel.clone(),
            });
            Ok(stream)
        })
    }

    fn descriptor(&self) -> &TtsDescriptor {
        &self.descriptor
    }

    fn voices<'a>(&'a self) -> VoiceFuture<'a, Result<Vec<VoiceProfile>, VoiceError>> {
        Box::pin(async move {
            Ok(vec![VoiceProfile {
                voice_id: BoundedString::new("af_heart".to_owned()).expect("fits"),
                label: BoundedString::new("Heart (mock fixture)".to_owned()).expect("fits"),
                sample_rate_hz: CANONICAL_SAMPLE_RATE_HZ,
            }])
        })
    }
}

struct MockStream {
    frames: Vec<PcmFrame>,
    index: usize,
    cancel: VoiceCancel,
}

impl Stream for MockStream {
    type Item = Result<TtsChunk, TtsMidStreamError>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        _context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let this = &mut *self;
        if this.cancel.is_cancelled() {
            return std::task::Poll::Ready(Some(Err(TtsMidStreamError::Cancelled)));
        }
        if this.index >= this.frames.len() {
            return std::task::Poll::Ready(Some(Ok(TtsChunk::Finished)));
        }
        let frame = this.frames[this.index].clone();
        this.index += 1;
        std::task::Poll::Ready(Some(Ok(TtsChunk::Audio(frame))))
    }
}

#[allow(dead_code)]
fn _assert_shared() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MockSynthesisTts>();
    let _ = Arc::new(MockSynthesisTts::new());
}
