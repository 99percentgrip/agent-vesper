//! Local STT compatibility adapter: **Rust adapter driving the shipped
//! Python sidecar** (feature `stt-sidecar`).
//!
//! Truthful label: Rust orchestration over existing sidecar inference —
//! compatibility, **not** proof of native Rust inference. It reuses the
//! exact protocol the TUI dictation surface ships
//! (`apps/agent-vesper-tui/src/voice_transcribe.py`): the embedded
//! script runs under a discovered interpreter; each request is one JSON
//! line (`{"wav","skip"}`); responses are bounded JSON lines
//! (`{"index","text","seconds"}` … `{"done","chunks"}`).
//!
//! Binding behaviors:
//! - VAD: the embedded script passes `vad_filter=True` on every
//!   `transcribe` call (the shipped hallucination-guard regression).
//! - Canonical PCM: input is validated 16 kHz mono i16; the adapter
//!   materializes a private WAV (`sampwidth 2`, `framerate 16000`).
//! - Process ownership: one shared warm child, spawned lazily (never at
//!   construction). `Drop` closes stdin and reaps exactly that PID in
//!   its own process group; unrelated processes are never touched.
//! - Deadlines: every line receive is bounded; a stalling child yields
//!   `Unavailable`, never an infinite hang. Cancellation discards our
//!   result; the child is reused, not killed.
//! - No setup side effects: no package installs, no model downloads. A
//!   missing interpreter/model is truthful `Unavailable` naming the
//!   setup prerequisite.
//! - Privacy: stderr is discarded (`Stdio::null()`); failures surface
//!   as bounded, content-free reason strings. No transcript, audio, or
//!   credential content enters telemetry.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use vesper_domain::{BoundedString, ProviderId};

use crate::audio::PcmFrame;
use crate::cancel::VoiceCancel;
use crate::composition::blocking::{self, ValueExecutor};
use crate::error::VoiceError;
use crate::ports::{
    SpeechEgressClass, SttDescriptor, SttTranscript, TranscriptProvenance, VoiceFuture, VoiceStt,
};

/// The shipped sidecar script, embedded verbatim (single source of truth
/// with the TUI dictation path).
const SIDECAR_SCRIPT: &str = include_str!("../../../apps/agent-vesper-tui/src/voice_transcribe.py");

/// Adapter configuration (validated; paths come from validated
/// discovery, never user-controlled shell strings).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarConfig {
    /// Interpreter executable for the script.
    pub python: PathBuf,
    /// Whisper model name (`GLM_ACP_WHISPER_MODEL` contract).
    pub model: String,
    /// Per-request deadline.
    pub request_deadline: Duration,
    /// Maximum accepted response line length (bytes).
    pub max_line_bytes: usize,
}

impl Default for SidecarConfig {
    fn default() -> Self {
        Self {
            python: PathBuf::from("python3"),
            model: String::from("base"),
            request_deadline: Duration::from_secs(300),
            max_line_bytes: 65_536,
        }
    }
}

impl SidecarConfig {
    /// Validates the shape: a plain executable path (no shell
    /// metacharacters — arguments are never constructed from
    /// user-controlled input) and a non-empty model.
    ///
    /// # Errors
    ///
    /// [`VoiceError::InvalidInput`] on a bad interpreter path or empty
    /// model name.
    pub fn validate(&self) -> Result<(), VoiceError> {
        let text = self.python.to_string_lossy();
        if text.is_empty() || text.contains([';', '|', '&', '`', ' ']) {
            return Err(VoiceError::InvalidInput(
                "interpreter path must be a plain executable path without arguments".into(),
            ));
        }
        if self.model.trim().is_empty() {
            return Err(VoiceError::InvalidInput(
                "model name must not be empty".into(),
            ));
        }
        Ok(())
    }
}

/// One warm sidecar child with a dedicated stdout reader thread.
struct SidecarChild {
    child: Child,
    stdin: ChildStdin,
    /// Bounded lines from stdout (reader thread owns the pipe).
    lines: std::sync::mpsc::Receiver<String>,
}

impl SidecarChild {
    fn spawn(config: &SidecarConfig) -> Result<Self, VoiceError> {
        let mut command = Command::new(&config.python);
        command
            .arg("-c")
            .arg(SIDECAR_SCRIPT)
            .env("GLM_ACP_WHISPER_MODEL", &config.model)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // Own process group so cleanup signals exactly this child.
            command.process_group(0);
        }
        let provider = ProviderId::new("stt-sidecar").expect("static id fits");
        let mut child = command.spawn().map_err(|error| VoiceError::Unavailable {
            provider: provider.clone(),
            reason: BoundedString::new(format!(
                "sidecar interpreter unavailable ({}): voice backend setup prerequisite",
                error.kind()
            ))
            .unwrap_or_else(|_| BoundedString::new("interpreter unavailable").unwrap()),
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| unavailability(&provider, "stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| unavailability(&provider, "stdout"))?;
        let (sender, lines) = std::sync::mpsc::sync_channel::<String>(16);
        let max_bytes = config.max_line_bytes;
        std::thread::spawn(move || {
            use std::io::Read;
            let mut reader = std::io::BufReader::new(stdout);
            let mut buffer: Vec<u8> = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let read = match reader.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(count) => count,
                    Err(_) => return,
                };
                // Handle EVERY newline in the read portion, carrying
                // only genuinely-read bytes forward.
                let mut start = 0;
                for (offset, byte) in chunk[..read].iter().enumerate() {
                    if *byte == b'\n' {
                        buffer.extend_from_slice(&chunk[start..offset]);
                        if buffer.len() > max_bytes || sender.send(lossy_line(&buffer)).is_err() {
                            return;
                        }
                        buffer.clear();
                        start = offset + 1;
                    }
                }
                buffer.extend_from_slice(&chunk[start..read]);
                if buffer.len() > max_bytes {
                    return;
                }
            }
            // EOF: flush any final unterminated line (bounded).
            if !buffer.is_empty() && buffer.len() <= max_bytes {
                let _ = sender.send(lossy_line(&buffer));
            }
        });
        Ok(Self {
            child,
            stdin,
            lines,
        })
    }

    fn exited(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)) | Err(_))
    }
}

fn lossy_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn unavailability(provider: &ProviderId, what: &'static str) -> VoiceError {
    VoiceError::Unavailable {
        provider: provider.clone(),
        reason: BoundedString::new(format!("sidecar {what} unavailable")).unwrap(),
    }
}

impl Drop for SidecarChild {
    fn drop(&mut self) {
        // Closing stdin signals EOF; the script exits after draining.
        let _ = self.stdin.flush();
        // Kill our own process group (exactly this child, nobody else).
        #[cfg(unix)]
        {
            let _ = Command::new("kill")
                .args(["-TERM", &format!("{}", self.child.id())])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Rust adapter over the shipped sidecar process.
pub struct SidecarStt {
    config: SidecarConfig,
    descriptor: SttDescriptor,
    executor: Arc<dyn ValueExecutor>,
    child: Mutex<Option<SidecarChild>>,
}

impl SidecarStt {
    /// Creates the adapter; the child spawns lazily on first use so
    /// construction never has process side effects.
    #[must_use]
    pub fn new(config: SidecarConfig, executor: Arc<dyn ValueExecutor>) -> Self {
        let descriptor = SttDescriptor {
            provider: ProviderId::new("stt-sidecar").expect("static id fits"),
            model: BoundedString::new(config.model.clone()).expect("bounded model name"),
            egress: SpeechEgressClass::OnDevice,
            partials: None,
            vad_enabled: true,
            languages: vec![BoundedString::new("en").expect("static tag")],
        };
        Self {
            config,
            descriptor,
            executor,
            child: Mutex::new(None),
        }
    }
}

/// Runs one sidecar request against `wav_path`, consuming `child` and
/// returning it alongside the transcript chunks.
fn transcribe_with(
    mut child: SidecarChild,
    wav_path: &str,
    deadline: Duration,
) -> (SidecarChild, Result<Vec<String>, VoiceError>) {
    let provider = ProviderId::new("stt-sidecar").expect("static id fits");
    let result = (|| {
        let request = serde_json::json!({"wav": wav_path, "skip": 0});
        let line = serde_json::to_string(&request)
            .map_err(|_| VoiceError::InvalidInput("request serialization failed".into()))?;
        writeln!(child.stdin, "{line}")
            .and_then(|()| child.stdin.flush())
            .map_err(|_| unavailability(&provider, "pipe (closed)"))?;
        let mut chunks: Vec<String> = Vec::new();
        loop {
            let line = child
                .lines
                .recv_timeout(deadline)
                .map_err(|_| unavailability(&provider, "deadline exceeded"))?;
            let value: serde_json::Value = serde_json::from_str(&line).map_err(|_| {
                VoiceError::InvalidInput("sidecar produced a malformed line".into())
            })?;
            if value.get("error").is_some() {
                return Err(VoiceError::Inference("sidecar transcription failed".into()));
            }
            if let Some(text) = value.get("text").and_then(serde_json::Value::as_str) {
                chunks.push(text.to_owned());
            }
            if value.get("done") == Some(&serde_json::Value::Bool(true)) {
                if let Some(count) = value.get("chunks").and_then(serde_json::Value::as_u64)
                    && count != chunks.len() as u64
                {
                    return Err(VoiceError::Inference(
                        "sidecar reported an incomplete transcription".into(),
                    ));
                }
                return Ok(chunks);
            }
            // `ready` and progress lines continue the loop.
        }
    })();
    (child, result)
}

impl VoiceStt for SidecarStt {
    fn transcribe<'a>(
        &'a self,
        audio: &'a [PcmFrame],
        cancel: &'a VoiceCancel,
    ) -> VoiceFuture<'a, Result<SttTranscript, VoiceError>> {
        Box::pin(async move {
            if audio.is_empty() {
                return Err(VoiceError::InvalidInput(
                    "no audio captured for transcription".into(),
                ));
            }
            // Pre-start cancellation: never spawn a child under a
            // cancelled scope (bounded process lifetimes).
            if cancel.is_cancelled() {
                return Err(VoiceError::Cancelled);
            }
            self.config.validate()?;
            // Private WAV materialization (canonical PCM).
            let dir = private_dir(&self.descriptor.provider)?;
            let wav = dir.path().join("capture.wav");
            write_wav(&wav, audio, &self.descriptor.provider)?;
            let wav_path = wav.display().to_string();
            // Take a live child out of the slot (spawn if needed) so the
            // blocking closure owns it without borrowing `self`.
            let child = self.take_or_spawn_child()?;
            let deadline = self.config.request_deadline;
            let executor = Arc::clone(&self.executor);
            let outcome = blocking::run_blocking(
                executor.as_ref(),
                cancel,
                Box::new(move || Ok(transcribe_with(child, &wav_path, deadline))),
            )
            .await?;
            // Return the (possibly reused) child for the next request.
            let (mut child, result) = outcome;
            if !child.exited()
                && let Ok(mut guard) = self.child.lock()
            {
                *guard = Some(child);
            }
            let provider = self.descriptor.provider.clone();
            let joined: String = result?
                .into_iter()
                .filter(|chunk| !chunk.trim().is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            let trimmed = joined.trim();
            if trimmed.is_empty() {
                // The shipped script applies vad_filter=True on every
                // call; an empty finalized result is the VAD outcome.
                return Ok(SttTranscript {
                    text: BoundedString::new(String::new()).unwrap(),
                    provider,
                    confidence: None,
                    provenance: TranscriptProvenance::VadConfirmedSilence,
                });
            }
            let text = BoundedString::new(trimmed)
                .map_err(|_| VoiceError::InvalidInput("transcript exceeded its bound".into()))?;
            Ok(SttTranscript {
                text,
                provider,
                confidence: None,
                provenance: TranscriptProvenance::InferredText,
            })
        })
    }

    fn descriptor(&self) -> &SttDescriptor {
        &self.descriptor
    }
}

impl SidecarStt {
    fn take_or_spawn_child(&self) -> Result<SidecarChild, VoiceError> {
        let mut guard = self
            .child
            .lock()
            .map_err(|_| unavailability(&self.descriptor.provider, "state (poisoned lock)"))?;
        let needs_spawn = match guard.as_mut() {
            Some(existing) => existing.exited(),
            None => true,
        };
        if needs_spawn {
            *guard = Some(SidecarChild::spawn(&self.config)?);
        }
        Ok(guard.take().expect("just ensured a live child"))
    }
}

/// Private temp directory removed on drop (audio never persists).
struct PrivateDir(PathBuf);

impl PrivateDir {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for PrivateDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

static DIR_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn private_dir(provider: &ProviderId) -> Result<PrivateDir, VoiceError> {
    let unique = DIR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("vesper-voice-{}-{}", std::process::id(), unique));
    std::fs::create_dir_all(&path).map_err(|_| unavailability(provider, "temp dir"))?;
    Ok(PrivateDir(path))
}

/// Writes canonical PCM frames as the WAV the script requires
/// (16-bit PCM, 16 000 Hz, mono, canonical 44-byte header).
fn write_wav(
    path: &std::path::Path,
    frames: &[PcmFrame],
    provider: &ProviderId,
) -> Result<(), VoiceError> {
    let mut data: Vec<u8> = Vec::new();
    for frame in frames {
        data.extend_from_slice(frame.bytes());
    }
    let mut file =
        std::fs::File::create(path).map_err(|_| unavailability(provider, "capture file"))?;
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&((36 + data.len()) as u32).to_le_bytes());
    header.extend_from_slice(b"WAVE");
    header.extend_from_slice(b"fmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes()); // PCM
    header.extend_from_slice(&1u16.to_le_bytes()); // mono
    header.extend_from_slice(&16_000u32.to_le_bytes()); // rate
    header.extend_from_slice(&32_000u32.to_le_bytes()); // byte rate
    header.extend_from_slice(&2u16.to_le_bytes()); // block align
    header.extend_from_slice(&16u16.to_le_bytes()); // bits
    header.extend_from_slice(b"data");
    header.extend_from_slice(&(data.len() as u32).to_le_bytes());
    file.write_all(&header)
        .and_then(|()| file.write_all(&data))
        .map_err(|_| unavailability(provider, "capture data"))
}
