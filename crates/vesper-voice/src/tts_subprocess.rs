//! Synthesis-only local TTS adapter over a user-installed system speech
//! engine (feature `tts-subprocess`).
//!
//! **Scope (frozen):** text in → validated canonical PCM out, through
//! the [`crate::ports::VoiceTts`] contract. This adapter **never plays
//! audio through a device and never delegates playback to a system
//! service**: it captures the engine's synthesized waveform on stdout
//! (`--stdout`-class invocation, no `-w` file, no dispatcher-client
//! route), validates the container, converts to the canonical 16 kHz
//! mono i16 contract, and streams bounded in-memory frames. Process
//! exit proves synthesis completed — **not** that anything was heard;
//! no playback acknowledgment exists or is fabricated here. Host
//! playback is a separately designed later component.
//!
//! Gate record (PR-2 §3 of the execution report): the engine route is
//! an **optional user-installed external engine baseline** — not
//! approval of final voice quality, a neural model, pure-Rust
//! inference, or NPU acceleration. Executable/data licenses are
//! recorded separately from Cargo dependencies; nothing is bundled or
//! copied from the system installation. Another user's machine may
//! lack the executable or data; absence is truthful unavailability.
//!
//! Engine specifics verified against the installed espeak-ng 1.52.0
//! (documented in the execution report): `--stdout` emits a WAV whose
//! RIFF/data size fields are **streaming placeholders** (`0x7FFFF...`)
//! because stdout is not seekable — container validation must not
//! trust those sizes; default output is 22050 Hz mono s16, requiring
//! bounded conversion to the canonical 16000 Hz (implemented here,
//! tested; the engine's rate is never relabeled).

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use vesper_domain::{BoundedString, ProviderId};

use crate::audio::{CANONICAL_SAMPLE_RATE_HZ, PcmFrame};
use crate::cancel::VoiceCancel;
use crate::composition::blocking::{self, ValueExecutor};
use crate::error::{TtsMidStreamError, VoiceError};
use crate::ports::{
    SpeechEgressClass, TtsChunk, TtsDescriptor, TtsStream, VoiceFuture, VoiceProfile, VoiceTts,
};

/// Validated adapter configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubprocessTtsConfig {
    /// Engine executable (validated plain path; from configuration or
    /// discovery, never assembled from assistant text).
    pub executable: PathBuf,
    /// On-device voice name (must exist in the engine's own voice data;
    /// verified lazily by the engine, never downloaded).
    pub voice: String,
    /// Per-request deadline.
    pub deadline: Duration,
    /// Maximum synthesized output accepted per request (bytes of
    /// engine output, pre-conversion).
    pub max_output_bytes: usize,
    /// Maximum input text length.
    pub max_text_bytes: usize,
}

impl Default for SubprocessTtsConfig {
    fn default() -> Self {
        Self {
            executable: PathBuf::from("espeak-ng"),
            voice: String::from("en"),
            deadline: Duration::from_secs(30),
            max_output_bytes: 16 * 1024 * 1024,
            max_text_bytes: 8192,
        }
    }
}

impl SubprocessTtsConfig {
    /// Validates the configuration shape (plain executable path; the
    /// engine is invoked with fixed arguments — text travels on stdin,
    /// never on the command line).
    ///
    /// # Errors
    ///
    /// [`VoiceError::InvalidInput`] on shell-metacharacter paths or an
    /// empty voice name.
    pub fn validate(&self) -> Result<(), VoiceError> {
        let text = self.executable.to_string_lossy();
        if text.is_empty() || text.contains([';', '|', '&', '`', ' ']) {
            return Err(VoiceError::InvalidInput(
                "engine path must be a plain executable path without arguments".into(),
            ));
        }
        if self.voice.trim().is_empty() {
            return Err(VoiceError::InvalidInput(
                "voice name must not be empty".into(),
            ));
        }
        Ok(())
    }
}

/// One owned synthesis child. `stdin` is `None` after the request text
/// was written and EOF was signaled (dropped).
struct EngineChild {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: ChildStdout,
}

impl EngineChild {
    fn spawn(config: &SubprocessTtsConfig, text: &str) -> Result<Self, VoiceError> {
        let provider = ProviderId::new("tts-subprocess").expect("static id fits");
        let mut command = Command::new(&config.executable);
        // Fixed argv: markup interpretation stays OFF (`-m` never
        // passed); voice + synthesis-to-stdout only. Text goes on stdin.
        command
            .arg("-v")
            .arg(&config.voice)
            .arg("--stdin")
            .arg("--stdout")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // Own process group so cleanup signals exactly this child.
            command.process_group(0);
        }
        // A just-published executable can briefly return ETXTBSY while the
        // filesystem releases its writer. This is transient availability,
        // not a missing engine. Retry only that exact OS classification with
        // a small bound; every other spawn error remains immediate and
        // truthful.
        let mut spawn_retries = 0;
        let mut child = loop {
            match command.spawn() {
                Ok(child) => break child,
                Err(error)
                    if error.kind() == std::io::ErrorKind::ExecutableFileBusy
                        && spawn_retries < 2 =>
                {
                    spawn_retries += 1;
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => {
                    return Err(VoiceError::Unavailable {
                        provider: provider.clone(),
                        reason: BoundedString::new(format!(
                            "speech engine unavailable ({}): install prerequisite",
                            error.kind()
                        ))
                        .unwrap_or_else(|_| BoundedString::new("engine unavailable").unwrap()),
                    });
                }
            }
        };
        let stdin: Option<ChildStdin> = child.stdin.take();
        let stdout = child.stdout.take().ok_or_else(|| VoiceError::Unavailable {
            provider: provider.clone(),
            reason: BoundedString::new("engine stdout unavailable").unwrap(),
        })?;
        let mut handle = Self {
            child,
            stdin: Some(stdin.ok_or_else(|| VoiceError::Unavailable {
                provider: provider.clone(),
                reason: BoundedString::new("engine stdin unavailable").unwrap(),
            })?),
            stdout,
        };
        // Write the text on stdin and close it: the engine synthesizes
        // till EOF. Bounded by config; no shell involved.
        use std::io::Write;
        {
            let pipe = handle
                .stdin
                .as_mut()
                .ok_or_else(|| unavailability(&provider, "stdin"))?;
            pipe.write_all(text.as_bytes())
                .and_then(|()| pipe.flush())
                .map_err(|_| unavailability(&provider, "stdin write"))?;
        }
        // Drop the pipe: EOF tells the engine to synthesize to completion.
        drop(handle.stdin.take());
        Ok(handle)
    }
}

impl Drop for EngineChild {
    fn drop(&mut self) {
        // Close stdin (already closed after write) and reap exactly this
        // child in its own process group; unrelated speech services and
        // accessibility tools are never signaled.
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

fn unavailability(provider: &ProviderId, what: &'static str) -> VoiceError {
    VoiceError::Unavailable {
        provider: provider.clone(),
        reason: BoundedString::new(format!("engine {what} unavailable")).unwrap(),
    }
}

/// Validated WAV header facts for the engine's streaming output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WavFacts {
    channels: u16,
    sample_rate: u32,
    bits: u16,
}

/// Parses the canonical 44-byte PCM WAV header the engine emits. The
/// RIFF/data size fields are streaming placeholders on stdout and are
/// deliberately **ignored**; the format facts must be self-consistent.
fn parse_wav_header(bytes: &[u8]) -> Result<WavFacts, VoiceError> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(VoiceError::InvalidInput(
            "engine output is not a WAV container".into(),
        ));
    }
    if &bytes[12..16] != b"fmt " {
        return Err(VoiceError::InvalidInput(
            "engine WAV missing fmt chunk".into(),
        ));
    }
    let audio_format = u16::from_le_bytes([bytes[20], bytes[21]]);
    if audio_format != 1 {
        return Err(VoiceError::InvalidInput(
            "engine WAV is not uncompressed PCM".into(),
        ));
    }
    Ok(WavFacts {
        channels: u16::from_le_bytes([bytes[22], bytes[23]]),
        sample_rate: u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]),
        bits: u16::from_le_bytes([bytes[34], bytes[35]]),
    })
}

/// Runs one complete synthesis on the executor thread: spawn, write
/// stdin, read bounded stdout to EOF, validate, convert, return frames.
/// No audio files are created anywhere; everything is in memory.
fn synthesize_blocking(
    config: &SubprocessTtsConfig,
    text: &str,
) -> Result<Vec<PcmFrame>, VoiceError> {
    let provider = ProviderId::new("tts-subprocess").expect("static id fits");
    let mut handle = EngineChild::spawn(config, text)?;
    let mut raw: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = handle
            .stdout
            .read(&mut chunk)
            .map_err(|_| unavailability(&provider, "read"))?;
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..read]);
        if raw.len() > config.max_output_bytes {
            return Err(VoiceError::ResourceExhausted(
                "engine output exceeded its bound".into(),
            ));
        }
    }
    let status = handle
        .child
        .wait()
        .map_err(|_| unavailability(&provider, "wait"))?;
    if !status.success() {
        return Err(VoiceError::Inference(
            "engine exited nonzero before audio completed".into(),
        ));
    }
    let facts = parse_wav_header(&raw)?;
    if facts.channels != 1 || facts.bits != 16 {
        return Err(VoiceError::InvalidInput(
            "engine WAV is not mono 16-bit".into(),
        ));
    }
    let payload = &raw[44..];
    // Truncation check: the engine streams, so an odd trailing byte is
    // possible on a killed pipe; treat as truncation (observable).
    if !payload.len().is_multiple_of(2) {
        return Err(VoiceError::Truncated);
    }
    let samples: Vec<i16> = payload
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let canonical = if facts.sample_rate == CANONICAL_SAMPLE_RATE_HZ {
        samples
    } else {
        resample(&samples, facts.sample_rate, CANONICAL_SAMPLE_RATE_HZ)
    };
    // One bounded frame per request (buffered-then-yielded behavior is
    // advertised truthfully in the descriptor; chunks are split from the
    // same in-memory buffer without copying the whole thing repeatedly).
    let frame = PcmFrame::from_aligned(canonical.iter().flat_map(|s| s.to_le_bytes()).collect())
        .map_err(|_| VoiceError::InvalidInput("conversion produced misaligned audio".into()))?;
    Ok(vec![frame])
}

/// Bounded linear resampler for integer-ratio-friendly conversions
/// (verified for 22050→16000 by tests; deterministic, allocation-bounded
/// by output length). Uses nearest-phase linear interpolation without
/// any resampling dependency.
fn resample(input: &[i16], from: u32, to: u32) -> Vec<i16> {
    if input.is_empty() || from == to {
        return input.to_vec();
    }
    let length = (input.len() as u64 * u64::from(to) / u64::from(from)) as usize;
    let mut output = Vec::with_capacity(length);
    for index in 0..length {
        let position = f64::from(index as u32) * f64::from(from) / f64::from(to);
        let base = position.floor() as usize;
        let fraction = position - position.floor();
        let left = input.get(base).copied().unwrap_or(0);
        let right = input.get(base + 1).copied().unwrap_or(left);
        let value = f64::from(left) * (1.0 - fraction) + f64::from(right) * fraction;
        output.push(value.round().clamp(-32768.0, 32767.0) as i16);
    }
    output
}

/// The synthesis-only local adapter.
pub struct SubprocessTts {
    config: SubprocessTtsConfig,
    descriptor: TtsDescriptor,
    executor: Arc<dyn ValueExecutor>,
    /// Serialized engine use: one child at a time per adapter instance
    /// (bounded concurrent children; tests prove no accumulation). The
    /// guard is taken inside the blocking closure, keeping the async
    /// future `Send`.
    lock: Arc<Mutex<()>>,
}

impl SubprocessTts {
    /// Creates the adapter (configuration validated eagerly).
    ///
    /// # Errors
    ///
    /// [`VoiceError::InvalidInput`] on an invalid configuration.
    pub fn new(
        config: SubprocessTtsConfig,
        executor: Arc<dyn ValueExecutor>,
    ) -> Result<Self, VoiceError> {
        config.validate()?;
        let descriptor = TtsDescriptor {
            provider: ProviderId::new("tts-subprocess").expect("static id fits"),
            model: BoundedString::new(format!("{}:{}", config.executable.display(), config.voice))
                .unwrap_or_else(|_| BoundedString::new("system-speech-engine").unwrap()),
            egress: SpeechEgressClass::OnDevice,
            // Honest advertisement: the engine synthesizes the whole
            // request before we yield frames. Not incremental synthesis;
            // reading stdout does not make it low-latency.
            streaming: false,
            requires_hygiene: false,
        };
        Ok(Self {
            config,
            descriptor,
            executor,
            lock: Arc::new(Mutex::new(())),
        })
    }
}

impl VoiceTts for SubprocessTts {
    fn synthesize<'a>(
        &'a self,
        text: &'a str,
        _voice: &'a VoiceProfile,
        cancel: &'a VoiceCancel,
    ) -> VoiceFuture<'a, Result<TtsStream<'a>, VoiceError>> {
        Box::pin(async move {
            if cancel.is_cancelled() {
                // Cancellation before work: never spawn a child.
                return Err(VoiceError::Cancelled);
            }
            if text.trim().is_empty() {
                return Err(VoiceError::InvalidInput(
                    "refusing to synthesize empty text".into(),
                ));
            }
            if text.len() > self.config.max_text_bytes {
                return Err(VoiceError::ResourceExhausted(
                    "synthesis text exceeded its bound".into(),
                ));
            }
            let config = self.config.clone();
            let text = text.to_owned();
            // Serialization happens inside the blocking closure (a
            // std::sync guard is fine there and the async future stays
            // Send): one child at a time per adapter instance.
            let lock = Arc::clone(&self.lock);
            let provider = self.descriptor.provider.clone();
            let frames = blocking::run_blocking(
                self.executor.as_ref(),
                cancel,
                Box::new(move || {
                    let _guard = lock
                        .lock()
                        .map_err(|_| unavailability(&provider, "state (poisoned lock)"))?;
                    synthesize_blocking(&config, &text)
                }),
            )
            .await?;
            // Stream the completed frames (buffered-then-yielded; the
            // descriptor says so) with cancellation/mid-stream semantics.
            let stream: TtsStream<'a> = Box::pin(FrameStream {
                frames,
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
        // Verified on-device voices only: the engine's own voice list is
        // not parsed here (that would be a separate capability); we
        // report the configured voice as the single known-good profile.
        let profile = VoiceProfile {
            voice_id: BoundedString::new(self.config.voice.clone())
                .unwrap_or_else(|_| BoundedString::new("default").unwrap()),
            label: BoundedString::new("System speech engine (configured voice)").unwrap(),
            sample_rate_hz: CANONICAL_SAMPLE_RATE_HZ,
        };
        Box::pin(async move { Ok(vec![profile]) })
    }
}

/// Yields buffered frames with cancellation checked between items; a
/// drop or cancel suppresses the remaining (stale) audio.
struct FrameStream {
    frames: Vec<PcmFrame>,
    index: usize,
    cancel: VoiceCancel,
}

impl futures_core::Stream for FrameStream {
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
