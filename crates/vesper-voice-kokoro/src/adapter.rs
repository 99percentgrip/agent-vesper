//! The production `VoiceTts` adapter (feature `ort`): wiring the pure
//! core's port to the engine with the synthesis lifecycle contract —
//! open-time failure vs mid-stream, bounded streaming, cancellation
//! between/around non-preemptible runs, and honest descriptors.

use std::path::PathBuf;
use std::sync::Arc;

use futures_core::Stream;
use vesper_domain::{BoundedString, ProviderId};
use vesper_voice::audio::CANONICAL_SAMPLE_RATE_HZ;
use vesper_voice::cancel::VoiceCancel;
use vesper_voice::composition::blocking::{self, ValueExecutor};
use vesper_voice::error::{TtsMidStreamError, VoiceError};
use vesper_voice::ports::{
    SpeechEgressClass, TtsChunk, TtsDescriptor, TtsStream, VoiceFuture, VoiceProfile, VoiceTts,
};

use crate::engine::{EngineState, KokoroEngine, StylePack, resolve_phonemizer};
use crate::pack;

/// The engine handle behind one adapter. Either loaded (shared by voices
/// and preview for the process lifetime) or a named problem.
enum Engine {
    Ready(Arc<KokoroEngine>),
    NotReady(crate::PackProblem),
    NotAttempted,
}

/// One configured adapter over the managed pack.
pub struct KokoroTts {
    pack_root: PathBuf,
    executor: Arc<dyn ValueExecutor>,
    descriptor: TtsDescriptor,
    engine: std::sync::Mutex<Engine>,
    /// Serializes synthesis work per adapter (one blocking inference at
    /// a time; the guard is taken inside the blocking closure).
    lock: Arc<std::sync::Mutex<()>>,
}

impl KokoroTts {
    /// Creates the adapter (no engine load yet: the first synthesis or
    /// explicit readiness call initializes lazily, keeping process
    /// startup free of pack/runtime work).
    ///
    /// # Errors
    /// [`VoiceError::InvalidInput`] never in practice (configuration is
    /// static); kept for port symmetry.
    pub fn new(executor: Arc<dyn ValueExecutor>) -> Result<Self, VoiceError> {
        Ok(Self::with_pack_root(pack::pack_root(), executor))
    }

    /// Explicit pack-root variant (tests and the managed cache override;
    /// production uses the shared per-user root).
    #[must_use]
    pub fn with_pack_root(pack_root: PathBuf, executor: Arc<dyn ValueExecutor>) -> Self {
        let descriptor = TtsDescriptor {
            provider: ProviderId::new(pack::PROVIDER_ID).expect("static id fits"),
            model: BoundedString::new(format!(
                "kokoro-82m:{}:{}",
                pack::PINNED_REVISION
                    .get(..7)
                    .unwrap_or(pack::PINNED_REVISION),
                pack::ASSET_MODEL.installed_name
            ))
            .unwrap_or_else(|_| BoundedString::new("kokoro-82m").expect("fits")),
            egress: SpeechEgressClass::OnDevice,
            // Honest advertisement: synthesis is buffered per unit — the
            // graph returns the full waveform for the unit's phonemes.
            // Sentence-gated dispatch (the existing hygiene gate) is what
            // bounds latency; this is NOT incremental synthesis.
            streaming: false,
            requires_hygiene: false,
        };
        Self {
            pack_root,
            executor,
            descriptor,
            engine: std::sync::Mutex::new(Engine::NotAttempted),
            lock: Arc::new(std::sync::Mutex::new(())),
        }
    }

    /// The current engine state, loading on first use. A load attempt is
    /// remembered until the pack identity changes (no reload on every
    /// Settings render — the caller invalidates via [`KokoroTts::invalidate`]).
    fn engine(&self) -> Result<Arc<KokoroEngine>, VoiceError> {
        let mut guard = self.engine.lock().map_err(|_| VoiceError::Unavailable {
            provider: crate::provider_id(),
            reason: BoundedString::new("adapter state poisoned").expect("fits"),
        })?;
        match &*guard {
            Engine::Ready(engine) => return Ok(Arc::clone(engine)),
            Engine::NotReady(problem) => {
                return Err(VoiceError::Unavailable {
                    provider: crate::provider_id(),
                    reason: BoundedString::new(problem.description()).unwrap_or_else(|_| {
                        BoundedString::new("voice pack not ready").expect("fits")
                    }),
                });
            }
            Engine::NotAttempted => {}
        }
        match EngineState::load(&self.pack_root)? {
            EngineState::Ready(engine) => {
                *guard = Engine::Ready(Arc::clone(&engine));
                Ok(engine)
            }
            EngineState::NotReady(problem) => {
                let error = VoiceError::Unavailable {
                    provider: crate::provider_id(),
                    reason: BoundedString::new(problem.description()).unwrap_or_else(|_| {
                        BoundedString::new("voice pack not ready").expect("fits")
                    }),
                };
                *guard = Engine::NotReady(problem);
                Err(error)
            }
        }
    }

    /// Prepares the verified inference session without synthesizing audio.
    /// Call only on a background worker after voice activation.
    ///
    /// # Errors
    /// Propagates pack verification and runtime loading failures.
    pub fn prepare(&self) -> Result<(), VoiceError> {
        self.engine().map(|_| ())
    }

    /// Invalidates the remembered engine state (called when the pack
    /// identity changes: install, repair, removal). The next synthesis
    /// reloads — never automatically, never per redraw.
    pub fn invalidate(&self) {
        if let Ok(mut guard) = self.engine.lock() {
            *guard = Engine::NotAttempted;
        }
    }

    /// Whether the pack currently verifies (cheap identity gate first;
    /// full digest check only when the identity changed since the last
    /// verified check). Readiness plumbing for the shared assessment.
    ///
    /// # Errors
    /// Propagates [`pack::assess`] component failures.
    pub fn assess_readiness(&self) -> Result<Option<crate::PackProblem>, VoiceError> {
        pack::assess(&self.pack_root)
    }

    /// Blocking synthesis body executed on the bounded executor.
    fn synthesize_blocking(
        engine: &KokoroEngine,
        text: &str,
        voice: &StylePack,
    ) -> Result<crate::engine::SynthesisOutput, VoiceError> {
        let Some(espeak) = resolve_phonemizer() else {
            return Err(VoiceError::Unavailable {
                provider: crate::provider_id(),
                reason: BoundedString::new("pronunciation engine (espeak-ng) is unavailable")
                    .unwrap_or_else(|_| BoundedString::new("phonemizer missing").expect("fits")),
            });
        };
        engine.synthesize_blocking(text, voice, &espeak)
    }
}

impl VoiceTts for KokoroTts {
    fn synthesize<'a>(
        &'a self,
        text: &'a str,
        voice: &'a VoiceProfile,
        cancel: &'a VoiceCancel,
    ) -> VoiceFuture<'a, Result<TtsStream<'a>, VoiceError>> {
        Box::pin(async move {
            if cancel.is_cancelled() {
                return Err(VoiceError::Cancelled);
            }
            if text.trim().is_empty() {
                return Err(VoiceError::InvalidInput(
                    "refusing to synthesize empty text".into(),
                ));
            }
            if text.len() > crate::phonemize::MAX_TEXT_BYTES {
                return Err(VoiceError::ResourceExhausted(
                    "synthesis text exceeded its bound".into(),
                ));
            }
            let engine = self.engine()?;
            let style = StylePack::load(&self.pack_root, voice.voice_id.as_str())?;
            let owned_text = text.to_owned();
            let engine_ref = Arc::clone(&engine);
            let lock = Arc::clone(&self.lock);
            let executor = Arc::clone(&self.executor);
            let cancel_for_run = cancel.clone();
            let cancel_for_task = cancel.clone();
            // One bounded blocking run; cancellation before the run is
            // honored here, mid-run cancellation surfaces between items
            // (ORT runs are non-preemptible — documented, not hidden).
            let output = blocking::run_blocking(
                executor.as_ref(),
                &cancel_for_run,
                Box::new(move || {
                    let _guard = lock.lock().map_err(|_| VoiceError::Unavailable {
                        provider: crate::provider_id(),
                        reason: BoundedString::new("synthesis lock poisoned").expect("fits"),
                    })?;
                    if cancel_for_task.is_cancelled() {
                        return Err(VoiceError::Cancelled);
                    }
                    Self::synthesize_blocking(&engine_ref, &owned_text, &style)
                }),
            )
            .await?;
            let stream: TtsStream<'a> = Box::pin(FrameStream {
                frames: output.frames,
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
        // Catalog-verified voices only: the two installed pack voices
        // with display labels; never invented entries.
        let pack_ready = pack::assess(&self.pack_root).ok().is_none();
        let profiles: Vec<VoiceProfile> = if pack_ready {
            vec![
                VoiceProfile {
                    voice_id: BoundedString::new("af_heart".to_owned()).expect("fits"),
                    label: BoundedString::new("Heart (US English, female)".to_owned())
                        .expect("fits"),
                    sample_rate_hz: CANONICAL_SAMPLE_RATE_HZ,
                },
                VoiceProfile {
                    voice_id: BoundedString::new("am_michael".to_owned()).expect("fits"),
                    label: BoundedString::new("Michael (US English, male)".to_owned())
                        .expect("fits"),
                    sample_rate_hz: CANONICAL_SAMPLE_RATE_HZ,
                },
            ]
        } else {
            Vec::new()
        };
        Box::pin(async move { Ok(profiles) })
    }
}

/// Yields buffered frames with cancellation checked between items; a
/// cancel suppresses the remaining (stale) audio. The engine already
/// ran; stopping here discards its result honestly (no fabricated PCM).
struct FrameStream {
    frames: Vec<vesper_voice::audio::PcmFrame>,
    index: usize,
    cancel: VoiceCancel,
}

impl Stream for FrameStream {
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use vesper_voice::composition::blocking::ThreadPoolExecutor;

    fn adapter_in(root: &Path) -> KokoroTts {
        KokoroTts::with_pack_root(root.to_path_buf(), Arc::new(ThreadPoolExecutor::new(1)))
    }

    fn profile(voice_id: &str) -> VoiceProfile {
        VoiceProfile {
            voice_id: BoundedString::new(voice_id.to_owned()).expect("fits"),
            label: BoundedString::new("test".to_owned()).expect("fits"),
            sample_rate_hz: CANONICAL_SAMPLE_RATE_HZ,
        }
    }

    #[test]
    fn descriptor_is_truthful_about_buffered_synthesis() {
        let adapter = adapter_in(Path::new("/nonexistent-pack"));
        assert!(!adapter.descriptor().streaming);
        assert_eq!(adapter.descriptor().egress, SpeechEgressClass::OnDevice);
        assert_eq!(adapter.descriptor().provider.as_str(), "voice-kokoro");
    }

    #[tokio::test]
    async fn missing_pack_is_unavailable_with_setup_guidance() {
        let adapter = adapter_in(Path::new("/nonexistent-pack"));
        let cancel = VoiceCancel::new();
        let error = match adapter
            .synthesize("Understood.", &profile("af_heart"), &cancel)
            .await
        {
            Err(error) => error,
            Ok(_) => panic!("must refuse without a pack"),
        };
        assert!(matches!(error, VoiceError::Unavailable { .. }));
        if let VoiceError::Unavailable { reason, .. } = error {
            assert!(reason.as_str().contains("not installed"), "{reason}");
        }
    }

    #[tokio::test]
    async fn unknown_voice_is_invalid_input_not_unavailable() {
        // Pack directory exists but is empty: engine load fails first —
        // so use an engine-ready probe via the StylePack seam instead.
        let dir = tempfile::tempdir().expect("tempdir");
        let error = StylePack::load(dir.path(), "af_notreal").expect_err("must refuse");
        assert!(matches!(error, VoiceError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn empty_text_refuses_before_engine_load() {
        let adapter = adapter_in(Path::new("/nonexistent-pack"));
        let cancel = VoiceCancel::new();
        let error = match adapter
            .synthesize("   ", &profile("af_heart"), &cancel)
            .await
        {
            Err(error) => error,
            Ok(_) => panic!("must refuse empty text"),
        };
        assert!(matches!(error, VoiceError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn cancelled_request_never_starts_work() {
        let adapter = adapter_in(Path::new("/nonexistent-pack"));
        let cancel = VoiceCancel::new();
        cancel.cancel();
        let error = match adapter
            .synthesize("Understood.", &profile("af_heart"), &cancel)
            .await
        {
            Err(error) => error,
            Ok(_) => panic!("must refuse cancelled work"),
        };
        assert!(matches!(error, VoiceError::Cancelled));
    }

    #[tokio::test]
    async fn voices_catalog_is_exactly_the_supported_pair() {
        // With no pack, the catalog is empty (never fabricated); with a
        // verified pack it is the two voices (covered by pack tests).
        let adapter = adapter_in(Path::new("/nonexistent-pack"));
        let cancel = VoiceCancel::new();
        let voices = adapter.voices().await.expect("catalog");
        assert!(voices.is_empty(), "no pack → no fabricated voices");
        let _ = cancel;
    }
}
