use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::support::ProcessHarness;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use vesper_domain::{
    BoundedString, ContentPart, ContentText, EndpointId, ExtensionMap, ExtensionNamespace,
    MessageId, MessageRole, ModelId, NormalizedUsage, ProviderId, QualifiedModelId, Revision,
    SchemaVersion, SessionId, SessionLineage, SessionOperatingMode, SessionPermissionMode,
    UsageMode, VersionedExtensionEnvelope, WorkspaceRoot,
};
use vesper_sessions::{PersistedProviderConfiguration, VesperSessionV1};

const PRIVATE_SYSTEM: &str = "private-system-canary";
const PRIVATE_REASONING: &str = "private-reasoning-canary";
const RAW_SECRET: &str = "raw-provider-key-canary";

struct SyntheticStores {
    root: PathBuf,
    vesper: PathBuf,
    legacy: PathBuf,
}

impl SyntheticStores {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "agent-vesper-stage5-persistence-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        let vesper = root.join("vesper");
        let legacy = root.join("legacy");
        fs::create_dir_all(&vesper).unwrap();
        fs::create_dir_all(&legacy).unwrap();
        let stores = Self {
            root,
            vesper,
            legacy,
        };
        stores.populate();
        stores
    }

    fn populate(&self) {
        write_json(
            &self.legacy.join("minimal.json"),
            &json!({
                "cwd": "/workspace",
                "model": "glm-5.2",
                "api_endpoint": "coding",
                "messages": [
                    {"role": "system", "content": PRIVATE_SYSTEM},
                    {"role": "user", "content": "visible-user"},
                    {"role": "assistant", "content": "visible-assistant",
                     "reasoning_content": PRIVATE_REASONING},
                    {"role": "tool", "tool_call_id": "orphan", "content": "private-tool"}
                ]
            }),
        );
        write_json(
            &self.legacy.join("minimal.meta"),
            &json!({
                "session_id": "minimal",
                "cwd": "/workspace",
                "title": "Minimal legacy",
                "updated_at": "2026-07-30T10:00:00Z",
                "model": "glm-5.2",
                "provider": "zai"
            }),
        );
        write_json(
            &self.legacy.join("unknown.json"),
            &json!({
                "cwd": "/workspace",
                "model": "glm-5.2",
                "api_endpoint": "coding",
                "messages": [{"role": "user", "content": "unknown-visible"}],
                "future_field": {"nested": [1, 2, 3]}
            }),
        );
        write_json(
            &self.legacy.join("nometa.json"),
            &json!({
                "cwd": "/workspace",
                "title": "JSON fallback",
                "model": "glm-5.2",
                "api_endpoint": "coding",
                "messages": [{"role": "assistant", "content": "fallback-visible"}]
            }),
        );
        fs::write(self.legacy.join("corrupt.json"), b"{corrupt").unwrap();
        write_json(
            &self.legacy.join("unsupported.json"),
            &json!({"version": 99, "cwd": "/workspace"}),
        );
        write_json(
            &self.legacy.join("collision.json"),
            &json!({
                "cwd": "/workspace",
                "model": "glm-5.2",
                "api_endpoint": "coding",
                "messages": [{"role": "assistant", "content": "legacy-collision"}]
            }),
        );
        write_json(
            &self.vesper.join("collision.json"),
            &serde_json::to_value(vesper_record("collision", "agent-vesper-collision")).unwrap(),
        );

        let mut raw_secret =
            serde_json::to_value(vesper_record("raw-secret", "never-replay")).unwrap();
        raw_secret["provider_configuration"]["values"]["values"] =
            json!({"provider:api-key": RAW_SECRET});
        write_json(&self.vesper.join("raw-secret.json"), &raw_secret);
    }

    fn environment(&self) -> Vec<(&'static str, String)> {
        vec![
            ("AGENT_VESPER_ENABLE_SESSION_READS", "1".into()),
            ("AGENT_VESPER_ENABLE_LEGACY_SESSION_READS", "1".into()),
            (
                "AGENT_VESPER_SESSION_ROOT",
                self.vesper.to_string_lossy().into_owned(),
            ),
            (
                "AGENT_VESPER_LEGACY_SESSION_ROOT",
                self.legacy.to_string_lossy().into_owned(),
            ),
            ("AGENT_VESPER_SESSION_MAX_BYTES", (1024 * 1024).to_string()),
            ("AGENT_VESPER_SESSION_MAX_ENTRIES", "128".into()),
        ]
    }

    fn roots(&self) -> [&Path; 2] {
        [&self.vesper, &self.legacy]
    }
}

impl Drop for SyntheticStores {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileState {
    sha256: [u8; 32],
    bytes: u64,
    modified: Option<SystemTime>,
}

#[test]
fn listing_load_resume_and_replay_visibility_are_disk_invariant() {
    let stores = SyntheticStores::new();
    run_vector(&stores, "listing", |process| {
        initialize(process);
        process.send(
            json!({"jsonrpc":"2.0","id":2,"method":"session/list","params":{"cwd":"/workspace"}}),
        );
        let response = process.response(2);
        let sessions = response["result"]["sessions"].as_array().unwrap();
        for id in ["minimal", "unknown", "nometa", "collision"] {
            assert!(sessions.iter().any(|entry| entry["sessionId"] == id));
        }
        assert_eq!(
            sessions
                .iter()
                .filter(|entry| entry["sessionId"] == "collision")
                .count(),
            1
        );
    });
    run_vector(&stores, "legacy-minimal-load-and-safe-replay", |process| {
        initialize(process);
        load(process, 3, "minimal");
        let rendered = serde_json::to_string(process.transcript()).unwrap();
        assert!(rendered.contains("visible-user"));
        assert!(rendered.contains("visible-assistant"));
        for forbidden in [PRIVATE_SYSTEM, PRIVATE_REASONING, "private-tool"] {
            assert!(!rendered.contains(forbidden));
        }
    });
    run_vector(&stores, "resume", |process| {
        initialize(process);
        resume(process, 4, "minimal");
        assert!(
            serde_json::to_string(process.transcript())
                .unwrap()
                .contains("visible-assistant")
        );
    });
    run_vector(&stores, "unknown-fields", |process| {
        initialize(process);
        load(process, 5, "unknown");
        let rendered = serde_json::to_string(process.transcript()).unwrap();
        assert!(rendered.contains("unknown-visible"));
        assert!(!rendered.contains("future_field"));
    });
    run_vector(&stores, "missing-metadata-fallback", |process| {
        initialize(process);
        load(process, 6, "nometa");
        assert!(
            serde_json::to_string(process.transcript())
                .unwrap()
                .contains("fallback-visible")
        );
    });
}

#[test]
fn fork_close_and_cross_source_collision_are_disk_invariant() {
    let stores = SyntheticStores::new();
    run_vector(&stores, "fork", |process| {
        initialize(process);
        load(process, 10, "minimal");
        process.send(json!({"jsonrpc":"2.0","id":11,"method":"session/fork","params":{"sessionId":"minimal","cwd":"/workspace"}}));
        let child = process.response(11)["result"]["sessionId"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_ne!(child, "minimal");
    });
    run_vector(&stores, "close", |process| {
        initialize(process);
        load(process, 12, "minimal");
        process.send(json!({"jsonrpc":"2.0","id":13,"method":"session/close","params":{"sessionId":"minimal"}}));
        assert!(process.response(13).get("error").is_none());
        process.send(
            json!({"jsonrpc":"2.0","id":14,"method":"session/list","params":{"cwd":"/workspace"}}),
        );
        assert!(
            process.response(14)["result"]["sessions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["sessionId"] == "minimal")
        );
    });
    run_vector(&stores, "cross-source-collision", |process| {
        initialize(process);
        load(process, 15, "collision");
        let rendered = serde_json::to_string(process.transcript()).unwrap();
        assert!(rendered.contains("agent-vesper-collision"));
        assert!(!rendered.contains("legacy-collision"));
    });
}

#[test]
fn corrupt_unsupported_and_secret_bearing_records_fail_without_repair() {
    let stores = SyntheticStores::new();
    for (name, id, reason) in [
        ("corrupt", "corrupt", "persistent-session-corrupt"),
        (
            "unsupported-version",
            "unsupported",
            "persistent-session-unsupported-version",
        ),
        ("raw-secret", "raw-secret", "persistent-session-corrupt"),
    ] {
        run_vector(&stores, name, |process| {
            initialize(process);
            process.send(json!({
                "jsonrpc":"2.0","id":20,"method":"session/load",
                "params":{"sessionId":id,"cwd":"/workspace","mcpServers":[]}
            }));
            let response = process.response(20);
            assert_eq!(response["error"]["data"]["reason"], reason);
            let rendered = serde_json::to_string(process.transcript()).unwrap();
            assert!(!rendered.contains(RAW_SECRET));

            process.send(json!({"jsonrpc":"2.0","id":21,"method":"session/list","params":{"cwd":"/workspace"}}));
            assert!(process.response(21).get("error").is_none());
        });
    }
}

fn run_vector(stores: &SyntheticStores, name: &str, test: impl FnOnce(&mut ProcessHarness)) {
    let before = snapshot(stores.roots());
    let mut process = ProcessHarness::spawn_with_environment(
        "127.0.0.1:9".parse().unwrap(),
        stores.environment(),
    );
    test(&mut process);
    for directory in ["config", "cache", "data", "state"] {
        assert!(
            !process.isolated_root().join(directory).exists(),
            "{name}: application created {directory} state"
        );
    }
    let (transcript, stderr) = process.finish_and_capture();
    let after = snapshot(stores.roots());
    assert_eq!(after, before, "{name}: persistent disk changed");
    let rendered = serde_json::to_string(&transcript).unwrap();
    for canary in [RAW_SECRET, PRIVATE_SYSTEM, PRIVATE_REASONING] {
        assert!(!stderr.contains(canary), "{name}: canary reached stderr");
        assert!(!rendered.contains(canary), "{name}: canary reached stdout");
    }
}

fn initialize(process: &mut ProcessHarness) {
    process
        .send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}}));
    assert_eq!(process.response(1)["result"]["protocolVersion"], 1);
}

fn load(process: &mut ProcessHarness, id: u64, session_id: &str) {
    process.send(json!({
        "jsonrpc":"2.0","id":id,"method":"session/load",
        "params":{"sessionId":session_id,"cwd":"/workspace","mcpServers":[]}
    }));
    assert!(process.response(id).get("error").is_none());
}

fn resume(process: &mut ProcessHarness, id: u64, session_id: &str) {
    process.send(json!({
        "jsonrpc":"2.0","id":id,"method":"session/resume",
        "params":{"sessionId":session_id,"cwd":"/workspace","mcpServers":[]}
    }));
    assert!(process.response(id).get("error").is_none());
}

fn snapshot<'a>(roots: impl IntoIterator<Item = &'a Path>) -> BTreeMap<PathBuf, FileState> {
    let mut result = BTreeMap::new();
    for (root_index, root) in roots.into_iter().enumerate() {
        for entry in fs::read_dir(root).unwrap() {
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            assert!(metadata.is_file());
            let bytes = fs::read(entry.path()).unwrap();
            let digest: [u8; 32] = Sha256::digest(&bytes).into();
            result.insert(
                PathBuf::from(root_index.to_string())
                    .join(entry.path().strip_prefix(root).unwrap()),
                FileState {
                    sha256: digest,
                    bytes: metadata.len(),
                    modified: metadata.modified().ok(),
                },
            );
        }
    }
    result
}

fn vesper_record(session_id: &str, visible: &str) -> VesperSessionV1 {
    let provider = ProviderId::new("zai").unwrap();
    let session_id = SessionId::new(session_id).unwrap();
    VesperSessionV1 {
        format: BoundedString::new(VesperSessionV1::format_name()).unwrap(),
        version: VesperSessionV1::current_version(),
        session_id: session_id.clone(),
        title: Some(BoundedString::new("Agent Vesper collision").unwrap()),
        updated_at: Some(BoundedString::new("2026-07-30T11:00:00Z").unwrap()),
        lineage: SessionLineage {
            root_session_id: session_id,
            parent_session_id: None,
        },
        workspace_roots: vec![WorkspaceRoot {
            name: BoundedString::new("workspace").unwrap(),
            path: BoundedString::new("/workspace").unwrap(),
            primary: true,
        }],
        provider_id: provider.clone(),
        model: QualifiedModelId {
            provider_id: provider.clone(),
            model_id: ModelId::new("glm-5.2").unwrap(),
        },
        endpoint_id: EndpointId::new("zai-coding").unwrap(),
        provider_configuration: PersistedProviderConfiguration {
            provider_id: provider,
            values: envelope("provider.zai"),
        },
        operating_mode: SessionOperatingMode::Code,
        permission_mode: SessionPermissionMode::Ask,
        history: vec![vesper_domain::ConversationMessage {
            id: MessageId::new("agent-vesper-message").unwrap(),
            role: MessageRole::Assistant,
            content: vec![ContentPart::Text(ContentText::new(visible).unwrap())],
            extensions: ExtensionMap::default(),
        }],
        cumulative_usage: NormalizedUsage::unavailable(UsageMode::Cumulative),
        revision: Revision::new(0),
        plan: Vec::new(),
        extensions: envelope("compat.agent-vesper"),
    }
}

fn envelope(namespace: &str) -> VersionedExtensionEnvelope {
    VersionedExtensionEnvelope {
        namespace: ExtensionNamespace::new(namespace).unwrap(),
        version: SchemaVersion::new(1).unwrap(),
        values: ExtensionMap::default(),
    }
}

fn write_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}
