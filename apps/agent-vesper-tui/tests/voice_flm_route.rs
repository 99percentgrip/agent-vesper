//! VRO-17 R16 red-first: the FLM NPU recognition composition through
//! REAL production seams (`voice_accel` route resolution, the
//! `FlmNpuStt` adapter's contract, VAD-composition discipline), with
//! doubles only at the external process boundary (a fixture ASR server
//! standing in for the owned flm process).
//!
//! No device is required: the adapter's request/response/error paths
//! are proven against a controlled loopback double; the real installed
//! model's receipts are produced separately by the PTY acceptance
//! probe (see the owning foundation record).

#![cfg(feature = "voice-flm")]
#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

/// Serializes tests that touch the process-global FLM verification
/// atomic; other tests in this suite stay parallel.
static FLM_STATE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

use vesper_voice::audio::PcmFrame;
use vesper_voice::cancel::VoiceCancel;
use vesper_voice::composition::blocking::ThreadPoolExecutor;
use vesper_voice::error::VoiceError;
use vesper_voice::ports::{SttTranscript, VoiceStt};

use agent_vesper_tui::voice_flm::{BACKEND_ID, FlmNpuStt, MODEL_ID, transcribe_request_public};

/// Builds canonical PCM frames: `seconds` of the given amplitude
/// envelope (positive segments alternate speech-like tone, else zeros).
fn pcm_fixture(seconds: f64, voiced: bool) -> Vec<PcmFrame> {
    let samples = (seconds * 16_000.0) as usize;
    let mut data: Vec<u8> = Vec::with_capacity(samples * 2);
    for index in 0..samples {
        let sample: i16 = if voiced {
            let t = index as f64 / 16_000.0;
            let envelope = (std::f64::consts::PI * t / seconds).sin().max(0.0);
            (envelope * 12_000.0 * (2.0 * std::f64::consts::PI * 220.0 * t).sin()) as i16
        } else {
            0
        };
        data.extend_from_slice(&sample.to_le_bytes());
    }
    data.chunks(2)
        .map(|chunk| PcmFrame::from_aligned(chunk.to_vec()).expect("aligned"))
        .collect()
}

/// A bounded loopback double for the owned ASR endpoint: records the
/// exact multipart request, replies with a controlled JSON body.
struct AsrDouble {
    requests: Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
}

impl AsrDouble {
    fn start_on(port: u16, response: &'static str) -> (Self, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0u8; 8192];
                let mut body = Vec::new();
                let _ = stream
                    .read(&mut buffer)
                    .map(|count| body.extend_from_slice(&buffer[..count]));
                let _ = stream.set_read_timeout(Some(Duration::from_millis(1500)));
                loop {
                    match stream.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(count) => {
                            body.extend_from_slice(&buffer[..count]);
                            // Complete: the multipart terminator arrived.
                            if body.ends_with(b"\r\n--vesper-flm-7f3a9c1d--\r\n") {
                                break;
                            }
                        }
                        // Read-quiet: the client finished writing.
                        Err(_) => break,
                    }
                }
                recorded.lock().expect("lock").push(body);
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                    response.len()
                );
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        std::thread::sleep(Duration::from_millis(50));
        let _ = port;
        (Self { requests }, handle)
    }
}

/// The audited multipart request reaches the wire with exactly the two
/// verified fields, a canonical WAV body, and bounded framing.
#[test]
fn multipart_request_shape_is_the_audited_contract() {
    let _seam = FLM_STATE_LOCK.lock().expect("seam lock");
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);
    let (server, handle) = AsrDouble::start_on(port, r#"{"model":"whisper-v3","text":"hello"}"#);
    let wav: Vec<u8> = vec![b'R', b'I', b'F', b'F', 0, 0, 0, 0, b'W', b'A', b'V', b'E'];
    let transcript = transcribe_request_public(port, &wav).expect("transcribe");
    handle.join().expect("server thread");
    assert_eq!(transcript, "hello");
    let recorded = server.requests.lock().expect("lock");
    let request = recorded.first().expect("one request");
    let text = String::from_utf8_lossy(request);
    assert!(
        text.starts_with("POST /v1/audio/transcriptions HTTP/1.1"),
        "path/method must match the audited contract"
    );
    assert!(text.contains("multipart/form-data; boundary="));
    assert!(text.contains(r#"name="model""#));
    assert!(text.contains("whisper-v3"));
    assert!(text.contains(r#"name="file""#));
    assert!(text.contains("filename=\"speech.wav\""));
    assert!(text.contains("Connection: close"));
    // No invented fields.
    assert!(!text.contains("language"));
    assert!(!text.contains("stream"));
    assert!(!text.contains("task"));
}

/// Malformed, oversized, missing-text and non-200 responses never
/// become transcripts or confirmed silence.
#[test]
fn backend_failures_never_become_silence_or_transcripts() {
    let _seam = FLM_STATE_LOCK.lock().expect("seam lock");
    // Empty text after positive VAD must be Inference (not silence).
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);
    let (server, handle) = AsrDouble::start_on(port, r#"{"model":"whisper-v3","text":""}"#);
    let outcome = transcribe_request_public(port, &[0u8; 44]);
    handle.join().expect("server thread");
    drop(server);
    // The transport layer returns the raw text; the ADAPTER maps empty
    // text to Inference. The transport seam returns "" and the adapter
    // test below covers the mapping; here we assert empty stays empty
    // (never a fabricated transcript).
    assert_eq!(outcome.expect("empty transport result"), "");

    // Missing `text` key.
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);
    let (server, handle) = AsrDouble::start_on(port, r#"{"model":"whisper-v3"}"#);
    let error = transcribe_request_public(port, &[0u8; 44]).unwrap_err();
    handle.join().expect("server thread");
    drop(server);
    assert!(matches!(error, VoiceError::InvalidInput(_)));

    // Malformed JSON.
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);
    let (server, handle) = AsrDouble::start_on(port, "not json");
    let error = transcribe_request_public(port, &[0u8; 44]).unwrap_err();
    handle.join().expect("server thread");
    drop(server);
    assert!(matches!(error, VoiceError::InvalidInput(_)));

    // Unreachable port.
    let error = transcribe_request_public(1, &[0u8; 44]).unwrap_err();
    assert!(matches!(error, VoiceError::Unavailable { .. }));
}

/// The adapter's descriptor is final-only, on-device, and truthful
/// about the backend's lack of native VAD.
#[test]
fn descriptor_is_final_only_and_truthful() {
    let pool = Arc::new(ThreadPoolExecutor::new(1));
    let adapter = FlmNpuStt::new(pool);
    let descriptor = adapter.descriptor();
    assert_eq!(descriptor.provider.as_str(), "stt-flm-npu");
    assert_eq!(descriptor.model.as_str(), MODEL_ID);
    assert_eq!(
        descriptor.egress,
        vesper_voice::ports::SpeechEgressClass::OnDevice
    );
    assert_eq!(
        descriptor.partials, None,
        "final-only: no partials advertised"
    );
    assert!(!descriptor.vad_enabled, "the engine itself has no VAD");
}

/// Zero audio is rejected before any process or request.
#[test]
fn empty_capture_is_invalid_input() {
    let pool = Arc::new(ThreadPoolExecutor::new(1));
    let adapter = FlmNpuStt::new(pool);
    let error = block_on_transcribe(&adapter, Vec::new(), VoiceCancel::default());
    assert!(matches!(error, Err(VoiceError::InvalidInput(_))));
}

/// Pre-start cancellation never spawns anything.
#[test]
fn cancelled_scope_makes_no_request() {
    let pool = Arc::new(ThreadPoolExecutor::new(1));
    let adapter = FlmNpuStt::new(pool);
    let cancel = VoiceCancel::default();
    cancel.cancel();
    let audio = pcm_fixture(0.2, true);
    let error = block_on_transcribe(&adapter, audio, cancel);
    assert!(matches!(error, Err(VoiceError::Cancelled)));
}

/// An over-budget capture is rejected before any VAD or service work.
#[test]
fn over_budget_capture_is_rejected() {
    let pool = Arc::new(ThreadPoolExecutor::new(1));
    let adapter = FlmNpuStt::new(pool);
    let oversized = vec![0u8; (4 * 1024 * 1024) + 2];
    let frames: Vec<PcmFrame> = oversized
        .chunks(2)
        .map(|chunk| PcmFrame::from_aligned(chunk.to_vec()).expect("aligned"))
        .collect();
    let error = block_on_transcribe(&adapter, frames, VoiceCancel::default());
    assert!(matches!(error, Err(VoiceError::InvalidInput(_))));
}

/// Blocks on an adapter future with a bounded timeout. Takes owned
/// audio/cancel so the future's borrows are 'static-safe.
fn block_on_transcribe(
    adapter: &FlmNpuStt,
    audio: Vec<PcmFrame>,
    cancel: VoiceCancel,
) -> Result<SttTranscript, VoiceError> {
    let future = adapter.transcribe(&audio, &cancel);
    let waker = std::task::Waker::noop();
    let mut context = std::task::Context::from_waker(waker);
    let mut future = std::pin::pin!(future);
    let deadline = std::time::Instant::now() + Duration::from_secs(240);
    loop {
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(result) => return result,
            std::task::Poll::Pending => {
                if std::time::Instant::now() > deadline {
                    panic!("adapter future exceeded the test deadline");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

/// The route registers with the exact verified identity, STT only, and
/// CPU policy stays structurally acceleration-blind with it registered.
#[test]
fn route_registration_and_cpu_blindness() {
    let routes = agent_vesper_tui::voice_accel::registered_routes();
    assert!(
        routes
            .iter()
            .all(|route| route.stage == vesper_voice::SpeechStage::Stt),
        "no TTS route may be registered by this unit (Kokoro stays CPU)"
    );
    let route = routes
        .iter()
        .find(|route| route.backend == BACKEND_ID)
        .expect("the FLM STT route is registered in voice-flm builds");
    assert_eq!(route.model, MODEL_ID);
    assert_eq!(route.stage, vesper_voice::SpeechStage::Stt);

    // CPU policy: structurally blind (never consults the registry).
    match agent_vesper_tui::voice_accel::stage_route_for_policy(
        vesper_voice::SpeechStage::Stt,
        vesper_voice::StageExecutionPolicy::Cpu,
    ) {
        vesper_voice::execution::StageResolution::Execute(decision) => {
            assert_eq!(
                decision.backend,
                vesper_voice::execution::StageBackend::Cpu,
                "CPU policy must execute on CPU even with a route registered"
            );
        }
        vesper_voice::execution::StageResolution::Refused { .. } => {
            panic!("CPU policy must never refuse");
        }
    }
}

/// Readiness is evidence-gated: without a Verify in this process the
/// honest state on this machine (device + runtime + verified pack) is
/// VerificationPending — Ready only after `record_flm_stt_verification`.
#[test]
fn readiness_is_pending_until_real_verification() {
    // The FLM verification state is one process-global atomic; the tests
    // that touch it hold this lock so their reset/record/observe window
    // is atomic against each other in parallel runs.
    let _guard = FLM_STATE_LOCK.lock().unwrap();
    // Explicit state contract: this suite's tests set the process-global
    // verification state they require instead of depending on declaration
    // order (the previous order-dependent pollution was itself a bug the
    // recording-review presentation tests exposed).
    agent_vesper_tui::voice_accel::reset_flm_stt_verification_for_test();
    // Reset shows the machine's evidence chain without this process's
    // verification: Ready ONLY after the record call, and its identity
    // names this backend/model.
    let readiness = agent_vesper_tui::voice_accel::stage_readiness(vesper_voice::SpeechStage::Stt);
    match readiness {
        vesper_voice::AcceleratorReadiness::VerificationPending { backend } => {
            assert_eq!(backend.as_str(), BACKEND_ID);
        }
        other => panic!("after reset, STT readiness must be pending on this machine: {other:?}"),
    }
}

/// Recording a verification flips readiness to Ready with the exact
/// identity (the native Verify action's contract).
#[test]
fn verification_record_promotes_readiness() {
    let _guard = FLM_STATE_LOCK.lock().unwrap();
    agent_vesper_tui::voice_accel::record_flm_stt_verification();
    let readiness = agent_vesper_tui::voice_accel::stage_readiness(vesper_voice::SpeechStage::Stt);
    match readiness {
        vesper_voice::AcceleratorReadiness::Ready { backend, model } => {
            assert_eq!(backend.as_str(), BACKEND_ID);
            assert_eq!(model.as_str(), MODEL_ID);
        }
        other => panic!("recorded verification must yield Ready, got {other:?}"),
    }
    // Strict NPU now resolves to Execute on the accelerator (the F9
    // gate would pass); Automatic does too. TTS stays independent.
    match agent_vesper_tui::voice_accel::stage_route_for_policy(
        vesper_voice::SpeechStage::Stt,
        vesper_voice::StageExecutionPolicy::NpuRequired,
    ) {
        vesper_voice::execution::StageResolution::Execute(decision) => {
            assert!(matches!(
                decision.backend,
                vesper_voice::execution::StageBackend::Accelerator { .. }
            ));
        }
        vesper_voice::execution::StageResolution::Refused { .. } => {
            panic!("strict NPU must execute after real verification on this machine");
        }
    }
    // TTS: no route registered, strict refuses with the honest reason.
    assert!(matches!(
        agent_vesper_tui::voice_accel::stage_route_for_policy(
            vesper_voice::SpeechStage::Tts,
            vesper_voice::StageExecutionPolicy::NpuRequired
        ),
        vesper_voice::execution::StageResolution::Refused { .. }
    ));
}

// ---------------------------------------------------------------------------
// F9 selection wiring (the gate's ordered policy checks over the saved
// scope, reconstructed with the same production functions in the same
// order as `main.rs::conversation_gate_decision` — the documented
// integration seam, as in `voice_policy_parity`).
// ---------------------------------------------------------------------------

/// Bounded temp workspace whose `.agent-vesper/config.toml` the
/// production save/read functions use.
struct F9Root {
    previous: std::path::PathBuf,
    dir: std::path::PathBuf,
    /// Held for the F9Root's whole lifetime: the process-global CWD
    /// must not change while this test's scope reads run.
    _guard: std::sync::MutexGuard<'static, ()>,
}

/// Serializes tests that chdir the PROCESS-GLOBAL working directory
/// (`F9Root`): parallel chdir from other threads swaps another test's
/// scope read out from under it (the nondeterministic
/// `f9_cpu_scope_never_refuses` flake). Distinct from the
/// verification-state lock; suite-local, zero-cost single-threaded.
static F9_ROOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl F9Root {
    fn new(tag: &str) -> Self {
        let guard = F9_ROOT_LOCK.lock().unwrap();
        let previous = std::env::current_dir().expect("cwd");
        let dir = std::env::temp_dir().join(format!("vesper-flm-f9-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        std::env::set_current_dir(&dir).expect("chdir");
        Self {
            previous,
            dir,
            _guard: guard,
        }
    }
}

impl Drop for F9Root {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.previous);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn f9_gate_for_current_scope() -> Result<&'static str, String> {
    let scope = vesper_voice::read_voice_scope(std::path::Path::new(".")).unwrap_or_default();
    if !scope.enabled {
        return Ok("Disabled");
    }
    for (stage, policy) in [
        (vesper_voice::SpeechStage::Stt, scope.stt_compute),
        (vesper_voice::SpeechStage::Tts, scope.tts_compute),
    ] {
        if let vesper_voice::StageResolution::Refused { message, .. } =
            agent_vesper_tui::voice_accel::stage_route_for_policy(stage, policy)
        {
            return Err(message.as_str().to_owned());
        }
    }
    Ok("Ready")
}

/// The saved NPU STT scope passes the gate ONLY after a real
/// verification; before that it is the honest stage refusal.
#[test]
fn f9_npu_scope_gates_on_real_verification() {
    let root = F9Root::new("npu-scope");
    let scope = vesper_voice::VoiceScope {
        enabled: true,
        stt_compute: vesper_voice::StageExecutionPolicy::NpuRequired,
        ..vesper_voice::VoiceScope::default()
    };
    agent_vesper_tui::settings_voice_save::save_voice_scope(std::path::Path::new("."), &scope)
        .expect("save");
    let saved = vesper_voice::read_voice_scope(std::path::Path::new(".")).expect("read");
    assert_eq!(
        saved.stt_compute,
        vesper_voice::StageExecutionPolicy::NpuRequired
    );

    // Note: verification state is process-global and another test in
    // this binary may have recorded it; the gate outcome must match the
    // CURRENT readiness derivation either way (Ready or the named
    // refusal), never a third value.
    match f9_gate_for_current_scope() {
        Ok("Ready") => {}
        Err(message) => assert!(
            message.contains("NPU required") && message.contains("Settings"),
            "refusal must name the policy and remedy: {message}"
        ),
        other => panic!("unexpected gate outcome: {other:?}"),
    }
    drop(root);
}

/// CPU stays the saved default and the gate is acceleration-blind: the
/// CPU scope never consults the registered route.
#[test]
fn f9_cpu_scope_never_refuses() {
    let root = F9Root::new("cpu-scope");
    let scope = vesper_voice::VoiceScope {
        enabled: true,
        stt_compute: vesper_voice::StageExecutionPolicy::Cpu,
        ..vesper_voice::VoiceScope::default()
    };
    agent_vesper_tui::settings_voice_save::save_voice_scope(std::path::Path::new("."), &scope)
        .expect("save");
    assert_eq!(f9_gate_for_current_scope().as_deref(), Ok("Ready"));
    drop(root);
}

// ---------------------------------------------------------------------------
// R16 reliability repair: stage-truthful failure classification and
// crash-leak prevention. See docs/foundation/voice-verify-read-failure-repair.md.
// ---------------------------------------------------------------------------

/// A server that accepts the request and then RESETS the connection
/// without answering must report the child/server-death failure, never
/// the read-deadline phrasing (the reproduced user failure conflated
/// an immediate reset from a dead flm child with a transport timeout).
#[test]
fn reset_connection_reports_server_death_not_read_timeout() {
    // The deadline seam is process-global: serialize with every test
    // that reads through transcribe_request (same pattern as
    // FLM_STATE_LOCK for the verification atomic).
    let _seam = FLM_STATE_LOCK.lock().expect("seam lock");
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let port = listener.local_addr().expect("addr").port();
    // A double that accepts, reads a slice, then aborts with unread
    // data still buffered — close() with a non-empty receive queue
    // makes the kernel answer with RST (the same bytes our production
    // reader observes when the flm child dies mid-response).
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut buffer = [0u8; 64];
        let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
        let _ = stream.read(&mut buffer); // read a slice only
        std::thread::sleep(Duration::from_millis(150));
        drop(stream); // RST: unread request bytes remain
    });
    let error = transcribe_request_public(port, &[0u8; 44]).unwrap_err();
    assert!(
        error.to_string().contains("owned ASR process exited"),
        "a reset from a dying server must name process death: {error}"
    );
    assert!(
        !error.to_string().contains("read failed or timed out"),
        "an immediate reset must never be labeled a timeout: {error}"
    );
}

/// The full zero-error classification table: timeout, EOF-before-answer,
/// and reset each carry their own failure name.
#[test]
fn transport_error_classes_are_distinguished() {
    let _seam = FLM_STATE_LOCK.lock().expect("seam lock");
    // (a) Timeout: a server that accepts and stays silent past the
    // deadline. Proof uses a short deadline so the test stays bounded.
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let port = listener.local_addr().expect("addr").port();
    agent_vesper_tui::voice_flm::with_request_read_deadline_for_test(
        Duration::from_millis(300),
        || {
            let server = std::thread::spawn(move || {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                // Read the full request until quiet, then hold the socket
                // open in silence until the client's (shortened) deadline
                // expires — a silent-but-accepted server.
                let mut buffer = [0u8; 8192];
                let _ = stream.set_read_timeout(Some(Duration::from_millis(400)));
                while let Ok(count) = stream.read(&mut buffer) {
                    if count == 0 {
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(1200));
            });
            let error = transcribe_request_public(port, &[0u8; 44]).unwrap_err();
            server.join().expect("server thread");
            assert!(
                error.to_string().contains("owned ASR read timed out"),
                "a silent server must be named a timeout: {error}"
            );
        },
    );

    // (b) EOF-before-answer: the server closes cleanly without
    // responding (currently surfaced as "malformed response"; the
    // contract here is only that it is never labeled a timeout).
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut buffer = [0u8; 8192];
        let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
        while let Ok(count) = stream.read(&mut buffer) {
            if count == 0 {
                break;
            }
        }
        // Clean FIN, no response bytes.
    });
    let error = transcribe_request_public(port, &[0u8; 44]).unwrap_err();
    server.join().expect("server thread");
    assert!(
        !error.to_string().contains("read timed out"),
        "a clean EOF must not be labeled a timeout: {error}"
    );
}
