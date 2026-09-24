//! Terminal-only microphone controller. The worker owns all audio and subprocesses.
use agent_vesper_tui::ui::VoicePhase;
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Default)]
pub struct Snapshot {
    pub phase: VoicePhase,
    pub elapsed: u64,
    pub detail: String,
}
#[derive(Clone, Copy)]
enum Control {
    Start,
    Stop,
    Retry,
    Discard,
}

pub struct Controller {
    tx: mpsc::SyncSender<Control>,
    text: mpsc::Receiver<String>,
    state: Arc<Mutex<Snapshot>>,
    cancel: Arc<AtomicBool>,
    quit: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    /// VRO-17 R16: mirror of the worker's conversation-STT slot
    /// (the SELECTED adapter: the composed FLM NPU route when the
    /// saved scope and per-process verification admit it, otherwise
    /// the shared CPU sidecar instance — one recognizer per process
    /// either way).
    #[cfg(all(feature = "voice-conversation", feature = "voice-flm"))]
    conversation_stt:
        std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<dyn vesper_voice::ports::VoiceStt>>>>,
}
impl Default for Controller {
    fn default() -> Self {
        let (tx, rx) = mpsc::sync_channel(4);
        let (out, text) = mpsc::sync_channel(1);
        let state = Arc::new(Mutex::new(Snapshot::default()));
        let cancel = Arc::new(AtomicBool::new(false));
        let quit = Arc::new(AtomicBool::new(false));
        #[cfg(all(feature = "voice-conversation", feature = "voice-flm"))]
        let conversation_stt = std::sync::Arc::new(std::sync::Mutex::new(None));
        let worker = {
            let state = state.clone();
            let cancel = cancel.clone();
            let quit = quit.clone();
            #[cfg(all(feature = "voice-conversation", feature = "voice-flm"))]
            let conversation_stt = conversation_stt.clone();
            thread::spawn(move || {
                #[allow(unused_mut)]
                let mut worker = Worker::new(state, cancel, quit, out);
                #[cfg(all(feature = "voice-conversation", feature = "voice-flm"))]
                {
                    worker.conversation_stt = conversation_stt;
                }
                worker.run(rx)
            })
        };
        Self {
            tx,
            text,
            state,
            cancel,
            quit,
            worker: Some(worker),
            #[cfg(all(feature = "voice-conversation", feature = "voice-flm"))]
            conversation_stt,
        }
    }
}
impl Controller {
    pub fn snapshot(&self) -> Snapshot {
        self.state.lock().unwrap().clone()
    }
    pub fn take_text(&self) -> Option<String> {
        self.text.try_recv().ok()
    }
    /// VRO-17 R16: installs the conversation-selected STT adapter
    /// for F9-origin captures (the composed FLM NPU route when the
    /// saved scope selects it). Called by the F9 gate after the
    /// shared assessment admits the gesture; the CPU scope leaves
    /// the slot empty so the existing sidecar path is unchanged.
    #[cfg(all(feature = "voice-conversation", feature = "voice-flm"))]
    pub fn set_conversation_stt(&self, stt: std::sync::Arc<dyn vesper_voice::ports::VoiceStt>) {
        if let Ok(mut slot) = self.conversation_stt.lock() {
            *slot = Some(stt);
        }
    }
    pub fn toggle(&self) {
        let mut state = self.state.lock().unwrap();
        let command = match state.phase {
            VoicePhase::Idle => Control::Start,
            VoicePhase::Recording => Control::Stop,
            VoicePhase::Error => Control::Retry,
            // F5 is the voice toggle itself: pressing it during first-use
            // preparation (which can run a multi-minute package/model
            // install) previously CANCELLED that preparation — the natural
            // "press again" reflex killed the install and surfaced
            // "Voice preparation cancelled". Preparation is no longer
            // cancellable via F5; Del is the explicit cancel key.
            VoicePhase::Preparing => return,
            VoicePhase::Transcribing => {
                self.cancel.store(true, Ordering::Release);
                state.detail =
                    "Stopping voice work… audio stays private for Retry / Discard.".into();
                return;
            }
        };
        self.cancel.store(false, Ordering::Release);
        if self.tx.try_send(command).is_ok() {
            state.phase = if matches!(command, Control::Stop) {
                VoicePhase::Transcribing
            } else {
                VoicePhase::Preparing
            };
            state.detail = if matches!(command, Control::Stop) {
                "Stopping microphone…"
            } else {
                "Preparing voice… this can take several minutes on first use; Del cancels."
            }
            .into();
        }
    }
    /// Explicit cancel for the long-running phases (Preparing/Transcribing).
    /// Retains any saved audio for Retry; does not clear the error state.
    pub fn cancel_work(&self) {
        let mut state = self.state.lock().unwrap();
        if matches!(
            state.phase,
            VoicePhase::Preparing | VoicePhase::Transcribing
        ) {
            self.cancel.store(true, Ordering::Release);
            state.detail = "Stopping voice work… audio stays private for Retry / Discard.".into();
        }
    }
    pub fn discard(&self) {
        if self.snapshot().phase == VoicePhase::Error {
            let _ = self.tx.try_send(Control::Discard);
        }
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.quit.store(true, Ordering::Release);
        self.cancel.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
struct Process(Child);
/// The Linux recorder-stream pump: joins the drain thread and holds the
/// shared capture slot (the worker takes the capture back at Stop).
struct PumpHandle {
    join: Option<std::thread::JoinHandle<()>>,
    slot: std::sync::Arc<
        std::sync::Mutex<Option<agent_vesper_tui::voice_capture_store::ManagedCapture>>,
    >,
    /// Set when the store's hard CAP (not a mere EOF) ended the capture.
    cap_hit: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl PumpHandle {
    /// Joins the pump thread after the recorder stopped (its stdout
    /// closes; the loop exits; the capture is finalized in-thread).
    fn join_and_finish(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
impl Drop for PumpHandle {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        // If the capture is still in the slot at drop (worker died
        // before Stop), the un-retained ManagedCapture cleans itself.
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        // Place POSIX helpers in their own group; cancel package-manager descendants too.
        #[cfg(unix)]
        {
            let _ = Command::new("kill")
                .args(["-KILL", "--", &format!("-{}", self.0.id())])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn spawn(command: &mut Command) -> Result<Process, String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.spawn().map(Process).map_err(|e| {
        format!(
            "Cannot start voice helper ({:?}). Check microphone/Python dependencies.",
            e.kind()
        )
    })
}
struct Audio {
    /// Retained managed capture, when the capture is store-owned (the
    /// normal path after the R20 repair). Holding it keeps the capture
    /// directory alive until this Audio is dropped/cleaned.
    managed: Option<agent_vesper_tui::voice_capture_store::ManagedCapture>,
    /// Legacy private tempdir (kept only for pre-repair compatibility of
    /// the in-file unit tests below; production captures are managed).
    _dir: Option<tempfile::TempDir>,
    path: PathBuf,
    chunks: Vec<String>,
}
impl Audio {
    #[cfg(test)]
    fn new() -> Result<Self, String> {
        let dir = tempfile::Builder::new()
            .prefix("vesper-voice-")
            .tempdir()
            .map_err(|_| "Cannot create private audio directory.")?;
        let path = dir.path().join("recording.wav");
        Ok(Self {
            managed: None,
            _dir: Some(dir),
            path,
            chunks: vec![],
        })
    }
    /// Wraps a retained managed capture (R20): the store owns the file;
    /// `retain_for_transcription` has already marked it retained.
    fn for_managed(mut capture: agent_vesper_tui::voice_capture_store::ManagedCapture) -> Self {
        let path = capture.retain_for_transcription();
        Self {
            managed: Some(capture),
            _dir: None,
            path,
            chunks: vec![],
        }
    }
}
impl Drop for Audio {
    fn drop(&mut self) {
        // Explicit cleanup on every drop: the retained capture directory
        // is ours; never a foreign file, never a symlink target.
        if let Some(capture) = self.managed.take() {
            let _ = capture.cleanup();
        }
    }
}
struct Worker {
    /// VRO-17 R20 (2026-09-23 repair): every explicit capture is owned
    /// by the ONE managed store. On Linux the capture lives in the pump
    /// slot during recording and is taken back at Stop; on macOS the
    /// recorder writes the store path directly and the capture stays
    /// here. Dictation (F5, default builds) and conversation (F9,
    /// feature builds) share this ownership policy; only what happens
    /// after transcription differs.
    #[cfg(not(target_os = "linux"))]
    managed: Option<agent_vesper_tui::voice_capture_store::ManagedCapture>,
    #[cfg(target_os = "linux")]
    pump: Option<PumpHandle>,
    state: Arc<Mutex<Snapshot>>,
    cancel: Arc<AtomicBool>,
    quit: Arc<AtomicBool>,
    out: mpsc::SyncSender<String>,
    recorder: Option<Process>,
    audio: Option<Audio>,
    started: Option<Instant>,
    python: Option<String>,
    sidecar: Option<Sidecar>,
    /// VRO-17 R16: the conversation-selected STT adapter (F9 origin).
    /// When present, a conversation capture's transcription runs through
    /// this adapter (the composed FLM NPU route when the saved scope
    /// selects it) — never the CPU sidecar. Shared with the conversation
    /// host (one instance, one warm child per process); the worker never
    /// constructs a second recognizer.
    #[cfg(all(feature = "voice-conversation", feature = "voice-flm"))]
    conversation_stt:
        std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<dyn vesper_voice::ports::VoiceStt>>>>,
}
impl Worker {
    fn new(
        state: Arc<Mutex<Snapshot>>,
        cancel: Arc<AtomicBool>,
        quit: Arc<AtomicBool>,
        out: mpsc::SyncSender<String>,
    ) -> Self {
        Self {
            #[cfg(not(target_os = "linux"))]
            managed: None,
            #[cfg(target_os = "linux")]
            pump: None,
            state,
            cancel,
            quit,
            out,
            recorder: None,
            audio: None,
            started: None,
            python: None,
            sidecar: None,
            #[cfg(all(feature = "voice-conversation", feature = "voice-flm"))]
            conversation_stt: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }
    fn publish(&self, phase: VoicePhase, detail: impl Into<String>) {
        let mut state = self.state.lock().unwrap();
        let elapsed = if phase == VoicePhase::Recording {
            self.started.map(|t| t.elapsed().as_secs()).unwrap_or(0)
        } else if phase == VoicePhase::Idle {
            0
        } else {
            state.elapsed
        };
        *state = Snapshot {
            phase,
            elapsed,
            detail: detail.into(),
        };
    }

    /// R20: whether the managed capture has already been finalized by a
    /// hard cap (the writer closed while data existed). On Linux the
    /// pump finalizes in-thread at the cap; the slot still holds the
    /// capture. On macOS the recorder writes the store path directly
    /// and the cap is enforced at finish.
    fn capture_finished_by_cap(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            self.pump
                .as_ref()
                .is_some_and(|pump| pump.cap_hit.load(std::sync::atomic::Ordering::Acquire))
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.managed
                .as_ref()
                .is_some_and(|capture| capture.hit_cap())
        }
    }
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire) || self.quit.load(Ordering::Acquire)
    }
    fn run(mut self, rx: mpsc::Receiver<Control>) {
        while !self.quit.load(Ordering::Acquire) {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(Control::Discard) => {
                    self.audio = None;
                    self.started = None;
                    self.publish(VoicePhase::Idle, "Voice audio discarded.");
                }
                Ok(command) => {
                    let result = match command {
                        Control::Start => self.start(),
                        Control::Stop => self.stop().and_then(|()| {
                            #[cfg(all(feature = "voice-conversation", feature = "voice-flm"))]
                            let adapter = self
                                .conversation_stt
                                .lock()
                                .ok()
                                .and_then(|slot| slot.clone());
                            #[cfg(all(feature = "voice-conversation", feature = "voice-flm"))]
                            if let Some(adapter) = adapter {
                                let result = self.transcribe_through(&adapter);
                                agent_vesper_tui::voice_accel::record_last_stt_route(format!(
                                    "selected adapter ({})",
                                    adapter.descriptor().provider.as_str()
                                ));
                                return result;
                            }
                            #[cfg(all(feature = "voice-conversation", feature = "voice-flm"))]
                            agent_vesper_tui::voice_accel::record_last_stt_route("CPU sidecar");
                            self.transcribe()
                        }),
                        Control::Retry if self.audio.is_some() => self.transcribe(),
                        Control::Retry => self.start(),
                        Control::Discard => unreachable!(),
                    };
                    if let Err(error) = result {
                        self.publish(VoicePhase::Error, format!("{error} F5 Retry / Del Discard. Audio is private and retained only until discard or exit."));
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if let Some(recorder) = self.recorder.as_mut() {
                match recorder.0.try_wait() {
                    Ok(None) => {
                        self.publish(VoicePhase::Recording, "Recording microphone · F5 Stop")
                    }
                    _ => {
                        // R20: a recorder exit is a FAILURE only when the
                        // capture is still open (no cap reached, no Stop).
                        // When the store's hard cap already finalized the
                        // capture (byte/time bound), the recorder exiting on
                        // the closed pipe is the EXPECTED end of capture:
                        // transition like an ordinary Stop (audio retained
                        // for transcription; never an error, never an
                        // auto-submit).
                        let capture_ended_by_cap = self.capture_finished_by_cap();
                        if capture_ended_by_cap {
                            self.recorder = None;
                            let _ = self.stop();
                        } else {
                            self.recorder = None;
                            // Abnormal recorder exit: salvage the capture
                            // (if any audio exists) so F5 Retry and Del
                            // Discard keep working on it — same lifecycle
                            // as an ordinary Stop, but surfaced as an
                            // error (the recorder died unexpectedly).
                            let _ = self.stop();
                            self.publish(VoicePhase::Error, "Microphone recorder stopped unexpectedly. Check device permissions and free disk space. F5 retries saved audio; Del discards.");
                        }
                    }
                }
            }
        }
        // Stop the recorder before dropping its private audio directory.
        self.recorder = None;
        self.audio = None;
        self.sidecar = None;
    }
    fn run_command(&self, mut command: Command, limit: Duration) -> Result<(), String> {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut process = spawn(&mut command)?;
        let start = Instant::now();
        loop {
            if self.cancelled() {
                return Err("Voice preparation cancelled.".into());
            }
            match process.0.try_wait() {
                Ok(Some(status)) => {
                    return if status.success() {
                        Ok(())
                    } else {
                        Err(
                            "Voice helper failed. Check dependencies, network and free disk space."
                                .into(),
                        )
                    };
                }
                Err(_) => return Err("Cannot inspect voice helper.".into()),
                _ => {}
            }
            if start.elapsed() >= limit {
                return Err("Voice helper stopped responding; retry when ready.".into());
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
    fn prepare(&mut self) -> Result<String, String> {
        if let Some(python) = &self.python {
            return Ok(python.clone());
        }
        self.publish(VoicePhase::Preparing, "Preparing local voice model · first use may download Python packages/model · Del cancels");
        for python in super::candidate_whisper_pythons().into_iter().take(64) {
            if self.cancelled() {
                return Err("Voice preparation cancelled.".into());
            }
            let mut command = Command::new(&python);
            command.args(["-c", "import faster_whisper"]);
            if self.run_command(command, Duration::from_secs(10)).is_ok() {
                self.python = Some(python.clone());
                return Ok(python);
            }
        }
        let root = agent_vesper_tui::voice_venv_root();
        std::fs::create_dir_all(root.parent().ok_or("Invalid voice directory.")?)
            .map_err(|_| "Cannot create voice backend directory.")?;
        let python = root.join("bin/python");
        let uv = super::bundled_uv_path().unwrap_or_else(|| "uv".into());
        let mut probe = Command::new(&uv);
        probe.arg("--version");
        if self.run_command(probe, Duration::from_secs(5)).is_ok() {
            let mut command = Command::new(&uv);
            command.arg("venv").arg(&root);
            if !python.exists() {
                self.run_command(command, Duration::from_secs(300))?;
            }
            let mut command = Command::new(&uv);
            command
                .args(["pip", "install", "faster-whisper", "--python"])
                .arg(&python);
            self.run_command(command, Duration::from_secs(600))?;
        } else {
            let mut command = Command::new("python3");
            command.args(["-m", "venv"]).arg(&root);
            if !python.exists() {
                self.run_command(command, Duration::from_secs(120))?;
            }
            let mut command = Command::new(&python);
            command.args(["-m", "pip", "install", "faster-whisper"]);
            self.run_command(command, Duration::from_secs(600))?;
        }
        let mut check = Command::new(&python);
        check.args(["-c", "import faster_whisper"]);
        self.run_command(check, Duration::from_secs(30))?;
        let python = python.to_string_lossy().into_owned();
        self.python = Some(python.clone());
        Ok(python)
    }
    fn start(&mut self) -> Result<(), String> {
        if !cfg!(any(target_os = "linux", target_os = "macos")) {
            return Err("Microphone capture currently supports Linux and macOS.".into());
        }
        // Capture must not wait for Python imports, package probing or model
        // loading. A known installed interpreter can load the sidecar in
        // parallel with recording; missing setup is handled at transcription.
        if self.sidecar.is_none() {
            let (configured, explicit) = super::vesper_python_interpreter_from(
                std::env::var_os("VESPER_PYTHON_PATH").as_deref(),
                std::env::var_os("GLM_VENV_PATH").as_deref(),
            );
            let installed = agent_vesper_tui::voice_venv_root().join("bin/python");
            let python = self.python.clone().or_else(|| {
                if explicit {
                    Some(configured)
                } else {
                    installed
                        .is_file()
                        .then(|| installed.to_string_lossy().into_owned())
                }
            });
            if let Some(python) = python {
                self.sidecar = Some(Sidecar::spawn(&python)?);
                self.python = Some(python);
            }
        }
        if self.cancelled() {
            return Err("Voice preparation cancelled.".into());
        }
        // VRO-17 R20 (2026-09-23 repair): EVERY explicit capture — F5
        // dictation in default builds and F9 conversation in feature
        // builds — is created through the ONE managed store (hard
        // 120 s / 4 MiB caps, cross-instance aggregate reservation,
        // lease-backed cleanup, free-space reserve, dead-lease
        // recovery). Low/unknown space or aggregate exhaustion defers
        // the capture with an actionable reason: never relocation,
        // unbounded buffering, or user-data deletion.
        let managed = match agent_vesper_tui::voice_capture_store::ManagedCapture::start_passthrough(
            &agent_vesper_tui::voice_capture_root(),
        ) {
            Ok(managed) => managed,
            Err(error) => {
                return Err(format!("Voice capture deferred: {error}"));
            }
        };
        // The recorder's destination is the managed capture. On Linux
        // the recorder streams WAV on stdout into the store's capped
        // writer (byte cap at the writer boundary — never polling); on
        // macOS afrecord needs a path and writes the store's capture
        // file directly (the store bounds and owns it).
        let mut command = if cfg!(target_os = "linux") {
            let mut c = Command::new("arecord");
            // "-" streams the WAV to stdout.
            c.args([
                "-q", "-f", "S16_LE", "-r", "16000", "-c", "1", "-t", "wav", "-",
            ]);
            c
        } else {
            let mut c = Command::new("afrecord");
            c.args(["-f", "WAVE", "-d", "LEI16@16000", "-c", "1"]);
            c.arg(managed.wav_path());
            c
        };
        command
            .stdin(Stdio::null())
            .stdout(if cfg!(target_os = "linux") {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stderr(Stdio::null());
        let mut process = spawn(&mut command)?;
        #[cfg(target_os = "linux")]
        {
            if let Some(stdout) = process.0.stdout.take() {
                let slot: std::sync::Arc<
                    std::sync::Mutex<Option<agent_vesper_tui::voice_capture_store::ManagedCapture>>,
                > = std::sync::Arc::new(std::sync::Mutex::new(Some(managed)));
                let pump_slot = std::sync::Arc::clone(&slot);
                let cap_hit = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let pump_cap = std::sync::Arc::clone(&cap_hit);
                let pump = std::thread::spawn(move || {
                    use std::io::Read;
                    let mut stdout = stdout;
                    let mut buffer = [0u8; 16 * 1024];
                    loop {
                        match stdout.read(&mut buffer) {
                            Ok(0) | Err(_) => break,
                            Ok(count) => {
                                let Ok(mut guard) = pump_slot.lock() else {
                                    break;
                                };
                                let Some(capture) = guard.as_mut() else {
                                    break;
                                };
                                if !capture.write_pcm(&buffer[..count]).unwrap_or(false) {
                                    pump_cap.store(true, std::sync::atomic::Ordering::Release);
                                    break;
                                }
                            }
                        }
                    }
                    if let Ok(mut guard) = pump_slot.lock()
                        && let Some(capture) = guard.as_mut()
                    {
                        capture.finish();
                    }
                });
                self.pump = Some(PumpHandle {
                    join: Some(pump),
                    slot,
                    cap_hit,
                });
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.managed = Some(managed);
        }
        self.recorder = Some(process);
        self.audio = None; // the managed store owns the capture file
        self.started = Some(Instant::now());
        self.publish(VoicePhase::Recording, "Recording microphone · F5 Stop");
        Ok(())
    }
    fn stop(&mut self) -> Result<(), String> {
        if let Some(mut recorder) = self.recorder.take() {
            #[cfg(unix)]
            {
                let _ = Command::new("kill")
                    .args(["-TERM", &recorder.0.id().to_string()])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            let start = Instant::now();
            while recorder
                .0
                .try_wait()
                .map_err(|_| "Cannot stop microphone.")?
                .is_none()
                && start.elapsed() < Duration::from_secs(3)
            {
                thread::sleep(Duration::from_millis(20));
            }
        }
        // R20: take the finished capture back and retain it for
        // transcription (explicit lifecycle: Stop never submits; the
        // transcript stays editable; Del discards and cleans).
        #[cfg(target_os = "linux")]
        if let Some(mut pump) = self.pump.take() {
            pump.join_and_finish();
            if let Ok(mut slot) = pump.slot.lock()
                && let Some(mut capture) = slot.take()
            {
                capture.finish();
                self.audio = Some(Audio::for_managed(capture));
            }
        }
        #[cfg(not(target_os = "linux"))]
        if let Some(mut capture) = self.managed.take() {
            capture.finish();
            self.audio = Some(Audio::for_managed(capture));
        }
        Ok(())
    }
    fn transcribe(&mut self) -> Result<(), String> {
        let python = self.prepare()?;
        let audio = self
            .audio
            .as_ref()
            .ok_or("No saved audio; discard and record again.")?;
        if std::fs::metadata(&audio.path).map_or(true, |m| m.len() <= 44) {
            return Err(
                "No recoverable audio. Check the microphone, then discard and record again.".into(),
            );
        }
        self.publish(
            VoicePhase::Transcribing,
            "Loading local speech model · Del cancels",
        );
        let mut sidecar = match self.sidecar.take() {
            Some(sidecar) => sidecar,
            None => Sidecar::spawn(&python)?,
        };
        let request = serde_json::json!({"wav": audio.path, "skip": audio.chunks.len()});
        writeln!(sidecar.stdin, "{request}")
            .and_then(|()| sidecar.stdin.flush())
            .map_err(|_| "Cannot send audio to speech helper; audio retained.")?;
        let result = self.collect(&sidecar.rx, Duration::from_secs(300));
        if result.is_ok() {
            self.sidecar = Some(sidecar);
        }
        result
    }
    /// VRO-17 R16: conversation-origin transcription through the
    /// selected adapter (the composed FLM NPU route). Reads the capture
    /// WAV once, drives the adapter's future to completion with the
    /// worker's cancel flag bridged into it, and emits the final text
    /// through the same channel the CPU path uses (drain_voice routes it
    /// by capture origin). The CPU sidecar is never spawned or consulted
    /// on this path.
    #[cfg(all(feature = "voice-conversation", feature = "voice-flm"))]
    fn transcribe_through(
        &mut self,
        adapter: &std::sync::Arc<dyn vesper_voice::ports::VoiceStt>,
    ) -> Result<(), String> {
        let audio = self
            .audio
            .as_ref()
            .ok_or("No saved audio; discard and record again.")?;
        let bytes = std::fs::read(&audio.path)
            .map_err(|_| "Cannot read saved audio; discard and record again.")?;
        if bytes.len() <= 44 {
            return Err(
                "No recoverable audio. Check the microphone, then discard and record again.".into(),
            );
        }
        self.publish(
            VoicePhase::Transcribing,
            "Transcribing through accelerated speech · Del cancels",
        );
        let frame = vesper_voice::audio::PcmFrame::from_aligned(bytes[44..].to_vec())
            .map_err(|_| "Capture audio is not sample-aligned.".to_owned())?;
        let cancel = vesper_voice::cancel::VoiceCancel::new();
        let future = adapter.transcribe(std::slice::from_ref(&frame), &cancel);
        let waker = std::task::Waker::noop();
        let mut context = std::task::Context::from_waker(waker);
        let mut future = std::pin::pin!(future);
        let deadline = Instant::now() + Duration::from_secs(300);
        let outcome = loop {
            match future.as_mut().poll(&mut context) {
                std::task::Poll::Ready(result) => break result,
                std::task::Poll::Pending => {
                    if self.cancelled() {
                        cancel.cancel();
                    }
                    if Instant::now() > deadline {
                        break Err(vesper_voice::error::VoiceError::Inference(
                            "accelerated transcription exceeded its time bound".into(),
                        ));
                    }
                    thread::sleep(Duration::from_millis(20));
                }
            }
        };
        match outcome {
            Ok(transcript) => {
                let text = transcript.text.as_str().trim().to_string();
                if text.is_empty() {
                    return Err("No speech detected; audio retained for retry.".into());
                }
                self.out.try_send(text).map_err(|_| {
                    "Composer has not consumed the previous dictation; audio retained.".to_owned()
                })?;
                self.audio = None;
                self.started = None;
                self.publish(
                    VoicePhase::Idle,
                    "Dictation transcribed through accelerated speech.",
                );
                Ok(())
            }
            Err(error) => Err(format!("{error}")),
        }
    }

    fn collect(
        &mut self,
        rx: &mpsc::Receiver<Result<serde_json::Value, &'static str>>,
        stall: Duration,
    ) -> Result<(), String> {
        let mut last_progress = Instant::now();
        loop {
            if self.cancelled() {
                return Err("Transcription cancelled; completed chunks and audio retained.".into());
            }
            if last_progress.elapsed() > stall {
                return Err(
                    "Speech model made no progress for five minutes; audio retained.".into(),
                );
            }
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(message) => {
                    let message = message?;
                    if message.get("error").is_some() {
                        return Err("Local transcription failed; audio retained for retry.".into());
                    }
                    if let Some(index) = message["index"].as_u64() {
                        let audio = self.audio.as_mut().ok_or("Audio unavailable.")?;
                        if index != audio.chunks.len() as u64 {
                            return Err("Out-of-order transcription chunk; audio retained.".into());
                        }
                        let text = message["text"]
                            .as_str()
                            .ok_or("Invalid transcript chunk.")?;
                        audio.chunks.push(text.into());
                        last_progress = Instant::now();
                        self.publish(
                            VoicePhase::Transcribing,
                            format!(
                                "Transcribed {} seconds · Del cancels",
                                message["seconds"].as_u64().unwrap_or(0)
                            ),
                        );
                    } else if message["ready"] == true {
                        last_progress = Instant::now();
                    } else if message["done"] == true {
                        let audio = self.audio.as_ref().ok_or("Audio unavailable.")?;
                        if message["chunks"].as_u64() != Some(audio.chunks.len() as u64) {
                            return Err("Incomplete transcription; audio retained.".into());
                        }
                        let text = audio
                            .chunks
                            .iter()
                            .filter(|t| !t.trim().is_empty())
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(" ");
                        if text.is_empty() {
                            return Err("No speech detected; audio retained for retry.".into());
                        }
                        self.out.try_send(text).map_err(
                            |_| "Composer has not consumed the previous dictation; audio retained.",
                        )?;
                        self.audio = None;
                        self.started = None;
                        self.publish(
                            VoicePhase::Idle,
                            "Dictation added to composer. Audio deleted; review before sending.",
                        );
                        return Ok(());
                    } else {
                        return Err("Invalid voice progress response.".into());
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("Speech helper exited before completion; audio retained.".into());
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

struct Sidecar {
    process: Option<Process>,
    stdin: std::process::ChildStdin,
    rx: mpsc::Receiver<Result<serde_json::Value, &'static str>>,
    reader: Option<thread::JoinHandle<()>>,
}
impl Sidecar {
    fn spawn(python: &str) -> Result<Self, String> {
        let mut command = Command::new(python);
        command
            .arg("-c")
            .arg(SCRIPT)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut process = spawn(&mut command)?;
        let stdout = process.0.stdout.take().ok_or("Voice output unavailable.")?;
        let stdin = process.0.stdin.take().ok_or("Voice input unavailable.")?;
        let (tx, rx) = mpsc::sync_channel(8);
        let reader = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = Vec::new();
                match (&mut reader).take(65_537).read_until(b'\n', &mut line) {
                    Ok(0) => break,
                    Ok(_) if line.len() <= 65_536 => {
                        if tx
                            .send(
                                serde_json::from_slice(&line)
                                    .map_err(|_| "Invalid voice response."),
                            )
                            .is_err()
                        {
                            break;
                        }
                    }
                    _ => {
                        let _ = tx.send(Err("Voice response exceeded its bound."));
                        break;
                    }
                }
            }
        });
        Ok(Self {
            process: Some(process),
            stdin,
            rx,
            reader: Some(reader),
        })
    }
}
impl Drop for Sidecar {
    fn drop(&mut self) {
        // Disconnect before joining, including a reader blocked on the bounded channel.
        let (_, empty) = mpsc::sync_channel(1);
        drop(std::mem::replace(&mut self.rx, empty));
        self.process = None;
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

// Bounded 30-second PCM slices: memory and per-chunk work do not grow with capture length.
const SCRIPT: &str = include_str!("voice_transcribe.py");

#[cfg(test)]
mod tests {
    use super::*;
    fn controller() -> (Controller, std::sync::mpsc::Receiver<Control>) {
        // Mirrors Controller::new but returns the control receiver so the
        // key-mapping contract is testable without spawning the worker.
        let (tx, rx) = mpsc::sync_channel(4);
        let (_out, text) = mpsc::sync_channel(1);
        let state = Arc::new(Mutex::new(Snapshot::default()));
        let cancel = Arc::new(AtomicBool::new(false));
        let quit = Arc::new(AtomicBool::new(false));
        std::mem::forget((cancel.clone(), quit.clone(), state.clone()));
        let controller = Controller {
            tx,
            text,
            state,
            cancel,
            quit,
            worker: None,
            #[cfg(all(feature = "voice-conversation", feature = "voice-flm"))]
            conversation_stt: std::sync::Arc::new(std::sync::Mutex::new(None)),
        };
        (controller, rx)
    }
    #[test]
    fn f5_during_preparing_is_ignored_not_a_cancel() {
        // Regression: F5 is the voice toggle itself. During first-use
        // preparation (multi-minute package install) pressing F5 used to
        // set the cancel flag, surfacing "Voice preparation cancelled."
        // and leaving the feature unusable. It must be a no-op now.
        let (controller, rx) = controller();
        {
            let mut state = controller.state.lock().unwrap();
            state.phase = VoicePhase::Preparing;
        }
        controller.toggle();
        assert!(
            !controller.cancel.load(Ordering::Acquire),
            "F5 during Preparing must not set the cancel flag"
        );
        assert!(
            rx.try_recv().is_err(),
            "F5 during Preparing must send no control command"
        );
        {
            let state = controller.state.lock().unwrap();
            assert_eq!(
                state.phase,
                VoicePhase::Preparing,
                "phase must stay Preparing"
            );
        }
    }
    #[test]
    fn del_during_preparing_and_transcribing_cancels() {
        let (controller, _rx) = controller();
        {
            let mut state = controller.state.lock().unwrap();
            state.phase = VoicePhase::Preparing;
        }
        controller.cancel_work();
        assert!(
            controller.cancel.load(Ordering::Acquire),
            "Del during Preparing must set the cancel flag"
        );
        controller.cancel.store(false, Ordering::Release);
        {
            let mut state = controller.state.lock().unwrap();
            state.phase = VoicePhase::Transcribing;
        }
        controller.cancel_work();
        assert!(
            controller.cancel.load(Ordering::Acquire),
            "Del during Transcribing must set the cancel flag"
        );
        // Idle/Recording/Error phases: cancel_work must be inert.
        controller.cancel.store(false, Ordering::Release);
        for phase in [VoicePhase::Idle, VoicePhase::Recording, VoicePhase::Error] {
            let mut state = controller.state.lock().unwrap();
            state.phase = phase;
            drop(state);
            controller.cancel_work();
            assert!(
                !controller.cancel.load(Ordering::Acquire),
                "cancel_work must be inert in {phase:?}"
            );
        }
    }
    fn worker() -> (Worker, mpsc::Receiver<String>) {
        let (tx, rx) = mpsc::sync_channel(1);
        (
            Worker::new(
                Arc::new(Mutex::new(Snapshot::default())),
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicBool::new(false)),
                tx,
            ),
            rx,
        )
    }
    fn message(index: u64, text: &str) -> Result<serde_json::Value, &'static str> {
        Ok(serde_json::json!({"index": index, "text": text, "seconds": (index+1)*30}))
    }
    #[test]
    fn voice_retry_preserves_order_and_never_duplicates_completed_chunks() {
        let (mut worker, result) = worker();
        worker.audio = Some(Audio::new().unwrap());
        let root = worker
            .audio
            .as_ref()
            .unwrap()
            .path
            .parent()
            .unwrap()
            .to_path_buf();
        let (tx, rx) = mpsc::channel();
        tx.send(message(0, "first")).unwrap();
        tx.send(Ok(serde_json::json!({"error":"failure"}))).unwrap();
        assert!(worker.collect(&rx, Duration::from_secs(1)).is_err());
        assert!(root.exists());
        assert_eq!(worker.audio.as_ref().unwrap().chunks, ["first"]);
        tx.send(message(1, "second")).unwrap();
        tx.send(Ok(serde_json::json!({"done":true,"chunks":2})))
            .unwrap();
        worker.collect(&rx, Duration::from_secs(1)).unwrap();
        assert_eq!(result.try_recv().unwrap(), "first second");
        assert!(!root.exists());
    }
    #[test]
    fn voice_stall_cancellation_and_invalid_order_retain_audio() {
        let (mut worker, result) = worker();
        worker.audio = Some(Audio::new().unwrap());
        let (tx, rx) = mpsc::channel();
        assert!(
            worker
                .collect(&rx, Duration::from_millis(5))
                .unwrap_err()
                .contains("progress")
        );
        worker.cancel.store(true, Ordering::Release);
        assert!(
            worker
                .collect(&rx, Duration::from_secs(1))
                .unwrap_err()
                .contains("cancelled")
        );
        worker.cancel.store(false, Ordering::Release);
        tx.send(message(2, "bad")).unwrap();
        assert!(
            worker
                .collect(&rx, Duration::from_secs(1))
                .unwrap_err()
                .contains("order")
        );
        assert!(worker.audio.is_some());
        assert!(result.try_recv().is_err());
    }
    #[test]
    fn voice_progress_extends_stall_budget_without_a_whole_recording_deadline() {
        let (mut worker, result) = worker();
        worker.audio = Some(Audio::new().unwrap());
        let (tx, rx) = mpsc::channel();
        let producer = thread::spawn(move || {
            for i in 0..20 {
                tx.send(message(i, &i.to_string())).unwrap();
                thread::sleep(Duration::from_millis(10));
            }
            tx.send(Ok(serde_json::json!({"done":true,"chunks":20})))
                .unwrap();
        });
        worker.collect(&rx, Duration::from_millis(100)).unwrap();
        producer.join().unwrap();
        assert_eq!(result.recv().unwrap().split_whitespace().count(), 20);
    }
    #[cfg(unix)]
    #[test]
    fn voice_cancel_reaps_helper_descendants() {
        let (worker, _) = worker();
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("must-not-exist");
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(r#"sleep 0.3; touch "$1""#)
            .arg("fixture")
            .arg(&marker);
        assert!(
            worker
                .run_command(command, Duration::from_millis(20))
                .is_err()
        );
        thread::sleep(Duration::from_millis(400));
        assert!(!marker.exists());
    }
    #[test]
    fn voice_exit_deletes_retained_audio() {
        let (mut worker, _) = worker();
        worker.audio = Some(Audio::new().unwrap());
        let root = worker
            .audio
            .as_ref()
            .unwrap()
            .path
            .parent()
            .unwrap()
            .to_path_buf();
        drop(worker);
        assert!(!root.exists());
    }
}
