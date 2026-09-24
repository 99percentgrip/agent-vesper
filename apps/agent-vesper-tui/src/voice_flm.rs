//! VRO-17 R16: the FLM NPU speech-recognition composition (feature
//! `voice-flm`). One thin host-owned owner per process, implementing the
//! verified standalone-ASR contract:
//!
//! `FLM_DISABLE_UPDATE_CHECK=1 flm serve --asr 1 --host 127.0.0.1
//! --port <owned> --cors 0 --quiet` — **no positional chat model** (the
//! positional slot is a chat-model tag; supplying `whisper-v3:turbo`
//! there triggers the verified `llama3.2:1b` fallback download — see
//! `docs/foundation/voice-npu-stt-implementation-progress.md`).
//!
//! Composition: canonical capture → **local Silero VAD preprocessing**
//! (`voice_vad.py`, the fixed persistent worker; CPU stage, labeled as
//! such) → owned loopback FLM ASR (`POST /v1/audio/transcriptions`,
//! multipart `model`+`file`, final-only) → one final transcript.
//!
//! Honest limits this module enforces (each backed by the recorded
//! evidence, not policy alone):
//!
//! - **The backend has no silence contract**: unfiltered digital silence
//!   produced hallucinated text on this exact build. Silence protection
//!   comes ONLY from the local preprocessor; the adapter therefore
//!   never dispatches audio the VAD did not clear, and VAD failures are
//!   errors, never "no speech".
//! - **No partials**: the endpoint has one final response; `None`.
//! - **Cancellation cannot stop NPU inference** (the handler ignores its
//!   token): a cancelled request settles locally, its late transcript is
//!   suppressed, and the owned service is retired (torn down and
//!   respawned on the next request) rather than admitting queued work
//!   behind an abandoned inference. CPU STT/Kokoro are unaffected.
//! - **Final-only bounded HTTP**: hand-rolled multipart against the
//!   verified owned loopback endpoint only; no proxy env inheritance, no
//!   redirects, bounded request/response, no invented fields.
//! - **Placement honesty**: `SttDescriptor` names the route; per-request
//!   offload receipts do not exist on this endpoint, so nothing here
//!   claims "full NPU" — the route's readiness record states
//!   process/device/model correlation strength.
//!
//! ## Crash-leak prevention (no unsafe)
//!
//! Crate-level `forbid(unsafe_code)` (lib.rs) stands: the crash-leak
//! repair uses a durable child registry (`~/.local/share/agent-vesper/
//! flm-child-registry/`) plus `/proc` identity checks and the `kill`
//! command — the same escalation the Drop teardown already used. No
//! PDEATHSIG/pre_exec unsafe block is needed; see
//! `docs/foundation/voice-verify-read-failure-repair.md`.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Test/boundary seam: overrides the read deadline used by
/// `transcribe_request` for the duration of one closure (tests prove
/// timeout classification without waiting the production 180 s). The
/// production default is [`REQUEST_DEADLINE`]; a zero override is
/// ignored (never an unbounded read).
#[doc(hidden)]
pub fn with_request_read_deadline_for_test<T>(deadline: Duration, body: impl FnOnce() -> T) -> T {
    REQUEST_DEADLINE_OVERRIDE
        .lock()
        .expect("test seam lock")
        .replace(deadline);
    let result = body();
    REQUEST_DEADLINE_OVERRIDE
        .lock()
        .expect("test seam lock")
        .take();
    result
}

/// The current read deadline (override-aware; the production default
/// is [`REQUEST_DEADLINE`]).
fn effective_read_deadline() -> Duration {
    REQUEST_DEADLINE_OVERRIDE
        .lock()
        .expect("test seam lock")
        .unwrap_or(REQUEST_DEADLINE)
}

#[doc(hidden)]
static REQUEST_DEADLINE_OVERRIDE: std::sync::Mutex<Option<Duration>> = std::sync::Mutex::new(None);

use vesper_domain::{BoundedString, ProviderId};
use vesper_voice::audio::PcmFrame;
use vesper_voice::cancel::VoiceCancel;
use vesper_voice::composition::blocking::{self, ValueExecutor};
use vesper_voice::error::VoiceError;
use vesper_voice::ports::{
    SpeechEgressClass, SttDescriptor, SttTranscript, TranscriptProvenance, VoiceFuture, VoiceStt,
};

/// The route identity this module registers (one backend, one model).
pub const PROVIDER_ID: &str = "stt-flm-npu";
/// The verified model identity (the installed, hash-verified pack).
pub const MODEL_ID: &str = "whisper-v3:turbo (FLM NPU2, q4nx)";
/// Backend identity used in readiness facts.
pub const BACKEND_ID: &str = "flm-npu";

/// Owned loopback port range (ephemeral-adjacent, never the FLM default
/// 52625 and never a well-known service port; exact port is picked per
/// spawn by binding first — see [`FlmService::pick_port`]).
const PORT_BASE: u16 = 18130;
const PORT_LAST: u16 = 18139;

/// Bounded lifecycle values. Startup must cover NPU model load (the
/// observed cold start bound the listener in <2 s; model load follows —
/// the recorded gate proves requests block until ready, so the first
/// *request* carries its own deadline and startup only waits for the
/// owned listener + process liveness).
const STARTUP_DEADLINE: Duration = Duration::from_secs(90);
/// One transcription request (upload + inference + delivery). The
/// recorded 1.6 s fixture completed in ~2.3 s wall; bounded headroom
/// for longer captures without an arbitrary unbounded wait.
const REQUEST_DEADLINE: Duration = Duration::from_secs(180);
/// Maximum accepted response body (the endpoint returns a small JSON
/// object; anything larger is server misbehavior, not a transcript).
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
/// Maximum accepted transcript length after decoding (port bound).
const MAX_TRANSCRIPT_BYTES: usize = 8192;
/// Maximum captured audio sent in one request: the managed capture
/// store's hard cap (R20). The VAD worker's own 64 MiB defensive
/// ceiling never overrides this.
const MAX_CAPTURE_BYTES: u64 = crate::voice_capture_store::CAPTURE_MAX_BYTES;
/// Startup log-line cap (bounded diagnostics; allowlisted metadata
/// only, never transcript text or paths beyond the owned model dir).
const MAX_LOG_LINES: usize = 512;

/// One classified startup-log observation (allowlisted metadata only).
#[derive(Debug, Clone, PartialEq, Eq)]
enum LogEvent {
    /// The server named the ASR model path it loaded.
    AsrModelLoaded,
    /// Evidence of a chat-model load or download attempt (the rejected
    /// configuration; startup must stop).
    ChatModelActivity,
}

/// Classifies one drained stderr/stdout line against the verified
/// allowlist. Returns `None` for lines carrying no classified event.
/// Never returns raw vendor text (the line is consumed, not stored).
fn classify_line(line: &str) -> Option<LogEvent> {
    let lower = line.to_ascii_lowercase();
    if lower.contains("loading model") && lower.contains("whisper") {
        return Some(LogEvent::AsrModelLoaded);
    }
    // The verified failure signatures of the chat-model fallback path:
    // an explicit unsupported-family error, a llama fallback load, or a
    // download progress line. Any of these means the launch shape was
    // wrong or the runtime changed; stop rather than race a download.
    if lower.contains("unsupported model family")
        || lower.contains("llama")
        || lower.contains("download")
        || lower.contains("downloading")
        || lower.contains("pulling")
    {
        return Some(LogEvent::ChatModelActivity);
    }
    None
}

/// The owned FLM ASR process: spawn, liveness, bounded log drain, and
/// exclusive teardown. One per application service; reused across
/// sequential requests.
struct FlmProcess {
    child: Child,
    port: u16,
    /// Bounded classified events (drained continuously by reader
    /// threads so pipes can never produce a false hang).
    events: std::sync::mpsc::Receiver<LogEvent>,
}

/// Crash-leak prevention (the reproduced orphan mechanism): every
/// spawned child's identity is recorded in this bounded registry; a
/// later host process adopts the registry and reaps entries whose
/// process is gone, whose parent is init (1), and whose exact launch
/// identity matches the owned shape. An entry whose process is alive
/// with a live parent is NEVER touched.
#[cfg(unix)]
fn owned_child_registry() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".local/share"))
        })
        .unwrap_or_else(std::env::temp_dir);
    base.join("agent-vesper")
        .join("flm-child-registry")
        .join("children")
}

/// One registry record: the exact owned launch identity plus the
/// host that spawned it (the liveness anchor for reaping: a child is
/// abandoned exactly when its registrar host is gone — never merely
/// because its parent changed, which systemd --user reparenting makes
/// normal on this platform).
#[derive(serde::Serialize, serde::Deserialize)]
struct OwnedChildRecord {
    pid: u32,
    port: u16,
    /// The host process that spawned and owns this child.
    host_pid: u32,
    /// Startup monotonic-ish marker (wall clock; advisory only).
    started_unix: u64,
}

impl FlmProcess {
    /// Picks a free loopback port by binding it, releasing it, and
    /// re-checking emptiness immediately before spawn. A port held by a
    /// foreign process is skipped, never killed.
    fn pick_port() -> Result<u16, VoiceError> {
        for port in PORT_BASE..=PORT_LAST {
            if let Ok(listener) = std::net::TcpListener::bind(("127.0.0.1", port)) {
                drop(listener);
                // Re-check: a foreign listener that raced us wins; skip.
                if std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
                    return Ok(port);
                }
            }
        }
        Err(VoiceError::Unavailable {
            provider: ProviderId::new(PROVIDER_ID).expect("static id fits"),
            reason: BoundedString::new("no free owned loopback port in the FLM range")
                .expect("static reason fits"),
        })
    }

    /// Reaps registry entries that are unambiguously abandoned owned
    /// children: the process is gone (no /proc entry) or its parent is
    /// init (1) — an orphaned leak — AND its argv still matches the
    /// owned launch shape. Entries failing the identity check are
    /// dropped from the registry but never signalled. Pure std: /proc
    /// reads plus the `kill` command (the house convention for owned
    /// teardown; no unsafe code).
    #[cfg(unix)]
    fn reap_stale_owned_children() {
        let registry = owned_child_registry();
        let Ok(entries) = std::fs::read_dir(&registry) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(raw) = std::fs::read_to_string(&path) else {
                let _ = std::fs::remove_file(&path);
                continue;
            };
            let Ok(record) = serde_json::from_str::<OwnedChildRecord>(&raw) else {
                let _ = std::fs::remove_file(&path);
                continue;
            };
            let pid = record.pid;
            let status_path = format!("/proc/{pid}/status");
            if !std::path::Path::new(&status_path).exists() {
                // Gone: a stale registry entry, nothing to signal.
                let _ = std::fs::remove_file(&path);
                continue;
            }
            // Abandoned exactly when the registrar host is gone. A
            // reparented PPid (systemd --user = 1642 here) is NOT
            // evidence of abandonment by itself.
            let host_alive =
                std::path::Path::new(&format!("/proc/{}/status", record.host_pid)).exists();
            if host_alive {
                // The host that recorded this child still runs: a
                // normal in-use owned child (another session). Never
                // touch it.
                continue;
            }
            // Confirm the exact owned launch shape before acting.
            let Ok(cmdline) = std::fs::read_to_string(format!("/proc/{pid}/cmdline")) else {
                let _ = std::fs::remove_file(&path);
                continue;
            };
            let args: Vec<&str> = cmdline.split('\0').filter(|s| !s.is_empty()).collect();
            let is_owned_shape = args.len() >= 8
                && args.iter().any(|a| a.ends_with("flm"))
                && args.windows(2).any(|w| w[0] == "serve")
                && args.windows(2).any(|w| w[0] == "--asr" && w[1] == "1");
            if !is_owned_shape {
                // PID reuse or a foreign process: drop the record,
                // never signal.
                let _ = std::fs::remove_file(&path);
                continue;
            }
            // Truly orphaned owned-shape child: TERM, bounded wait,
            // then KILL — the same escalation as our own Drop teardown.
            let pid_text = pid.to_string();
            let _ = Command::new("kill")
                .args(["-TERM", &pid_text])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            for _ in 0..30 {
                if !std::path::Path::new(&status_path).exists() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if std::path::Path::new(&status_path).exists() {
                let _ = Command::new("kill")
                    .args(["-KILL", &pid_text])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            let _ = std::fs::remove_file(&path);
        }
    }

    /// Records this child in the registry (crash-leak reaping).
    #[cfg(unix)]
    fn record_child(&self) {
        let dir = owned_child_registry();
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let record = OwnedChildRecord {
            pid: self.child.id(),
            port: self.port,
            host_pid: std::process::id(),
            started_unix: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        };
        let Ok(json) = serde_json::to_string(&record) else {
            return;
        };
        let _ = std::fs::write(dir.join(format!("{}.json", self.child.id())), json);
    }

    /// Removes this child's registry entry (normal teardown).
    #[cfg(unix)]
    fn clear_child_record(&self) {
        let _ =
            std::fs::remove_file(owned_child_registry().join(format!("{}.json", self.child.id())));
    }

    /// Spawns the verified standalone launch. The environment is
    /// constructed (not inherited) so no proxy variables or user FLM
    /// settings can redirect the loopback transport or model root.
    fn spawn() -> Result<Self, VoiceError> {
        let provider = ProviderId::new(PROVIDER_ID).expect("static id fits");
        // Crash-leak prevention: before adding another NPU-holding
        // child, reap entries abandoned by abruptly-dead hosts (the
        // reproduced `DRM_IOCTL_AMDXDNA_CREATE_HWCTX EINVAL` cause).
        #[cfg(unix)]
        Self::reap_stale_owned_children();
        let Some(flm) = crate::voice_flm_assets::flm_executable() else {
            return Err(VoiceError::Unavailable {
                provider: provider.clone(),
                reason: BoundedString::new("the flm runtime is not installed")
                    .expect("static reason fits"),
            });
        };
        // Assets verified before spawn: never start a process whose
        // missing assets would drive a vendor download path.
        crate::voice_flm_assets::verify_installed().map_err(|detail| VoiceError::Unavailable {
            provider: provider.clone(),
            reason: detail,
        })?;
        let port = Self::pick_port()?;
        let mut command = Command::new(&flm);
        command
            .args(["serve", "--asr", "1"])
            .args(["--host", "127.0.0.1"])
            .args(["--port", &port.to_string()])
            .args(["--cors", "0", "--quiet"])
            .env_clear()
            .env("FLM_DISABLE_UPDATE_CHECK", "1")
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", std::env::var_os("HOME").unwrap_or_default())
            // Preserve an explicitly-configured model root with the
            // VENDOR's semantics (the PARENT of the pack root): an
            // isolated-HOME test context keeps resolving the intended
            // read-only pack identically to the passive verifier.
            .env(
                "FLM_MODEL_PATH",
                std::env::var_os("FLM_MODEL_PATH").unwrap_or_default(),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr({
                let log = std::env::temp_dir().join("vesper-flm-child-stderr.log");
                match std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log)
                {
                    Ok(file) => Stdio::from(file),
                    Err(_) => Stdio::piped(),
                }
            });
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // Own process group: teardown signals exactly this child.
            command.process_group(0);
        }
        let mut child = command.spawn().map_err(|error| VoiceError::Unavailable {
            provider: provider.clone(),
            reason: BoundedString::new(format!("flm spawn failed: {}", error.kind()))
                .unwrap_or_else(|_| BoundedString::new("flm spawn failed").unwrap()),
        })?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let (sender, events) = std::sync::mpsc::sync_channel(64);
        if let Some(stdout) = stdout {
            let sender = sender.clone();
            std::thread::spawn(move || {
                use std::io::BufRead;
                let reader = std::io::BufReader::new(stdout);
                for line in reader.lines() {
                    let Ok(line) = line else { break };
                    if let Some(event) = classify_line(&line) {
                        let _ = sender.send(event);
                    }
                }
            });
        }
        if let Some(stderr) = stderr {
            let sender = sender.clone();
            std::thread::spawn(move || {
                use std::io::BufRead;
                let reader = std::io::BufReader::new(stderr);
                for line in reader.lines() {
                    let Ok(line) = line else { break };
                    if let Some(event) = classify_line(&line) {
                        let _ = sender.send(event);
                    }
                }
            });
        }
        drop(sender);
        let process = Self {
            child,
            port,
            events,
        };
        #[cfg(unix)]
        process.record_child();
        Ok(process)
    }

    /// Waits for the owned listener (liveness, not ASR readiness — the
    /// recorded gate shows the listener binds before model load and
    /// requests queue; the request deadline covers model-load latency).
    fn wait_listener(&mut self) -> Result<(), VoiceError> {
        let start = Instant::now();
        loop {
            if self.exited() {
                return Err(VoiceError::Unavailable {
                    provider: ProviderId::new(PROVIDER_ID).expect("static id fits"),
                    reason: BoundedString::new("flm exited during startup")
                        .expect("static reason fits"),
                });
            }
            if std::net::TcpStream::connect(("127.0.0.1", self.port)).is_ok() {
                return Ok(());
            }
            if start.elapsed() > STARTUP_DEADLINE {
                return Err(VoiceError::Unavailable {
                    provider: ProviderId::new(PROVIDER_ID).expect("static id fits"),
                    reason: BoundedString::new("flm listener did not appear in time")
                        .expect("static reason fits"),
                });
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Whether the child has exited.
    fn exited(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)) | Err(_))
    }

    /// Pops classified events since the last check (bounded) and
    /// enforces the stop rule: evidence of chat-model activity means
    /// the launch shape was wrong or the runtime changed; the owned
    /// process must be torn down rather than racing a download.
    fn enforce_log_contract(&self) -> Result<(), VoiceError> {
        for event in self.drain_events() {
            if matches!(event, LogEvent::ChatModelActivity) {
                return Err(VoiceError::Unavailable {
                    provider: ProviderId::new(PROVIDER_ID).expect("static id fits"),
                    reason: BoundedString::new(
                        "the accelerated recognizer attempted to load a chat model; stopping the owned service (no download is permitted)",
                    )
                    .expect("static fits"),
                });
            }
        }
        Ok(())
    }

    /// Pops classified events since the last check (bounded).
    fn drain_events(&self) -> Vec<LogEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            events.push(event);
            if events.len() >= MAX_LOG_LINES {
                break;
            }
        }
        events
    }
}

impl Drop for FlmProcess {
    fn drop(&mut self) {
        // Stop exactly the owned process group (never a foreign service,
        // never a by-port or by-name kill).
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
        #[cfg(unix)]
        self.clear_child_record();
        // Reader threads terminate on pipe EOF after child exit; the
        // bounded channel senders observe this receiver's closure.
    }
}

/// One multipart transcription request against the owned endpoint.
/// Hand-rolled: verified field set only (`model`, `file`), canonical
/// WAV body, bounded read, no redirects, no proxy, `Connection: close`.
fn transcribe_request(port: u16, wav: &[u8]) -> Result<String, VoiceError> {
    let provider = ProviderId::new(PROVIDER_ID).expect("static id fits");
    let boundary = "vesper-flm-7f3a9c1d";
    let mut body: Vec<u8> = Vec::with_capacity(wav.len() + 256);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\n{MODEL_FIELD}\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"speech.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(wav);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let request = format!(
        "POST /v1/audio/transcriptions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: multipart/form-data; boundary={boundary}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
        return Err(VoiceError::Unavailable {
            provider,
            reason: BoundedString::new("owned ASR endpoint unreachable").expect("static fits"),
        });
    };
    let read_deadline = effective_read_deadline();
    let _ = stream.set_read_timeout(Some(read_deadline));
    let _ = stream.set_write_timeout(Some(read_deadline));
    stream
        .write_all(request.as_bytes())
        .and_then(|()| stream.write_all(&body))
        .map_err(|_| VoiceError::Unavailable {
            provider: provider.clone(),
            reason: BoundedString::new("owned ASR upload failed").expect("static fits"),
        })?;
    let mut raw = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                raw.extend_from_slice(&chunk[..count]);
                if raw.len() > MAX_RESPONSE_BYTES {
                    return Err(VoiceError::InvalidInput(
                        "ASR response exceeded its bound".into(),
                    ));
                }
            }
            Err(error) => {
                // Stage-truthful classification (the reproduced user
                // failure): a reset from a dying owned flm child is
                // process death, not a transport timeout. EINVAL from
                // the vendor's NPU ioctl surfaces as a child exit (the
                // server dies mid-load) and lands here as ECONNRESET.
                let reason = match error.kind() {
                    std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::BrokenPipe => "owned ASR process exited while answering",
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
                        "owned ASR read timed out"
                    }
                    _ => "owned ASR read failed or timed out",
                };
                return Err(VoiceError::Unavailable {
                    provider,
                    reason: BoundedString::new(reason).expect("static fits"),
                });
            }
        }
    }
    let text = String::from_utf8_lossy(&raw);
    let Some((head, body)) = text.split_once("\r\n\r\n") else {
        return Err(VoiceError::Unavailable {
            provider,
            reason: BoundedString::new("owned ASR malformed response").expect("static fits"),
        });
    };
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| VoiceError::Unavailable {
            provider: provider.clone(),
            reason: BoundedString::new("owned ASR malformed status").expect("static fits"),
        })?;
    if (300..400).contains(&status) {
        return Err(VoiceError::Unavailable {
            provider,
            reason: BoundedString::new("owned ASR redirect refused").expect("static fits"),
        });
    }
    if status != 200 {
        return Err(VoiceError::Unavailable {
            provider,
            reason: BoundedString::new(format!("owned ASR http {status}"))
                .unwrap_or_else(|_| BoundedString::new("owned ASR http error").unwrap()),
        });
    }
    let value: serde_json::Value = serde_json::from_str(body.trim())
        .map_err(|_| VoiceError::InvalidInput("ASR returned malformed JSON".into()))?;
    let transcript = value
        .get("text")
        .ok_or_else(|| VoiceError::InvalidInput("ASR response missing `text`".into()))?
        .as_str()
        .ok_or_else(|| VoiceError::InvalidInput("ASR `text` is not a string".into()))?
        .trim();
    if transcript.len() > MAX_TRANSCRIPT_BYTES {
        return Err(VoiceError::InvalidInput(
            "ASR transcript exceeded its bound".into(),
        ));
    }
    Ok(transcript.to_owned())
}

/// The multipart `model` field (audited contract: `whisper-v3`; the
/// echoed value is metadata only, never routing or offload proof).
const MODEL_FIELD: &str = "whisper-v3";

/// Test/boundary seam: one transcription request against an explicit
/// port (the double-server tests use this; production always resolves
/// the port through the owned service).
#[doc(hidden)]
pub fn transcribe_request_public(port: u16, wav: &[u8]) -> Result<String, VoiceError> {
    transcribe_request(port, wav)
}

/// The VAD-preprocessed, final-only FLM `VoiceStt` adapter.
///
/// Construction spawns nothing. The shared VAD worker and the owned FLM
/// service both start lazily on the first transcription and are reused
/// for subsequent requests (one warm child each per process).
pub struct FlmNpuStt {
    descriptor: SttDescriptor,
    executor: Arc<dyn ValueExecutor>,
    /// Lazily started owned service (None until first positive request).
    service: std::sync::Mutex<Option<FlmProcess>>,
    /// Retired flag: set when a cancelled request leaves backend work
    /// unfinished; cleared only by verified teardown (the next request
    /// respawns a fresh owned process).
    retired: std::sync::Mutex<bool>,
}

impl FlmNpuStt {
    /// Creates the adapter (no process side effects).
    #[must_use]
    pub fn new(executor: Arc<dyn ValueExecutor>) -> Self {
        let descriptor = SttDescriptor {
            provider: ProviderId::new(PROVIDER_ID).expect("static id fits"),
            model: BoundedString::new(MODEL_ID).expect("static model fits"),
            egress: SpeechEgressClass::OnDevice,
            partials: None,
            // Truthful: the engine itself has NO VAD. Silence protection
            // is supplied by this composition's local preprocessor.
            vad_enabled: false,
            languages: Vec::new(),
        };
        Self {
            descriptor,
            executor,
            service: std::sync::Mutex::new(None),
            retired: std::sync::Mutex::new(false),
        }
    }

    /// The descriptor's backend identity for readiness wiring.
    #[must_use]
    pub fn backend_id(&self) -> &'static str {
        BACKEND_ID
    }

    /// Starts (or reuses) the owned service. Returns the port.
    fn ensure_service(&self) -> Result<u16, VoiceError> {
        let mut guard = self.service.lock().map_err(|_| VoiceError::Unavailable {
            provider: self.descriptor.provider.clone(),
            reason: BoundedString::new("flm state lock poisoned").expect("static fits"),
        })?;
        let needs_spawn = match guard.as_mut() {
            Some(process) => process.exited(),
            None => true,
        };
        if needs_spawn {
            // Bounded spawn retry: the pre-bind/release port race can lose
            // the port to another consumer between release and flm's bind
            // (flm exits silently under --quiet). Retry with a fresh port
            // pick, bounded, rather than reporting a phantom service.
            let mut last_error = None;
            for _ in 0..3 {
                match FlmProcess::spawn() {
                    Ok(mut process) => match process.wait_listener() {
                        Ok(()) => {
                            // The startup log contract is enforced before
                            // the service is admitted: any chat-model
                            // activity fails startup here (the process is
                            // dropped, running its bounded owned teardown).
                            if let Err(error) = process.enforce_log_contract() {
                                last_error = Some(error);
                                continue;
                            }
                            *guard = Some(process);
                            *self.retired.lock().map_err(|_| VoiceError::Unavailable {
                                provider: self.descriptor.provider.clone(),
                                reason: BoundedString::new("flm state lock poisoned")
                                    .expect("static fits"),
                            })? = false;
                            break;
                        }
                        Err(error) => {
                            last_error = Some(error);
                        }
                    },
                    Err(error) => {
                        last_error = Some(error);
                        break;
                    }
                }
            }
            if guard.is_none() {
                return Err(last_error.unwrap_or_else(|| VoiceError::Unavailable {
                    provider: self.descriptor.provider.clone(),
                    reason: BoundedString::new("the local recognizer service failed to start")
                        .expect("static fits"),
                }));
            }
        }
        // Re-check the running service's log contract too (a runtime
        // behavior change mid-life surfaces as unavailability, not a
        // silently tolerated download).
        if let Some(process) = guard.as_ref() {
            process.enforce_log_contract()?;
        }
        guard
            .as_ref()
            .map(|process| process.port)
            .ok_or_else(|| VoiceError::Unavailable {
                provider: self.descriptor.provider.clone(),
                reason: BoundedString::new("flm service missing").expect("static fits"),
            })
    }

    /// Retires the owned service after unfinished backend work (a
    /// cancelled in-flight inference cannot be stopped; the service is
    /// torn down so no later request queues behind it).
    fn retire_service(&self) {
        if let Ok(mut guard) = self.service.lock() {
            // Drop runs the bounded teardown (owned group only).
            *guard = None;
        }
        if let Ok(mut retired) = self.retired.lock() {
            *retired = true;
        }
    }

    /// Whether the service is currently retired (busy/unavailable).
    fn is_retired(&self) -> bool {
        self.retired.lock().is_ok_and(|retired| *retired)
    }

    /// Tears the owned service down (Settings teardown / shutdown).
    pub fn shutdown(&self) {
        self.retire_service();
    }
}

impl VoiceStt for FlmNpuStt {
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
            if cancel.is_cancelled() {
                return Err(VoiceError::Cancelled);
            }
            // Aggregate capture bound (R20 hard cap; never the helper's
            // larger defensive ceiling).
            let mut pcm: Vec<u8> = Vec::new();
            for frame in audio {
                pcm.extend_from_slice(frame.bytes());
            }
            if pcm.len() as u64 > MAX_CAPTURE_BYTES {
                return Err(VoiceError::InvalidInput(
                    "capture exceeds the managed capture limit".into(),
                ));
            }
            // Stage 1: local Silero VAD preprocessing through the fixed
            // persistent worker. Silence short-circuits with ZERO ASR
            // requests (and no FLM spawn for that utterance). VAD
            // failure is an error — never "no speech", never a bypass.
            let filtered = crate::voice_flm_vad::preprocess(&pcm, cancel).await?;
            let Some(filtered_wav) = filtered else {
                return Ok(SttTranscript {
                    text: BoundedString::new(String::new()).unwrap(),
                    provider: self.descriptor.provider.clone(),
                    confidence: None,
                    provenance: TranscriptProvenance::VadConfirmedSilence,
                });
            };
            // Cancellation after VAD: settle locally without dispatch.
            if cancel.is_cancelled() {
                return Err(VoiceError::Cancelled);
            }
            // Busy/retired: honest unavailability (bounded serial
            // admission; no queueing behind unfinished backend work).
            if self.is_retired() {
                return Err(VoiceError::Unavailable {
                    provider: self.descriptor.provider.clone(),
                    reason: BoundedString::new(
                        "the accelerated recognizer is recovering from an interrupted request",
                    )
                    .expect("static fits"),
                });
            }
            // Stage 2: dispatch to the owned loopback service.
            let port = self.ensure_service()?;
            let provider = self.descriptor.provider.clone();
            let outcome = blocking::run_blocking(
                self.executor.as_ref(),
                cancel,
                Box::new(move || transcribe_request(port, &filtered_wav)),
            )
            .await;
            match outcome {
                Ok(text) => {
                    let trimmed = text.trim();
                    if trimmed.is_empty() {
                        // Positive VAD + empty backend text: NOT silence.
                        // The backend discarded the distinction; surface
                        // it as an engine failure, never a valid final.
                        return Err(VoiceError::Inference(
                            "the accelerated recognizer returned no transcript".into(),
                        ));
                    }
                    let bounded_text = BoundedString::new(trimmed).map_err(|_| {
                        VoiceError::InvalidInput("transcript exceeded its bound".into())
                    })?;
                    Ok(SttTranscript {
                        text: bounded_text,
                        provider,
                        confidence: None,
                        provenance: TranscriptProvenance::InferredText,
                    })
                }
                Err(VoiceError::Cancelled) => {
                    // The NPU inference may still run: retire the owned
                    // service so no later request queues behind it. Late
                    // transcripts from the abandoned request are dropped
                    // (this awaiter's result is discarded upstream).
                    self.retire_service();
                    Err(VoiceError::Cancelled)
                }
                Err(error) => Err(error),
            }
        })
    }

    fn descriptor(&self) -> &SttDescriptor {
        &self.descriptor
    }
}
