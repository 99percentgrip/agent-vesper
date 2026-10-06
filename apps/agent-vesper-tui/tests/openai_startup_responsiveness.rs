//! OpenAI startup must keep the real TUI event loop responsive while account
//! model discovery is stalled, then apply either a completed catalog or a
//! discovery failure without freezing later input.
#![cfg(unix)]
#![allow(unsafe_code)]

use std::fs::File;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::fd::FromRawFd;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const WIDTH: u16 = 120;
const HEIGHT: u16 = 36;
const LOADING: &[u8] = b"Loading OpenAI account models";
const LEGACY_LOADING: &[u8] = b"Loading account models";
const TYPED_AND_PASTED: &[u8] = b"ZQPASTETOKEN";
const AFTER_FAILURE: &[u8] = b"POSTFAIL";
const CATALOG_APPLIED: &[u8] = b"Select a model";
const DISCOVERY_FAILED: &[u8] = b"HTTP 500";

struct Release {
    request_seen: bool,
    released: bool,
    succeed: bool,
}

fn open_pty() -> io::Result<(File, File)> {
    let mut master = -1;
    let mut slave = -1;
    // macOS `openpty` takes `*mut winsize`; Linux takes `*const winsize`.
    #[cfg(target_os = "macos")]
    let mut size = libc::winsize {
        ws_row: HEIGHT,
        ws_col: WIDTH,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    #[cfg(not(target_os = "macos"))]
    let size = libc::winsize {
        ws_row: HEIGHT,
        ws_col: WIDTH,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    #[cfg(target_os = "macos")]
    let size_ptr = &raw mut size;
    #[cfg(not(target_os = "macos"))]
    let size_ptr = &raw const size;
    let result = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            size_ptr,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((unsafe { File::from_raw_fd(master) }, unsafe {
        File::from_raw_fd(slave)
    }))
}

fn spawn_reader(mut master: File, output: Arc<Mutex<Vec<u8>>>) {
    thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            match master.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    let mut bytes = output.lock().expect("pty buffer");
                    bytes.extend_from_slice(&buffer[..count]);
                    if bytes.windows(4).any(|window| window == b"\x1b[6n") {
                        drop(bytes);
                        let _ = master.write_all(b"\x1b[24;1R");
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });
}

fn contains(output: &Mutex<Vec<u8>>, needle: &[u8]) -> bool {
    let bytes = output.lock().expect("pty buffer");
    !needle.is_empty() && bytes.windows(needle.len()).any(|window| window == needle)
}

fn wait_for(output: &Mutex<Vec<u8>>, needle: &[u8], timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if contains(output, needle) {
            return true;
        }
        thread::sleep(Duration::from_millis(30));
    }
    false
}

fn screen(output: &Mutex<Vec<u8>>) -> String {
    String::from_utf8_lossy(&output.lock().expect("pty buffer")).into_owned()
}

fn serve(listener: TcpListener, release: Arc<(Mutex<Release>, Condvar)>) {
    listener
        .set_nonblocking(true)
        .expect("fixture listener mode");
    let started = Instant::now();
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if started.elapsed() > Duration::from_secs(25) {
                    return;
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => panic!("fixture accept failed: {error}"),
        }
    };
    stream.set_nonblocking(false).expect("fixture stream mode");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("fixture read timeout");
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) => return,
            Ok(count) => request.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("fixture request read failed: {error}"),
        }
    }
    assert!(
        std::str::from_utf8(&request)
            .unwrap_or("")
            .contains("GET /models"),
        "discovery fixture did not receive GET /models: {}",
        String::from_utf8_lossy(&request)
    );
    let (lock, gate) = &*release;
    let succeed = {
        let mut state = lock.lock().expect("release lock");
        state.request_seen = true;
        gate.notify_all();
        while !state.released {
            state = gate.wait(state).expect("release wait");
        }
        state.succeed
    };
    let body = if succeed {
        r#"{"data":[{"id":"gpt-5.4"}]}"#
    } else {
        r#"{"error":{"message":"fixture discovery failure"}}"#
    };
    let status = if succeed {
        "200 OK"
    } else {
        "500 Internal Server Error"
    };
    write_http(&mut stream, status, body);
}

fn write_http(stream: &mut TcpStream, status: &str, body: &str) {
    let message = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(message.as_bytes())
        .expect("fixture response");
}

struct TuiProcess {
    child: Child,
    input: File,
    output: Arc<Mutex<Vec<u8>>>,
}

fn launch(
    endpoint: &str,
    home: &std::path::Path,
    xai_endpoint: Option<&str>,
) -> io::Result<TuiProcess> {
    use std::os::unix::fs::PermissionsExt;
    for provider in ["openai", "xai"] {
        let path = home.join(format!("{provider}-credentials.json"));
        let vault = serde_json::json!({"credentials": {
            provider: {"native-auth": "{\"mode\":\"signed-out\"}"}
        }});
        std::fs::write(&path, vault.to_string())?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    let (master, slave) = open_pty()?;
    let input = master.try_clone()?;
    let stdin = slave.try_clone()?;
    let stdout = slave.try_clone()?;
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent-vesper-tui"));
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .arg("--resume")
        .arg("missing-openai-startup-session")
        .current_dir(home)
        .env("TERM", "xterm-256color")
        .env("AGENT_VESPER_PROVIDER", "openai")
        .env("AGENT_VESPER_HOME", home)
        .env("AGENT_VESPER_OPENAI_TEST_URL", endpoint)
        .env(
            "AGENT_VESPER_OPENAI_CREDENTIALS_PATH",
            home.join("openai-credentials.json"),
        )
        .env(
            "AGENT_VESPER_XAI_CREDENTIALS_PATH",
            home.join("xai-credentials.json"),
        )
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env(
            "AGENT_VESPER_GLOBAL_COGNITION_ROOT",
            home.join("global-cognition"),
        )
        .env("HOME", home)
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(slave));
    if let Some(endpoint) = xai_endpoint {
        command.env("AGENT_VESPER_XAI_TEST_URL", endpoint);
    }
    let child = command.spawn()?;
    let output = Arc::new(Mutex::new(Vec::new()));
    spawn_reader(master, Arc::clone(&output));
    Ok(TuiProcess {
        child,
        input,
        output,
    })
}

fn stop(child: &mut Child, input: &mut File) {
    let _ = input.write_all(b"\x04");
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn wait_for_any(output: &Mutex<Vec<u8>>, needles: &[&[u8]], timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if needles.iter().any(|needle| contains(output, needle)) {
            return true;
        }
        thread::sleep(Duration::from_millis(30));
    }
    false
}

fn input_while_discovery_is_pending(output: &Mutex<Vec<u8>>, input: &mut File) {
    assert!(
        wait_for_any(output, &[LOADING, LEGACY_LOADING], Duration::from_secs(20)),
        "OpenAI startup did not reach a model-loading frame:\n{}",
        screen(output)
    );
    assert!(
        contains(output, LOADING),
        "startup did not show the truthful OpenAI loading status:\n{}",
        screen(output)
    );
    input
        .write_all(b"ZQ\x1b[200~PASTETOKEN\x1b[201~")
        .expect("send startup input");
    let started = Instant::now();
    let visible = wait_for(output, TYPED_AND_PASTED, Duration::from_millis(1200));
    assert!(
        visible,
        "typed/pasted input was not visible within {:?} while discovery was still pending (elapsed {:?}):\n{}",
        Duration::from_millis(1200),
        started.elapsed(),
        screen(output)
    );
    assert!(
        !contains(output, CATALOG_APPLIED) && !contains(output, DISCOVERY_FAILED),
        "discovery settled before the stalled fixture was released:\n{}",
        screen(output)
    );
}

fn release_discovery(release: &Arc<(Mutex<Release>, Condvar)>, succeed: bool) {
    let (lock, gate) = &**release;
    let started = Instant::now();
    let mut state = lock.lock().expect("release lock");
    while !state.request_seen && started.elapsed() < Duration::from_secs(12) {
        let wait = gate
            .wait_timeout(state, Duration::from_millis(100))
            .expect("request wait");
        state = wait.0;
        if wait.1.timed_out() && started.elapsed() >= Duration::from_secs(12) {
            break;
        }
    }
    assert!(
        state.request_seen,
        "stalled discovery endpoint received no request"
    );
    state.succeed = succeed;
    state.released = true;
    gate.notify_all();
}

fn run_case(succeed: bool) {
    let home = tempfile::tempdir().expect("home");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let endpoint = format!("http://{}/models", listener.local_addr().expect("addr"));
    let release = Arc::new((
        Mutex::new(Release {
            request_seen: false,
            released: false,
            succeed,
        }),
        Condvar::new(),
    ));
    let server_release = Arc::clone(&release);
    let server = thread::spawn(move || serve(listener, server_release));
    let TuiProcess {
        mut child,
        mut input,
        output,
    } = launch(&endpoint, home.path(), None).expect("launch");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        input_while_discovery_is_pending(&output, &mut input);
        release_discovery(&release, succeed);
        if succeed {
            assert!(
                wait_for(&output, CATALOG_APPLIED, Duration::from_secs(5)),
                "completed discovery did not update the model prompt:\n{}",
                screen(&output)
            );
        } else {
            assert!(
                wait_for(&output, DISCOVERY_FAILED, Duration::from_secs(5)),
                "failed discovery did not update status:\n{}",
                screen(&output)
            );
            input
                .write_all(b"\x1b[200~POSTFAIL\x1b[201~")
                .expect("send post-failure input");
            assert!(
                wait_for(&output, AFTER_FAILURE, Duration::from_millis(1200)),
                "input stayed blocked after discovery failure:\n{}",
                screen(&output)
            );
        }
    }));
    {
        let (lock, gate) = &*release;
        lock.lock().expect("release lock").released = true;
        gate.notify_all();
    }
    stop(&mut child, &mut input);
    server.join().expect("discovery fixture settlement");
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[test]
fn stalled_openai_discovery_accepts_input_then_applies_catalog() {
    run_case(true);
}

#[test]
fn stalled_openai_discovery_failure_leaves_input_responsive() {
    run_case(false);
}

#[test]
fn openai_startup_does_not_wait_for_unselected_xai_discovery() {
    let home = tempfile::tempdir().expect("home");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let endpoint = format!("http://{}/responses", listener.local_addr().expect("addr"));
    let contacted = Arc::new(AtomicBool::new(false));
    let server_contacted = Arc::clone(&contacted);
    let finished = Arc::new(AtomicBool::new(false));
    let server_finished = Arc::clone(&finished);
    let xai_server = thread::spawn(move || {
        listener.set_nonblocking(true).expect("listener");
        let started = Instant::now();
        while !server_finished.load(Ordering::Acquire)
            && started.elapsed() < Duration::from_secs(12)
        {
            if listener.accept().is_ok() {
                server_contacted.store(true, Ordering::Release);
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
    });
    let openai_listener = TcpListener::bind("127.0.0.1:0").expect("OpenAI bind");
    let openai_endpoint = format!(
        "http://{}/models",
        openai_listener.local_addr().expect("addr")
    );
    let release = Arc::new((
        Mutex::new(Release {
            request_seen: false,
            released: false,
            succeed: true,
        }),
        Condvar::new(),
    ));
    let server_release = Arc::clone(&release);
    let openai_server = thread::spawn(move || serve(openai_listener, server_release));
    let TuiProcess {
        mut child,
        mut input,
        output,
    } = launch(&openai_endpoint, home.path(), Some(&endpoint)).expect("launch");
    let visible = wait_for(&output, LOADING, Duration::from_secs(8));
    {
        let (lock, gate) = &*release;
        lock.lock().expect("release lock").released = true;
        gate.notify_all();
    }
    // Let the pending catalog response settle before closing its PTY child.
    let _ = wait_for(&output, CATALOG_APPLIED, Duration::from_secs(5));
    stop(&mut child, &mut input);
    openai_server.join().expect("OpenAI fixture settlement");
    finished.store(true, Ordering::Release);
    xai_server.join().expect("xAI fixture settlement");
    assert!(
        visible,
        "TUI did not render while unselected xAI discovery was stalled:\n{}",
        screen(&output)
    );
    assert!(
        !contacted.load(Ordering::Acquire),
        "unselected xAI discovery was contacted before the TUI could render"
    );
}
