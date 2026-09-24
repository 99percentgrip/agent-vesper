//! VRO-17 PR-4: the single shared STT adapter per TUI process.
//!
//! Wraps the shipped sidecar script/venv (the same one dictation
//! bootstraps) behind the [`vesper_voice::ports::VoiceStt`] port so
//! conversation mode reuses one model instance, one environment, and
//! the PR-1 error/provenance/VAD contracts. Request cancellation is
//! per-call; the warm child's lifetime is the process's (no second
//! sidecar is ever spawned for conversation).

use std::sync::Arc;

use vesper_voice::audio::PcmFrame;
use vesper_voice::cancel::VoiceCancel;
use vesper_voice::composition::blocking::{ThreadPoolExecutor, ValueExecutor};
use vesper_voice::error::VoiceError;
use vesper_voice::ports::{SttDescriptor, SttTranscript, VoiceFuture, VoiceStt};
use vesper_voice::stt_sidecar::{SidecarConfig, SidecarStt};

/// The shared sidecar-backed STT adapter (one per process).
pub struct SharedSidecarStt {
    inner: SidecarStt,
}

impl SharedSidecarStt {
    /// Creates the shared adapter over the harness voice venv's Python
    /// (the exact interpreter dictation probes/boots) and the default
    /// cached model. Construction spawns nothing; the warm child starts
    /// lazily on first transcription (PR-1 behavior).
    #[must_use]
    pub fn new() -> Self {
        let python = shared_voice_python();
        Self {
            inner: SidecarStt::new(
                SidecarConfig {
                    python,
                    model: std::env::var("AGENT_VESPER_WHISPER_MODEL")
                        .unwrap_or_else(|_| "base".into()),
                    ..SidecarConfig::default()
                },
                shared_executor(),
            ),
        }
    }
}

impl Default for SharedSidecarStt {
    fn default() -> Self {
        Self::new()
    }
}

impl VoiceStt for SharedSidecarStt {
    fn transcribe<'a>(
        &'a self,
        audio: &'a [PcmFrame],
        cancel: &'a VoiceCancel,
    ) -> VoiceFuture<'a, Result<SttTranscript, VoiceError>> {
        self.inner.transcribe(audio, cancel)
    }
    fn descriptor(&self) -> &SttDescriptor {
        self.inner.descriptor()
    }
}

/// The harness voice venv interpreter (same resolution order as the
/// dictation worker's `candidate_whisper_pythons`, first hit) — never
/// a second environment.
fn shared_voice_python() -> std::path::PathBuf {
    // Same precedence tier the dictation F5 path resolves
    // (`VESPER_PYTHON_PATH`, then `GLM_VENV_PATH`, then the harness
    // voice venv): the FLM-integration rewiring of the conversation
    // CPU adapter dropped the env tiers, so isolated runs and explicit
    // overrides silently fell back to the installed venv. Existence
    // checks only — the child's model load happens lazily on first
    // transcription exactly as before.
    if let Some(path) = std::env::var_os("VESPER_PYTHON_PATH") {
        let candidate = std::path::PathBuf::from(path);
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Some(venv) = std::env::var_os("GLM_VENV_PATH") {
        let candidate = std::path::PathBuf::from(venv).join("bin").join("python");
        if candidate.is_file() {
            return candidate;
        }
    }
    let candidate = crate::voice_venv_root().join("bin").join("python");
    if candidate.is_file() {
        return candidate;
    }
    std::path::PathBuf::from("python3")
}

/// One small shared blocking pool for the process's voice work.
fn shared_executor() -> Arc<dyn ValueExecutor> {
    static POOL: std::sync::OnceLock<Arc<ThreadPoolExecutor>> = std::sync::OnceLock::new();
    let pool = POOL.get_or_init(|| Arc::new(ThreadPoolExecutor::new(2)));
    let erased: Arc<dyn ValueExecutor> = Arc::clone(pool) as Arc<dyn ValueExecutor>;
    erased
}
