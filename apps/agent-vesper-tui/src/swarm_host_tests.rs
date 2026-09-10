//! Full TUI command/task/event composition, run in a child with isolated state.
use super::*;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

struct Server {
    address: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn new(handler: impl Fn(&mut TcpStream, Value) + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let handler = Arc::new(handler);
        let thread = std::thread::spawn(move || {
            let mut children = Vec::new();
            while let Ok((mut stream, _)) = listener.accept() {
                if flag.load(Ordering::Acquire) {
                    break;
                }
                let handler = handler.clone();
                children.push(std::thread::spawn(move || {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(10)))
                        .unwrap();
                    let mut wire = Vec::new();
                    let header = loop {
                        let mut byte = [0];
                        stream.read_exact(&mut byte).unwrap();
                        wire.push(byte[0]);
                        assert!(wire.len() <= 65536);
                        if wire.ends_with(b"\r\n\r\n") {
                            break String::from_utf8(wire).unwrap();
                        }
                    };
                    let length: usize = header
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap())
                        })
                        .unwrap();
                    assert!(length <= 1_048_576);
                    let mut body = vec![0; length];
                    stream.read_exact(&mut body).unwrap();
                    handler(&mut stream, serde_json::from_slice(&body).unwrap());
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
fn respond(stream: &mut TcpStream, kind: &str, body: &str) {
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
}

#[test]
#[ignore = "requires real container runtime and bundled Landlock image; all state is isolated"]
fn native_tui_swarm_settings_task_and_history_round_trip() {
    if std::env::var("VESPER_TUI_SWARM_CHILD").as_deref() == Ok("1") {
        tokio::runtime::Runtime::new().unwrap().block_on(child());
        return;
    }
    let requests = Arc::new(AtomicUsize::new(0));
    let count = requests.clone();
    let barrier = Arc::new((std::sync::Mutex::new(0usize), std::sync::Condvar::new()));
    let provider = Server::new(move |stream, body| {
        count.fetch_add(1, Ordering::SeqCst);
        let messages = body["messages"].to_string();
        let delta = if messages.contains("Return only JSON") {
            json!({"content":r#"{"tasks":[{"prompt":"driver-A","required_capabilities":["read_file"]},{"prompt":"driver-B","required_capabilities":["read_file"]},{"prompt":"driver-C","required_capabilities":["read_file"]}]}"#})
        } else if body["tools"].as_array().is_none_or(Vec::is_empty) {
            for label in ["A", "B", "C"] {
                assert!(messages.contains(&format!("native-tui-file-{label}")));
            }
            json!({"content":"grounded-native-tui-synthesis"})
        } else {
            let label = ["A", "B", "C"]
                .into_iter()
                .find(|label| messages.contains(&format!("driver-{label}")))
                .unwrap();
            if body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["role"] == "tool")
            {
                assert!(messages.contains(&format!("native-tui-file-{label}")));
                json!({"content":format!("native-tui-file-{label}")})
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
                    "three workers must overlap"
                );
                json!({"tool_calls":[{"index":0,"id":format!("read-{label}"),"type":"function","function":{"name":"read_file","arguments":json!({"path":format!("{label}.txt")}).to_string()}}]})
            }
        };
        let finish = if delta.get("tool_calls").is_some() {
            "tool_calls"
        } else {
            "stop"
        };
        respond(
            stream,
            "text/event-stream",
            &format!(
                "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"delta":delta}]}),
                json!({"choices":[{"delta":{},"finish_reason":finish}]})
            ),
        );
    });
    let embeddings = Arc::new(AtomicUsize::new(0));
    let count = embeddings.clone();
    let embedding = Server::new(move |stream, body| {
        assert!(body.get("input").is_some());
        count.fetch_add(1, Ordering::SeqCst);
        respond(
            stream,
            "application/json",
            &json!({"data":[{"embedding":[1.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0]}]}).to_string(),
        );
    });
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    std::fs::create_dir(&root).unwrap();
    let cognition = temp.path().join("cognition");
    std::fs::create_dir(&cognition).unwrap();
    std::fs::write(cognition.join("embedding.json"), json!({"source":"lmstudio","endpoint":format!("http://{}/v1/embeddings",embedding.address),"model":"fixture-embedding","dimension":8}).to_string()).unwrap();
    for label in ["A", "B", "C"] {
        std::fs::write(
            root.join(format!("{label}.txt")),
            format!("native-tui-file-{label}"),
        )
        .unwrap();
    }
    let cli = std::env::var("VESPER_DOCKER_BIN").unwrap_or_else(|_| "docker".into());
    let image = std::env::var("VESPER_DOCKER_IMAGE").expect("select bundled acceptance image");
    let inspected = std::process::Command::new(&cli)
        .args(["image", "inspect", &image, "--format", "{{.Id}}"])
        .output()
        .unwrap();
    assert!(inspected.status.success());
    let mut image = String::from_utf8(inspected.stdout)
        .unwrap()
        .trim()
        .to_owned();
    if image.len() == 64 {
        image = format!("sha256:{image}");
    }
    vesper_harness::web_settings::save(
        &root,
        &vesper_harness::web_settings::WebScopeConfig {
            driver_image: Some(image),
            ..Default::default()
        },
    )
    .unwrap();
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
    let wrapper = temp.path().join("container-cli");
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
    let mut child = std::process::Command::new(std::env::current_exe().unwrap());
    child
        .args([
            "--exact",
            "swarm_host_tests::native_tui_swarm_settings_task_and_history_round_trip",
            "--ignored",
            "--nocapture",
        ])
        .current_dir(&root)
        .env_clear()
        .env("HOME", temp.path().join("home"))
        .env("XDG_CONFIG_HOME", temp.path().join("config"))
        .env("XDG_DATA_HOME", temp.path().join("data"))
        .env("XDG_CACHE_HOME", temp.path().join("cache"))
        .env("XDG_STATE_HOME", temp.path().join("state"))
        .env("VESPER_TUI_SWARM_CHILD", "1")
        .env("VESPER_DOCKER_BIN", wrapper)
        .env("AGENT_VESPER_COGNITION_ROOT", cognition)
        .env("ZAI_API_KEY", "synthetic-tui-canary")
        .env(
            "AGENT_VESPER_GLM_BASE_URL",
            format!("http://{}/v4", provider.address),
        )
        .env("AGENT_VESPER_ALLOW_INSECURE_LOOPBACK", "1");
    if let Ok(path) = std::env::var("PATH") {
        child.env("PATH", path);
    }
    let result = child.output().unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!String::from_utf8_lossy(&result.stderr).contains("synthetic-tui-canary"));
    assert_eq!(requests.load(Ordering::SeqCst), 16);
    assert!(embeddings.load(Ordering::SeqCst) >= 10);
}

async fn child() {
    use vesper_provider::ProviderSuperpowers;
    let registry = Arc::new(vesper_runtime::ProviderRegistry::new());
    register_default_providers(&registry).await.unwrap();
    let provider = ProviderId::new("zai").unwrap();
    let surface = ProviderSuperpowerSurface::new(
        provider.clone(),
        vesper_provider_glm::GlmFactory::default().superpowers(),
    );
    let mut config = build_agent_config(&provider).unwrap();
    let fixture_endpoint = std::env::var("AGENT_VESPER_GLM_BASE_URL").unwrap();
    assert!(fixture_endpoint.starts_with("http://127.0.0.1:"));
    for (key, value) in [
        ("zai:endpoint-plan", json!("custom")),
        ("zai:base-url", json!(fixture_endpoint)),
        ("zai:allow-insecure-http", json!(true)),
    ] {
        config
            .provider_configuration
            .values
            .values
            .insert(key, value)
            .unwrap();
    }
    let agent = Arc::new(AgentLoop::new(
        registry.clone(),
        ToolRegistry::parity_default(),
        config,
    ));
    let root = std::env::current_dir().unwrap();
    let tools: Arc<dyn vesper_agent::ToolService> = Arc::new(TuiToolService::new(
        Arc::new(MemoryStores::open_default()),
        root.join(".agent-vesper/cron"),
        root.join(".agent-vesper/plugins"),
        None,
    ));
    let permission: Arc<dyn vesper_agent::PermissionPort> =
        Arc::new(vesper_agent::DenyPermissionPort);
    let cognition = CognitionBundle {
        engine: None,
        global_engine: None,
        root_display: "isolated".into(),
        global_root_display: "isolated".into(),
        project_display: "fixture".into(),
        root: std::env::var_os("AGENT_VESPER_COGNITION_ROOT")
            .unwrap()
            .into(),
        embedder: None,
        credential_source: Arc::new(vesper_provider_glm::EnvironmentCredentialSource),
    };
    let mut session = tests::fresh_tui_session_for_trajectory_tests();
    session.state.controls.endpoint_plan = "custom".into();
    let resolved = turn_configuration(&agent, &session.state, &surface).unwrap();
    assert_eq!(
        resolved
            .provider_configuration
            .values
            .values
            .get("zai:endpoint-plan"),
        Some(&json!("custom"))
    );
    assert_eq!(
        resolved
            .provider_configuration
            .values
            .values
            .get("zai:base-url"),
        Some(&json!(fixture_endpoint))
    );
    for scope in ["isolated", "shared"] {
        for command in [
            "settings enabled on".to_owned(),
            format!("settings scope {scope}"),
            "settings save".into(),
            "run inspect the three files".into(),
        ] {
            swarm_host::command(
                &command,
                &registry,
                &agent,
                &tools,
                &permission,
                &mut session,
                &surface,
                &cognition,
            )
            .unwrap();
        }
        assert!(session.agent_running);
        let task = session.agent_task.take().unwrap();
        tokio::time::timeout(Duration::from_secs(60), task)
            .await
            .unwrap()
            .unwrap();
        drain_agent_event(&mut session);
        assert!(!session.agent_running);
        assert!(session.state.swarm_cancel.is_none());
        let history = serde_json::to_string(&session.conversation).unwrap();
        assert!(
            history.contains("grounded-native-tui-synthesis")
                && history.contains("Worker artifacts:"),
            "{history}"
        );
    }
    assert!(swarm_host::shutdown().await);
    assert_eq!(session.conversation.len(), 4);
}
