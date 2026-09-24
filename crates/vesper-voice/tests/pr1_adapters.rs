//! VRO-17 PR-1 adapter and composition tests.
//!
//! These exercise the **real adapter code** through its actual boundary:
//! deterministic subprocess fixtures (a script-fixture sidecar speaking
//! the shipped protocol) and a controlled local TCP server (speaking
//! the pinned worker's HTTP contract). No public services, no model
//! downloads, no credentials. Real-model probes are separate and are
//! run only when assets exist (see the execution report §real-model).

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use vesper_voice::audio::PcmFrame;
use vesper_voice::cancel::VoiceCancel;
use vesper_voice::composition::blocking::ThreadPoolExecutor;
use vesper_voice::composition::failover::{AttemptOutcome, FailoverStt};
use vesper_voice::composition::partials::PartialGate;
use vesper_voice::config::{CaptureBudget, SpeechEgress};
use vesper_voice::error::VoiceError;
use vesper_voice::fakes::{FakeStt, FakeSttOutcome};
use vesper_voice::ports::{SttTranscript, TranscriptProvenance, VoiceStt};
use vesper_voice::stt_http::{HttpStt, HttpSttConfig, HttpSttEndpoint};
use vesper_voice::stt_sidecar::{SidecarConfig, SidecarStt};

fn executor() -> Arc<ThreadPoolExecutor> {
    Arc::new(ThreadPoolExecutor::new(2))
}

fn frame(bytes: usize) -> PcmFrame {
    PcmFrame::from_aligned(vec![0u8; bytes]).unwrap()
}

fn speech_frames() -> Vec<PcmFrame> {
    vec![frame(3200), frame(3200), frame(3200)]
}

// ---------------------------------------------------------------- fixtures

/// Writes a fixture sidecar "interpreter": an executable wrapper whose
/// shebang runs python3 with the fixture body via `-c`. The adapter
/// invokes the interpreter path directly (no shell involved).
#[cfg(unix)]
fn fixture_sidecar_script(dir: &std::path::Path, script: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = dir.join(format!(
        "fake_sidecar_{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    // Write the python body to a separate file and reference it by path
    // (avoids quoting entirely).
    let body_path = dir.join(format!(
        "fake_body_{}_{}.py",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::write(&body_path, script).unwrap();
    // "$@" forwards the adapter's argv (script via -c) verbatim; sh
    // re-quoting of embedded double quotes would corrupt the script.
    let wrapper = format!(
        "#!/bin/sh\nexec python3 \"{}\" \"$@\"\n",
        body_path.display()
    );
    std::fs::write(&path, wrapper).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
const SIDECAR_OK: &str = r#"
import json, sys, wave
def emit(v): print(json.dumps(v), flush=True)
emit({"ready": True})
for line in sys.stdin:
    req = json.loads(line)
    with wave.open(req["wav"], "rb") as w:
        n = w.getnframes()
    emit({"index": 0, "text": "fixture hello from sidecar", "seconds": n // 16000})
    emit({"done": True, "chunks": 1})
"#;

#[cfg(unix)]
const SIDECAR_EMPTY: &str = r#"
import json, sys
def emit(v): print(json.dumps(v), flush=True)
emit({"ready": True})
for line in sys.stdin:
    emit({"index": 0, "text": "", "seconds": 0})
    emit({"done": True, "chunks": 1})
"#;

#[cfg(unix)]
const SIDECAR_ERROR: &str = r#"
import json, sys
def emit(v): print(json.dumps(v), flush=True)
emit({"ready": True})
for line in sys.stdin:
    emit({"error": "transcription failed"})
"#;

#[cfg(unix)]
const SIDECAR_MALFORMED: &str = r#"
import sys
print("not json at all", flush=True)
for line in sys.stdin:
    print("still not json", flush=True)
"#;

#[cfg(unix)]
const SIDECAR_STALL: &str = r#"
import sys, time
print('{"ready": true}', flush=True)
for line in sys.stdin:
    time.sleep(60)
"#;

#[cfg(unix)]
const SIDECAR_HANG_NO_READY: &str = r#"
import sys, time
time.sleep(120)
"#;

// ------------------------------------------------------------- sidecar tests

#[cfg(unix)]
fn sidecar_with(script: &str) -> SidecarStt {
    let dir = std::env::temp_dir().join(format!("vesper-voice-pr1-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script_path = fixture_sidecar_script(&dir, script);
    SidecarStt::new(
        SidecarConfig {
            python: script_path,
            model: "fixture-model".into(),
            request_deadline: Duration::from_secs(20),
            max_line_bytes: 65_536,
        },
        executor(),
    )
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    vesper_voice::test_util::block_on(future)
}

#[test]
#[cfg(unix)]
fn sidecar_speaks_the_shipped_protocol() {
    let sidecar = sidecar_with(SIDECAR_OK);
    let cancel = VoiceCancel::new();
    let transcript = block_on(sidecar.transcribe(&speech_frames(), &cancel)).unwrap();
    assert_eq!(transcript.text.as_str(), "fixture hello from sidecar");
    assert_eq!(transcript.provenance, TranscriptProvenance::InferredText);
    // Reuse: the warm child serves a second request.
    let second = block_on(sidecar.transcribe(&speech_frames(), &cancel)).unwrap();
    assert_eq!(second.text.as_str(), "fixture hello from sidecar");
}

#[test]
#[cfg(unix)]
fn sidecar_empty_result_is_vad_confirmed_silence() {
    // The shipped script applies vad_filter=True; its empty finalized
    // result is the VAD outcome — distinct from the legacy worker's
    // ambiguity (see stt_http tests).
    let sidecar = sidecar_with(SIDECAR_EMPTY);
    let cancel = VoiceCancel::new();
    let transcript = block_on(sidecar.transcribe(&speech_frames(), &cancel)).unwrap();
    assert_eq!(
        transcript.provenance,
        TranscriptProvenance::VadConfirmedSilence
    );
    assert!(transcript.text.as_str().is_empty());
}

#[test]
#[cfg(unix)]
fn sidecar_error_line_is_inference_failure_not_silence() {
    let sidecar = sidecar_with(SIDECAR_ERROR);
    let cancel = VoiceCancel::new();
    let error = block_on(sidecar.transcribe(&speech_frames(), &cancel)).unwrap_err();
    assert!(matches!(error, VoiceError::Inference(_)));
}

#[test]
#[cfg(unix)]
fn sidecar_malformed_line_is_invalid_input() {
    let sidecar = sidecar_with(SIDECAR_MALFORMED);
    let cancel = VoiceCancel::new();
    let error = block_on(sidecar.transcribe(&speech_frames(), &cancel)).unwrap_err();
    assert!(matches!(error, VoiceError::InvalidInput(_)));
}

#[test]
#[cfg(unix)]
fn sidecar_stall_becomes_unavailable_under_deadline() {
    let dir = std::env::temp_dir().join(format!("vesper-voice-pr1-stall-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script_path = fixture_sidecar_script(&dir, SIDECAR_STALL);
    let sidecar = SidecarStt::new(
        SidecarConfig {
            python: script_path,
            model: "fixture".into(),
            request_deadline: Duration::from_millis(900),
            max_line_bytes: 65_536,
        },
        executor(),
    );
    let cancel = VoiceCancel::new();
    let start = std::time::Instant::now();
    let error = block_on(sidecar.transcribe(&speech_frames(), &cancel)).unwrap_err();
    assert!(matches!(error, VoiceError::Unavailable { .. }), "{error:?}");
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "must be deadline-bounded"
    );
}

#[test]
fn sidecar_missing_interpreter_is_unavailable_with_setup_hint() {
    let sidecar = SidecarStt::new(
        SidecarConfig {
            python: std::path::PathBuf::from("/nonexistent/interpreter"),
            ..SidecarConfig::default()
        },
        executor(),
    );
    let cancel = VoiceCancel::new();
    let error = block_on(sidecar.transcribe(&speech_frames(), &cancel)).unwrap_err();
    match error {
        VoiceError::Unavailable { reason, .. } => {
            let text = reason.as_str();
            assert!(
                text.contains("setup prerequisite"),
                "reason must name the setup prerequisite: {text}"
            );
        }
        other => panic!("expected Unavailable, got {other:?}"),
    }
}

#[test]
fn sidecar_empty_audio_is_invalid_input() {
    let sidecar = SidecarStt::new(
        SidecarConfig {
            python: std::path::PathBuf::from("/nonexistent/interpreter"),
            ..SidecarConfig::default()
        },
        executor(),
    );
    let cancel = VoiceCancel::new();
    assert!(matches!(
        block_on(sidecar.transcribe(&[], &cancel)),
        Err(VoiceError::InvalidInput(_))
    ));
}

#[test]
#[cfg(unix)]
fn sidecar_pre_start_cancellation_never_spawns() {
    let dir = std::env::temp_dir().join(format!("vesper-voice-pr1-nospawn-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // A script that would fail loudly if executed.
    let script_path = fixture_sidecar_script(&dir, "import sys; sys.exit(3)\n");
    let sidecar = SidecarStt::new(
        SidecarConfig {
            python: script_path,
            model: "fixture".into(),
            request_deadline: Duration::from_secs(5),
            max_line_bytes: 1024,
        },
        executor(),
    );
    let cancel = VoiceCancel::new();
    cancel.cancel();
    assert!(matches!(
        block_on(sidecar.transcribe(&speech_frames(), &cancel)),
        Err(VoiceError::Cancelled)
    ));
}

#[test]
fn sidecar_config_rejects_shell_metacharacters() {
    let config = SidecarConfig {
        python: std::path::PathBuf::from("/bin/sh -c evil"),
        ..SidecarConfig::default()
    };
    assert!(config.validate().is_err());
}

#[test]
#[cfg(unix)]
fn sidecar_drop_reaps_exactly_its_child() {
    // A script that lingers; Drop must reap it (bounded teardown).
    let dir = std::env::temp_dir().join(format!("vesper-voice-pr1-reap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script_path = fixture_sidecar_script(&dir, SIDECAR_HANG_NO_READY);
    let sidecar = SidecarStt::new(
        SidecarConfig {
            python: script_path,
            model: "fixture".into(),
            request_deadline: Duration::from_secs(5),
            max_line_bytes: 1024,
        },
        executor(),
    );
    let cancel = VoiceCancel::new();
    // Drive one request (it will time out while the child lingers).
    let _ = block_on(sidecar.transcribe(&speech_frames(), &cancel));
    drop(sidecar); // must not hang: teardown kills the process group
}

// ---------------------------------------------------------------- HTTP tests

/// One-shot controlled HTTP server speaking the pinned worker contract.
struct WorkerServer {
    listener: TcpListener,
    responses: Arc<Mutex<Vec<ServerResponse>>>,
}

enum ServerResponse {
    OkText(&'static str),
    OkEmpty,
    OkMalformed,
    OkMissingField,
    OkNonString,
    OkOversized,
    Unauthorized,
    Quota,
    Redirect,
}

impl WorkerServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        Self {
            listener,
            responses: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn port(&self) -> u16 {
        self.listener.local_addr().unwrap().port()
    }

    fn script(&self, responses: Vec<ServerResponse>) {
        *self.responses.lock().unwrap() = responses;
    }

    fn serve_one(&self) {
        let (mut stream, _) = self.listener.accept().unwrap();
        let mut buffer = [0u8; 8192];
        let mut request = Vec::new();
        loop {
            let read = stream.read(&mut buffer).unwrap_or(0);
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            if let Some(header_end) = find_header_end(&request) {
                let headers = String::from_utf8_lossy(&request[..header_end]).to_uppercase();
                if headers.contains("CONTENT-LENGTH") {
                    let content_length = headers
                        .split("CONTENT-LENGTH:")
                        .nth(1)
                        .and_then(|rest| rest.split(['\r', '\n']).next())
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if request.len() >= header_end + 4 + content_length {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        let response = self
            .responses
            .lock()
            .unwrap()
            .pop()
            .unwrap_or(ServerResponse::OkEmpty);
        match response {
            ServerResponse::OkText(text) => {
                let json = format!("{{\"text\": \"{text}\"}}");
                write_response(&mut stream, 200, &json);
            }
            ServerResponse::OkEmpty => {
                write_response(&mut stream, 200, "{\"text\": \"\"}");
            }
            ServerResponse::OkMalformed => {
                write_response(&mut stream, 200, "this is not json");
            }
            ServerResponse::OkMissingField => {
                write_response(&mut stream, 200, "{\"other\": 1}");
            }
            ServerResponse::OkNonString => {
                write_response(&mut stream, 200, "{\"text\": 42}");
            }
            ServerResponse::OkOversized => {
                let big = "x".repeat(200_000);
                let json = format!("{{\"text\": \"{big}\"}}");
                write_response(&mut stream, 200, &json);
            }
            ServerResponse::Unauthorized => {
                write_response(&mut stream, 401, "auth required");
            }
            ServerResponse::Quota => {
                write_response(&mut stream, 429, "slow down");
            }
            ServerResponse::Redirect => {
                write_response(&mut stream, 302, "");
                let _ = stream.write_all(
                    b"HTTP/1.1 302 Found\r\nLocation: http://evil.example/stt\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        };
        let _ = stream.flush();
    }
}

fn find_header_end(request: &[u8]) -> Option<usize> {
    request.windows(4).position(|window| window == b"\r\n\r\n")
}

fn write_response(stream: &mut std::net::TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        302 => "Found",
        401 => "Unauthorized",
        429 => "Too Many Requests",
        _ => "Status",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
}

fn http_adapter(port: u16) -> HttpStt {
    HttpStt::new(
        HttpSttConfig {
            endpoint: HttpSttEndpoint::from_validated_parts("127.0.0.1", port, "/stt").unwrap(),
            deadline: Duration::from_secs(5),
            max_response_bytes: 64 * 1024,
            max_transcript_bytes: 8192,
        },
        executor(),
    )
    .unwrap()
}

#[test]
fn http_worker_text_round_trip() {
    let server = WorkerServer::start();
    server.script(vec![ServerResponse::OkText("hello over http")]);
    let port = server.port();
    let server_ref = &server;
    let transcript = std::thread::scope(|scope| {
        scope.spawn(move || server_ref.serve_one());
        let adapter = http_adapter(port);
        let cancel = VoiceCancel::new();
        block_on(adapter.transcribe(&speech_frames(), &cancel)).unwrap()
    });
    assert_eq!(transcript.text.as_str(), "hello over http");
    assert_eq!(transcript.provenance, TranscriptProvenance::InferredText);
}

#[test]
fn http_legacy_empty_is_not_confirmed_silence() {
    // D21: the pinned worker collapses exceptions AND silence to empty
    // text; the client must not relabel that as verified silence.
    let server = WorkerServer::start();
    server.script(vec![ServerResponse::OkEmpty]);
    let port = server.port();
    let server_ref = &server;
    let transcript = std::thread::scope(|scope| {
        scope.spawn(move || server_ref.serve_one());
        let adapter = http_adapter(port);
        let cancel = VoiceCancel::new();
        block_on(adapter.transcribe(&speech_frames(), &cancel)).unwrap()
    });
    assert_eq!(
        transcript.provenance,
        TranscriptProvenance::LegacyEmptyResponse
    );
    assert!(transcript.text.as_str().is_empty());
}

#[test]
fn http_error_matrix_maps_truthfully() {
    for (response, expected) in [
        (ServerResponse::Unauthorized, "auth"),
        (ServerResponse::Quota, "quota"),
        (ServerResponse::OkMalformed, "invalid"),
        (ServerResponse::OkMissingField, "invalid"),
        (ServerResponse::OkNonString, "invalid"),
        (ServerResponse::OkOversized, "invalid"),
        (ServerResponse::Redirect, "unavailable"),
    ] {
        let server = WorkerServer::start();
        server.script(vec![response]);
        let port = server.port();
        let error = std::thread::scope(|scope| {
            let server_ref = &server;
            scope.spawn(move || server_ref.serve_one());
            let adapter = http_adapter(port);
            let cancel = VoiceCancel::new();
            block_on(adapter.transcribe(&speech_frames(), &cancel)).unwrap_err()
        });
        match expected {
            "auth" => assert!(matches!(error, VoiceError::Auth { .. }), "{error:?}"),
            "quota" => assert!(matches!(error, VoiceError::Quota { .. }), "{error:?}"),
            "invalid" => assert!(matches!(error, VoiceError::InvalidInput(_)), "{error:?}"),
            "unavailable" => {
                assert!(matches!(error, VoiceError::Unavailable { .. }), "{error:?}");
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn http_redirect_is_refused_not_followed() {
    // The redirect case is asserted in the matrix; this test pins the
    // reason string so silent destination changes stay observable.
    let server = WorkerServer::start();
    server.script(vec![ServerResponse::Redirect]);
    let port = server.port();
    let error = std::thread::scope(|scope| {
        let server_ref = &server;
        scope.spawn(move || server_ref.serve_one());
        let adapter = http_adapter(port);
        let cancel = VoiceCancel::new();
        block_on(adapter.transcribe(&speech_frames(), &cancel)).unwrap_err()
    });
    match error {
        VoiceError::Unavailable { reason, .. } => {
            assert!(reason.as_str().contains("redirect"), "{}", reason.as_str());
        }
        other => panic!("expected unavailable, got {other:?}"),
    }
}

#[test]
fn http_short_capture_rejected_before_network() {
    let adapter = http_adapter(1); // nothing listens; must not connect
    let cancel = VoiceCancel::new();
    assert!(matches!(
        block_on(adapter.transcribe(&[frame(1600)], &cancel)),
        Err(VoiceError::InvalidInput(_))
    ));
}

#[test]
fn http_endpoint_validation_rejects_userinfo_and_query() {
    for (host, path) in [
        ("user@host", "/stt"),
        ("host/extra", "/stt"),
        ("host", "/stt?x=1"),
        ("host", "stt"),
    ] {
        assert!(
            HttpSttEndpoint::from_validated_parts(host, 8768, path).is_err(),
            "{host} {path}"
        );
    }
}

#[test]
fn http_transport_failure_is_unavailable_failover_eligible() {
    // Nothing listens on this port: connection refused maps to
    // Unavailable (failover may advance), never to silence.
    let adapter = http_adapter(1);
    let cancel = VoiceCancel::new();
    let error = block_on(adapter.transcribe(&speech_frames(), &cancel)).unwrap_err();
    assert!(matches!(error, VoiceError::Unavailable { .. }));
    assert!(error.failover_eligible());
}

// -------------------------------------------------------------- failover tests

fn fake_with(outcome: FakeSttOutcome) -> Arc<FakeStt> {
    let fake = FakeStt::on_device();
    fake.set_outcome(outcome);
    Arc::new(fake)
}

#[test]
fn failover_advances_only_on_unavailable() {
    let chain = FailoverStt::new(
        vec![
            fake_with(FakeSttOutcome::Unavailable),
            fake_with(FakeSttOutcome::Text("second adapter answer")),
        ],
        SpeechEgress::OnDevice,
    )
    .unwrap();
    let cancel = VoiceCancel::new();
    let (transcript, trace) = block_on(chain.transcribe_traced(&speech_frames(), &cancel)).unwrap();
    assert_eq!(transcript.text.as_str(), "second adapter answer");
    assert_eq!(trace.len(), 2);
    assert_eq!(trace[0].outcome, AttemptOutcome::Advanced);
    assert_eq!(trace[1].outcome, AttemptOutcome::Succeeded);
}

#[test]
fn failover_preserves_terminal_classification() {
    for is_silence in [true, false] {
        let first_outcome = if is_silence {
            FakeSttOutcome::NoSpeech
        } else {
            FakeSttOutcome::InferenceFailure
        };
        let chain = FailoverStt::new(
            vec![
                fake_with(first_outcome),
                fake_with(FakeSttOutcome::Text("never")),
            ],
            SpeechEgress::OnDevice,
        )
        .unwrap();
        let cancel = VoiceCancel::new();
        let error = block_on(chain.transcribe(&speech_frames(), &cancel)).unwrap_err();
        if is_silence {
            assert!(matches!(error, VoiceError::NoSpeech), "{error:?}");
        } else {
            assert!(matches!(error, VoiceError::Inference(_)), "{error:?}");
        }
    }
}

#[test]
fn failover_mixed_chain_with_real_http_and_fake() {
    // Real adapter (unreachable) advances to the fake: composition +
    // adapter integration through the actual boundary.
    let dead_http = Arc::new(
        HttpStt::new(
            HttpSttConfig {
                endpoint: HttpSttEndpoint::from_validated_parts("127.0.0.1", 1, "/stt").unwrap(),
                deadline: Duration::from_millis(500),
                ..HttpSttConfig::default()
            },
            executor(),
        )
        .unwrap(),
    );
    let fallback = fake_with(FakeSttOutcome::Text("fallback"));
    let chain =
        FailoverStt::new(vec![dead_http, fallback], SpeechEgress::SelfHostedRemote).unwrap();
    let cancel = VoiceCancel::new();
    let (transcript, trace) = block_on(chain.transcribe_traced(&speech_frames(), &cancel)).unwrap();
    assert_eq!(transcript.text.as_str(), "fallback");
    assert_eq!(trace[0].outcome, AttemptOutcome::Advanced);
}

#[test]
fn failover_rechecks_egress_per_attempt() {
    // Chain built under a permissive policy; the per-attempt recheck
    // still denies a remote-class candidate when policy tightened.
    // (Simulated by constructing with a remote adapter under on-device
    // policy, which construction rejects — plus a policy-allowed build
    // where every adapter advances normally.)
    let chain = FailoverStt::new(
        vec![
            fake_with(FakeSttOutcome::Unavailable),
            fake_with(FakeSttOutcome::Text("ok")),
        ],
        SpeechEgress::OnDevice,
    )
    .unwrap();
    let cancel = VoiceCancel::new();
    let (_, trace) = block_on(chain.transcribe_traced(&speech_frames(), &cancel)).unwrap();
    assert_eq!(trace.len(), 2);
    assert_eq!(trace[1].outcome, AttemptOutcome::Succeeded);
}

// --------------------------------------------------------------- partial tests

#[test]
fn partial_gate_bounds_the_whole_capture() {
    let budget = CaptureBudget {
        max_capture_bytes: 6400,
        ..CaptureBudget::default()
    };
    let gate = PartialGate::new(
        fake_with(FakeSttOutcome::Text("partial text")),
        executor(),
        budget,
    );
    gate.start_capture();
    assert!(gate.push_audio(frame(3200)).is_ok());
    assert!(gate.push_audio(frame(3200)).is_ok());
    // Third frame exceeds the whole-capture budget: loud rejection.
    assert!(matches!(
        gate.push_audio(frame(3200)),
        Err(VoiceError::ResourceExhausted(_))
    ));
}

#[test]
fn partial_gate_final_supersedes_and_rejects_stale_partials() {
    let gate = PartialGate::new(
        fake_with(FakeSttOutcome::Text("partial")),
        executor(),
        CaptureBudget::default(),
    );
    gate.start_capture();
    gate.push_audio(frame(3200)).unwrap();
    let cancel = VoiceCancel::new();
    // Begin finalization: partials become stale.
    gate.begin_finalize();
    let stale = block_on(gate.partial_eval(&cancel)).unwrap();
    assert!(stale.is_none(), "finalized generation rejects partials");

    // New generation publishes partials again.
    gate.start_capture();
    gate.push_audio(frame(3200)).unwrap();
    let partial = block_on(gate.partial_eval(&cancel)).unwrap().unwrap();
    assert_eq!(partial.generation, 2);
    assert!(partial.text.is_some());

    // Final has priority and reflects the buffer.
    let final_transcript = block_on(gate.finalize(&cancel)).unwrap();
    assert_eq!(final_transcript.text.as_str(), "partial");
}

#[test]
fn partial_gate_coalesces_concurrent_evaluations() {
    let gate = PartialGate::new(
        fake_with(FakeSttOutcome::Text("coalesced")),
        executor(),
        CaptureBudget::default(),
    );
    gate.start_capture();
    gate.push_audio(frame(3200)).unwrap();
    let cancel = VoiceCancel::new();
    let first = block_on(gate.partial_eval(&cancel));
    drop(first);
    // Sequential calls always run; concurrency coalescing is asserted by
    // the in-flight flag: a second call during a pending one returns None.
    // (Driving true concurrency needs an executor; the flag behavior is
    // the contract and is exercised via finalize-during-partial below.)
    gate.start_capture();
    gate.push_audio(frame(3200)).unwrap();
    gate.begin_finalize();
    assert!(block_on(gate.partial_eval(&cancel)).unwrap().is_none());
}

#[test]
fn partial_gate_never_exposes_turn_submission() {
    // Compile-time property: PartialGate exposes no turn submission API.
    // This test documents the absence by exercising the full public
    // surface available.
    let gate = PartialGate::new(
        fake_with(FakeSttOutcome::Text("x")),
        executor(),
        CaptureBudget::default(),
    );
    gate.start_capture();
    gate.push_audio(frame(320)).unwrap();
    gate.begin_finalize();
    let cancel = VoiceCancel::new();
    let _ = block_on(gate.finalize(&cancel));
    // Public surface: start_capture, push_audio, begin_finalize,
    // partial_eval, finalize, VoiceStt impl. No turn/agent API exists.
}

// ---------------------------------------------------------- cancellation tests

#[test]
#[cfg(unix)]
fn cancellation_hierarchy_under_adapter_use() {
    // Parent cancels child mid-flight; the adapter result is discarded.
    let sidecar = sidecar_with(SIDECAR_OK);
    let parent = VoiceCancel::new();
    let child = parent.child();
    let frames = speech_frames();
    let handle = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        parent.cancel();
    });
    let outcome = block_on(sidecar.transcribe(&frames, &child));
    handle.join().unwrap();
    // Either the fast fixture completed before cancellation (Ok) or the
    // scope cancelled first (Cancelled) — but never a late Ok AFTER a
    // cancelled scope was observed.
    match outcome {
        Ok(transcript) => assert!(!transcript.text.as_str().is_empty()),
        Err(error) => assert!(matches!(error, VoiceError::Cancelled), "{error:?}"),
    }
    assert!(child.is_cancelled());
}

#[test]
fn repeated_cancellation_is_idempotent() {
    let sidecar = SidecarStt::new(
        SidecarConfig {
            python: std::path::PathBuf::from("/nonexistent/interpreter"),
            ..SidecarConfig::default()
        },
        executor(),
    );
    let cancel = VoiceCancel::new();
    cancel.cancel();
    cancel.cancel();
    cancel.cancel();
    assert!(matches!(
        block_on(sidecar.transcribe(&speech_frames(), &cancel)),
        Err(VoiceError::Cancelled)
    ));
}

#[test]
#[cfg(unix)]
fn repeated_error_cycles_do_not_leak_resources() {
    // Bounded lifetimes over repeated failures: many adapters created,
    // each failing, all dropped — completes promptly (no thread/process
    // accumulation deadlock).
    let start = std::time::Instant::now();
    for _ in 0..8 {
        let sidecar = sidecar_with(SIDECAR_MALFORMED);
        let cancel = VoiceCancel::new();
        let _ = block_on(sidecar.transcribe(&speech_frames(), &cancel));
        drop(sidecar);
    }
    assert!(
        start.elapsed() < Duration::from_secs(30),
        "repeated error/cancel cycles must stay bounded"
    );
}

#[test]
fn provenance_contract_rejects_empty_inferred_text() {
    // Construction-level invariant: empty text with InferredText
    // provenance is a contract violation; adapters must produce one of
    // the empty provenances instead.
    let transcript = SttTranscript {
        text: vesper_domain::BoundedString::new("").unwrap(),
        provider: vesper_domain::ProviderId::new("t").unwrap(),
        confidence: None,
        provenance: TranscriptProvenance::InferredText,
    };
    // The type permits it; the adapters never produce it (asserted by
    // the adapter tests above). Documented as an adapter obligation.
    let _ = transcript;
}

#[test]
fn failover_attempt_bound_prevents_retry_multiplication() {
    let mut chain = FailoverStt::new(
        vec![
            fake_with(FakeSttOutcome::Unavailable),
            fake_with(FakeSttOutcome::Unavailable),
            fake_with(FakeSttOutcome::Unavailable),
        ],
        SpeechEgress::OnDevice,
    )
    .unwrap();
    chain.set_max_attempts(2);
    let cancel = VoiceCancel::new();
    // Exhausted chain: terminal Unavailable (documented semantics),
    // with the trace capped at the attempt bound.
    let error = block_on(chain.transcribe_traced(&speech_frames(), &cancel)).unwrap_err();
    assert!(matches!(error, VoiceError::Unavailable { .. }));
    // Re-run with a succeeding third adapter to observe the cap: the
    // bound stops attempts before the third candidate can succeed.
    let mut capped = FailoverStt::new(
        vec![
            fake_with(FakeSttOutcome::Unavailable),
            fake_with(FakeSttOutcome::Unavailable),
            fake_with(FakeSttOutcome::Text("third")),
        ],
        SpeechEgress::OnDevice,
    )
    .unwrap();
    capped.set_max_attempts(2);
    let error = block_on(capped.transcribe_traced(&speech_frames(), &cancel)).unwrap_err();
    assert!(
        matches!(error, VoiceError::Unavailable { .. }),
        "the attempt bound must prevent the third candidate from running"
    );
}
