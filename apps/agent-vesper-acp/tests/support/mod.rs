use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Condvar, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

pub const CANARY: &str = "vesper-stage41-secret-canary";
const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct ReaderGate(Arc<(Mutex<bool>, Condvar)>);

impl ReaderGate {
    pub fn running() -> Self {
        Self(Arc::new((Mutex::new(true), Condvar::new())))
    }

    pub fn pause(&self) {
        *self.0.0.lock().unwrap() = false;
    }

    pub fn resume(&self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }

    fn wait(&self) {
        let (lock, ready) = &*self.0;
        let mut running = lock.lock().unwrap();
        while !*running {
            running = ready.wait(running).unwrap();
        }
    }
}

pub struct ProcessHarness {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: mpsc::Receiver<String>,
    transcript: Vec<Value>,
    pending_responses: BTreeMap<String, Value>,
    reader_gate: ReaderGate,
    reader: Option<thread::JoinHandle<()>>,
    stderr: Option<thread::JoinHandle<String>>,
    temp: PathBuf,
}

impl ProcessHarness {
    pub fn spawn(address: std::net::SocketAddr) -> Self {
        Self::spawn_binary(
            env!("CARGO_BIN_EXE_agent-vesper-acp"),
            address,
            std::iter::empty::<(&str, String)>(),
        )
    }

    pub fn spawn_test_driver(address: std::net::SocketAddr, gate: std::net::SocketAddr) -> Self {
        Self::spawn_binary(
            env!("CARGO_BIN_EXE_agent-vesper-acp-test-driver"),
            address,
            [("AGENT_VESPER_TEST_DISPATCH_GATE", gate.to_string())],
        )
    }

    pub fn spawn_with_environment(
        address: std::net::SocketAddr,
        extra: impl IntoIterator<Item = (&'static str, String)>,
    ) -> Self {
        Self::spawn_binary(env!("CARGO_BIN_EXE_agent-vesper-acp"), address, extra)
    }

    fn spawn_binary(
        binary: &str,
        address: std::net::SocketAddr,
        extra: impl IntoIterator<Item = (&'static str, String)>,
    ) -> Self {
        let temp = std::env::temp_dir().join(format!(
            "agent-vesper-stage41-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        fs::create_dir_all(&temp).unwrap();
        let mut command = Command::new(binary);
        command
            .env_clear()
            .env("HOME", &temp)
            .env("XDG_CONFIG_HOME", temp.join("config"))
            .env("XDG_CACHE_HOME", temp.join("cache"))
            .env("XDG_DATA_HOME", temp.join("data"))
            .env("XDG_STATE_HOME", temp.join("state"))
            .env("ZAI_API_KEY", CANARY)
            .env("AGENT_VESPER_GLM_BASE_URL", format!("http://{address}/v4"))
            .env("AGENT_VESPER_ALLOW_INSECURE_LOOPBACK", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in extra {
            command.env(key, value);
        }
        let mut child = command.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let child_stderr = child.stderr.take().unwrap();
        let gate = ReaderGate::running();
        let reader_gate = gate.clone();
        let (sender, lines) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                reader_gate.wait();
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let _ = sender.send(line.trim_end().to_owned());
                    }
                }
            }
        });
        let stderr = thread::spawn(move || {
            let mut output = String::new();
            let _ = BufReader::new(child_stderr).read_to_string(&mut output);
            output
        });
        Self {
            child,
            stdin: Some(stdin),
            lines,
            transcript: Vec::new(),
            pending_responses: BTreeMap::new(),
            reader_gate: gate,
            reader: Some(reader),
            stderr: Some(stderr),
            temp,
        }
    }

    pub fn initialize_and_new_session(&mut self) -> String {
        self.send(
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}}),
        );
        assert_eq!(self.response(1)["result"]["protocolVersion"], 1);
        self.send(json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":"/tmp","mcpServers":[]}}));
        self.response(2)["result"]["sessionId"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    pub fn send(&mut self, value: Value) {
        let stdin = self.stdin.as_mut().unwrap();
        serde_json::to_writer(&mut *stdin, &value).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
    }

    pub fn prompt(&mut self, id: u64, session: &str, text: &str, message_id: &str) {
        self.send(json!({
            "jsonrpc":"2.0","id":id,"method":"session/prompt",
            "params":{"sessionId":session,"prompt":[{"type":"text","text":text}],
            "_meta":{"userMessageId":message_id}}
        }));
    }

    pub fn response(&mut self, id: u64) -> Value {
        let key = id.to_string();
        if let Some(value) = self.pending_responses.remove(&key) {
            return value;
        }
        loop {
            let value = self.next();
            if value["id"] == id {
                return value;
            }
            if !value["id"].is_null() {
                self.pending_responses
                    .insert(value["id"].to_string(), value);
            }
        }
    }

    pub fn next(&mut self) -> Value {
        let line = self.lines.recv_timeout(TIMEOUT).unwrap();
        assert!(!line.contains(CANARY), "secret reached ACP stdout");
        let value: Value = serde_json::from_str(&line).expect("stdout contained non-JSON text");
        self.transcript.push(value.clone());
        value
    }

    pub fn transcript(&self) -> &[Value] {
        &self.transcript
    }

    pub fn pause_stdout(&self) {
        self.reader_gate.pause();
    }

    pub fn resume_stdout(&self) {
        self.reader_gate.resume();
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn isolated_root(&self) -> &std::path::Path {
        &self.temp
    }

    pub fn finish(self) {
        let _ = self.finish_and_capture();
    }

    pub fn finish_and_capture(mut self) -> (Vec<Value>, String) {
        self.stdin.take();
        self.reader_gate.resume();
        wait_for_exit(&mut self.child);
        if let Some(reader) = self.reader.take() {
            reader.join().unwrap();
        }
        let stderr = self.stderr.take().unwrap().join().unwrap();
        assert!(!stderr.contains(CANARY), "secret reached ACP stderr");
        let _ = fs::remove_dir_all(&self.temp);
        (std::mem::take(&mut self.transcript), stderr)
    }
}

impl Drop for ProcessHarness {
    fn drop(&mut self) {
        self.reader_gate.resume();
        self.stdin.take();
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        if let Some(stderr) = self.stderr.take() {
            let _ = stderr.join();
        }
        let _ = fs::remove_dir_all(&self.temp);
    }
}

pub fn read_http_request(stream: &mut impl Read) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream.read(&mut buffer).unwrap();
        assert!(read > 0, "HTTP request ended before headers");
        bytes.extend_from_slice(&buffer[..read]);
        let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let header_end = header_end + 4;
        let headers = String::from_utf8_lossy(&bytes[..header_end]).to_lowercase();
        let content_length = headers
            .lines()
            .find_map(|line| line.strip_prefix("content-length: "))
            .unwrap()
            .trim()
            .parse::<usize>()
            .unwrap();
        if bytes.len() >= header_end + content_length {
            return String::from_utf8_lossy(&bytes[..header_end + content_length]).into_owned();
        }
    }
}

pub fn write_sse(stream: &mut impl Write, body: &str) {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .unwrap();
    stream.flush().unwrap();
}

pub fn successful_body(content: &str) -> String {
    format!(
        "data: {{\"choices\":[{{\"delta\":{{\"content\":{}}},\"finish_reason\":\"stop\"}}],\"usage\":{{\"prompt_tokens\":2,\"completion_tokens\":3,\"total_tokens\":5}}}}\n\ndata: [DONE]\n\n",
        serde_json::to_string(content).unwrap()
    )
}

pub fn terminal_count(values: &[Value], id: u64) -> usize {
    values.iter().filter(|value| value["id"] == id).count()
}

pub fn update_texts(values: &[Value], kind: &str) -> Vec<String> {
    values
        .iter()
        .filter(|value| value["params"]["update"]["sessionUpdate"] == kind)
        .filter_map(|value| {
            value["params"]["update"]["content"]["text"]
                .as_str()
                .map(str::to_owned)
        })
        .collect()
}

#[cfg(target_os = "linux")]
pub fn rss_kib(pid: u32) -> u64 {
    fs::read_to_string(format!("/proc/{pid}/status"))
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|value| value.split_whitespace().next())
        .unwrap()
        .parse()
        .unwrap()
}

fn wait_for_exit(child: &mut Child) {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            return;
        }
        assert!(Instant::now() < deadline, "ACP process did not exit on EOF");
        thread::sleep(Duration::from_millis(10));
    }
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}
