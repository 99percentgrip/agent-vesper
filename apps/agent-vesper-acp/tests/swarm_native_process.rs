//! Explicit isolated ACP -> configured embeddings -> native Hive acceptance.
#![cfg(all(feature = "swarm", feature = "docker", unix))]
#![allow(dead_code)]
mod support;
use serde_json::{Value, json};
use std::{
    io::Write,
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use support::{ProcessHarness, read_http_request, write_sse};

struct Server {
    address: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn new(handler: impl Fn(&mut TcpStream, Value) + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let handler = Arc::new(handler);
        let thread = thread::spawn(move || {
            let mut children = Vec::new();
            while let Ok((mut stream, _)) = listener.accept() {
                if flag.load(Ordering::Acquire) {
                    break;
                }
                let handler = handler.clone();
                children.push(thread::spawn(move || {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(10)))
                        .unwrap();
                    let wire = read_http_request(&mut stream);
                    let body =
                        serde_json::from_str(wire.split_once("\r\n\r\n").unwrap().1).unwrap();
                    handler(&mut stream, body);
                }));
            }
            for child in children {
                child.join().unwrap();
            }
        });
        Self {
            address,
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let result = thread.join();
            if !std::thread::panicking() {
                result.unwrap();
            }
        }
    }
}

#[test]
#[ignore = "requires a real local container runtime and selected immutable image"]
fn native_acp_settings_run_executes_three_workers_and_captures_tool_continuations() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let barrier = Arc::new((Mutex::new(0usize), std::sync::Condvar::new()));
    let provider = Server::new(move |stream, body| {
        let messages = body["messages"].to_string();
        captured.lock().unwrap().push(body.clone());
        let delta = if messages.contains("Return only JSON") {
            json!({"content": r#"{"tasks":[{"prompt":"driver-A","required_capabilities":["read_file"]},{"prompt":"driver-B","required_capabilities":["read_file"]},{"prompt":"driver-C","required_capabilities":["read_file"]}]}"#})
        } else if body["tools"].as_array().is_none_or(Vec::is_empty) {
            for label in ["A", "B", "C"] {
                assert!(
                    messages.contains(&format!("native-acp-file-{label}")),
                    "ungrounded synthesis: {messages}"
                );
            }
            json!({"content":"grounded-native-acp-synthesis"})
        } else {
            let label = ["A", "B", "C"]
                .into_iter()
                .find(|label| messages.contains(&format!("driver-{label}")))
                .unwrap();
            if body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["role"] == "tool")
            {
                assert!(messages.contains(&format!("native-acp-file-{label}")));
                json!({"content":format!("native-acp-file-{label}")})
            } else {
                let (count, ready) = &*barrier;
                let mut count = count.lock().unwrap();
                let target = (*count / 3 + 1) * 3;
                *count += 1;
                ready.notify_all();
                let (count, wait) = ready
                    .wait_timeout_while(count, Duration::from_secs(10), |count| *count < target)
                    .unwrap();
                assert!(
                    !wait.timed_out() && *count >= target,
                    "native workers did not overlap"
                );
                json!({"tool_calls":[{"index":0,"id":format!("read-{label}"),"type":"function","function":{"name":"read_file","arguments":json!({"path":format!("{label}.txt")}).to_string()}}]})
            }
        };
        let finish = if delta.get("tool_calls").is_some() {
            "tool_calls"
        } else {
            "stop"
        };
        let body = format!(
            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"delta":delta}]}),
            json!({"choices":[{"delta":{},"finish_reason":finish}]})
        );
        write_sse(stream, &body);
    });
    let embedding_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = embedding_count.clone();
    let embedding = Server::new(move |stream, body| {
        assert!(body.get("input").is_some());
        count.fetch_add(1, Ordering::AcqRel);
        let body = json!({"data":[{"embedding":[1.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0]}]}).to_string();
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    });
    let cognition = tempfile::tempdir().unwrap();
    let global = tempfile::tempdir().unwrap();
    std::fs::write(cognition.path().join("embedding.json"), json!({"source":"lmstudio","endpoint":format!("http://{}/v1/embeddings",embedding.address),"model":"fixture-embedding","dimension":8}).to_string()).unwrap();
    let cli = std::env::var("VESPER_DOCKER_BIN").unwrap_or_else(|_| "docker".into());
    let selected = std::env::var("VESPER_DOCKER_IMAGE").expect("select the tested local image");
    let image = std::process::Command::new(&cli)
        .args(["image", "inspect", &selected, "--format", "{{.Id}}"])
        .output()
        .unwrap();
    assert!(image.status.success());
    let image = String::from_utf8(image.stdout).unwrap().trim().to_owned();
    let image = if image.len() == 64 && image.bytes().all(|b| b.is_ascii_hexdigit()) {
        format!("sha256:{image}")
    } else {
        image
    };
    // Only the container CLI inherits its normal storage/runtime environment;
    // the actual ACP process keeps fully isolated HOME/XDG/cognition roots.
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
    let wrapper = cognition.path().join("container-cli");
    let mut script = "#!/bin/sh\n".to_owned();
    for key in [
        "HOME",
        "XDG_DATA_HOME",
        "XDG_CONFIG_HOME",
        "XDG_RUNTIME_DIR",
    ] {
        if let Ok(value) = std::env::var(key) {
            script.push_str(&format!("export {key}={}\n", quote(&value)));
        } else {
            script.push_str(&format!("unset {key}\n"));
        }
    }
    script.push_str(&format!("exec {} \"$@\"\n", quote(&cli)));
    std::fs::write(&wrapper, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut process = ProcessHarness::spawn_with_environment(
        provider.address,
        [
            ("AGENT_VESPER_FULL_HARNESS", "1".into()),
            ("AGENT_VESPER_VRO_ENABLED", "0".into()),
            (
                "AGENT_VESPER_COGNITION_ROOT",
                cognition.path().display().to_string(),
            ),
            (
                "AGENT_VESPER_GLOBAL_COGNITION_ROOT",
                global.path().display().to_string(),
            ),
            ("VESPER_DOCKER_BIN", wrapper.display().to_string()),
        ],
    );
    let root = process.isolated_root().join("workspace");
    std::fs::create_dir(&root).unwrap();
    for label in ["A", "B", "C"] {
        std::fs::write(
            root.join(format!("{label}.txt")),
            format!("native-acp-file-{label}"),
        )
        .unwrap();
    }
    vesper_harness::web_settings::save(
        &root,
        &vesper_harness::web_settings::WebScopeConfig {
            driver_image: Some(image),
            ..Default::default()
        },
    )
    .unwrap();
    process
        .send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}}));
    process.response(1);
    process.send(json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":root,"mcpServers":[]}}));
    let session = process.response(2)["result"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    for (scope_index, scope) in ["isolated", "shared"].into_iter().enumerate() {
        for (step, command) in [
            "/settings swarm enabled on".to_owned(),
            format!("/settings swarm scope {scope}"),
            "/settings swarm save".into(),
            "/swarm run inspect the three native project files".into(),
        ]
        .into_iter()
        .enumerate()
        {
            let id = (3 + scope_index * 4 + step) as u64;
            process.prompt(id, &session, &command, &format!("swarm-{id}"));
            assert!(process.response(id).get("error").is_none());
        }
    }
    let output = support::update_texts(process.transcript(), "agent_message_chunk").join("\n");
    assert!(output.contains("grounded-native-acp-synthesis"), "{output}");
    assert!(output.contains("Worker artifacts:"));
    assert_eq!(requests.lock().unwrap().len(), 16);
    assert!(embedding_count.load(Ordering::Acquire) >= 10);
    assert!(vesper_harness::swarm_settings::load(&root).unwrap().enabled);
    process.finish();
}
