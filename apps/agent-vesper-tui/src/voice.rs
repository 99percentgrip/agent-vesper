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
}
impl Default for Controller {
    fn default() -> Self {
        let (tx, rx) = mpsc::sync_channel(4);
        let (out, text) = mpsc::sync_channel(1);
        let state = Arc::new(Mutex::new(Snapshot::default()));
        let cancel = Arc::new(AtomicBool::new(false));
        let quit = Arc::new(AtomicBool::new(false));
        let worker = {
            let state = state.clone();
            let cancel = cancel.clone();
            let quit = quit.clone();
            thread::spawn(move || Worker::new(state, cancel, quit, out).run(rx))
        };
        Self {
            tx,
            text,
            state,
            cancel,
            quit,
            worker: Some(worker),
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
    pub fn toggle(&self) {
        let mut state = self.state.lock().unwrap();
        let command = match state.phase {
            VoicePhase::Idle => Control::Start,
            VoicePhase::Recording => Control::Stop,
            VoicePhase::Error => Control::Retry,
            VoicePhase::Preparing | VoicePhase::Transcribing => {
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
                "Preparing voice… F5 cancels."
            }
            .into();
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
    _dir: tempfile::TempDir,
    path: PathBuf,
    chunks: Vec<String>,
}
impl Audio {
    fn new() -> Result<Self, String> {
        let dir = tempfile::Builder::new()
            .prefix("vesper-voice-")
            .tempdir()
            .map_err(|_| "Cannot create private audio directory.")?;
        let path = dir.path().join("recording.wav");
        Ok(Self {
            _dir: dir,
            path,
            chunks: vec![],
        })
    }
}
struct Worker {
    state: Arc<Mutex<Snapshot>>,
    cancel: Arc<AtomicBool>,
    quit: Arc<AtomicBool>,
    out: mpsc::SyncSender<String>,
    recorder: Option<Process>,
    audio: Option<Audio>,
    started: Option<Instant>,
    python: Option<String>,
    sidecar: Option<Sidecar>,
}
impl Worker {
    fn new(
        state: Arc<Mutex<Snapshot>>,
        cancel: Arc<AtomicBool>,
        quit: Arc<AtomicBool>,
        out: mpsc::SyncSender<String>,
    ) -> Self {
        Self {
            state,
            cancel,
            quit,
            out,
            recorder: None,
            audio: None,
            started: None,
            python: None,
            sidecar: None,
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
                        Control::Stop => self.stop().and_then(|()| self.transcribe()),
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
                        self.recorder = None;
                        self.publish(VoicePhase::Error, "Microphone recorder stopped unexpectedly. Check device permissions and free disk space. F5 retries saved audio; Del discards.");
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
        self.publish(VoicePhase::Preparing, "Preparing local voice model · F5 Cancel (first use may download Python packages/model)");
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
        let root = super::voice_venv_root();
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
        let python = self.prepare()?;
        if self.sidecar.is_none() {
            self.sidecar = Some(Sidecar::spawn(&python)?);
        }
        if self.cancelled() {
            return Err("Voice preparation cancelled.".into());
        }
        let audio = Audio::new()?;
        let mut command = if cfg!(target_os = "linux") {
            let mut c = Command::new("arecord");
            c.args(["-q", "-f", "S16_LE", "-r", "16000", "-c", "1", "-t", "wav"]);
            c
        } else {
            let mut c = Command::new("afrecord");
            c.args(["-f", "WAVE", "-d", "LEI16@16000", "-c", "1"]);
            c
        };
        command
            .arg(&audio.path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        self.recorder = Some(spawn(&mut command)?);
        self.audio = Some(audio);
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
            "Loading local speech model · F5 Cancel",
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
                                "Transcribed {} seconds · F5 Cancel",
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
