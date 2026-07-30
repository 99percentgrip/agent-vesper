use std::sync::{Arc, Mutex};

use serde_json::json;
use vesper_domain::{
    ContentPart, EndpointId, MessageRole, ModelId, ProviderId, QualifiedModelId, SessionId,
};
use vesper_sessions::{
    CompatibilityAvailability, ConfigurationIssue, DecodedLegacySession, LegacyLoadOutcome,
    LegacyRuntimeConverter, LegacySessionDecoder, MetadataOrigin, ReplayError, ReplayFuture,
    ReplaySink, ReplayUpdate, SessionConfigurationStatus, SessionMetadata, SessionSource,
};

fn metadata(id: &str) -> SessionMetadata {
    SessionMetadata {
        session_id: SessionId::new(id).unwrap(),
        source: SessionSource::LegacyNativeGlm { profile: None },
        byte_len: 0,
        modified: None,
        record_path: None,
        metadata_path: None,
        origin: MetadataOrigin::JsonFallback,
        title: Some("Compatibility session".into()),
        cwd: "/workspace".into(),
        updated_at: Some("2026-07-30T00:00:00Z".into()),
        model: Some("glm-5.2".into()),
        provider: Some("zai".into()),
        parent_session_id: None,
        branch_root_id: None,
        safe_preview: None,
        read_only: true,
    }
}

fn availability() -> CompatibilityAvailability {
    let provider = ProviderId::new("zai").unwrap();
    CompatibilityAvailability::default()
        .with_provider(provider.clone())
        .with_model(QualifiedModelId {
            provider_id: provider.clone(),
            model_id: ModelId::new("glm-5.2").unwrap(),
        })
        .with_endpoint(provider, EndpointId::new("zai-coding").unwrap())
}

fn decoded(id: &str) -> DecodedLegacySession {
    let value = json!({
        "cwd": "/workspace",
        "additional_directories": ["/workspace/vendor"],
        "model": "glm-5.2",
        "api_endpoint": "coding",
        "thought_level": "high",
        "generation_profile": "balanced",
        "parent_session_id": "parent",
        "branch_root_id": "root",
        "permission_mode": "read-only",
        "mode": "plan",
        "total_input_tokens": 8,
        "total_output_tokens": 5,
        "total_cached_tokens": 3,
        "plan": [{
            "content": "Inspect safely",
            "status": "in_progress",
            "priority": "high"
        }],
        "messages": [
            {"role": "system", "content": "never replay or actively reuse"},
            {"role": "user", "content": [
                {"type": "text", "text": "inspect"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AA=="}}
            ]},
            {"role": "assistant", "content": "checking", "reasoning_content": "opaque-private",
             "tool_calls": [{"id": "call-1", "function": {"name": "read_file", "arguments": "{\"path\":\"x\"}"}}]},
            {"role": "tool", "tool_call_id": "call-1", "content": "tool internals"},
            {"role": "assistant", "content": ""},
            {"role": "tool", "tool_call_id": "orphan", "content": "drop orphan"}
        ],
        "future_field": {"preserve": true}
    });
    let bytes = serde_json::to_vec(&value).unwrap();
    let LegacyLoadOutcome::Loaded(decoded) =
        LegacySessionDecoder::default().decode_record(metadata(id), &bytes)
    else {
        panic!("fixture did not decode")
    };
    *decoded
}

#[test]
fn conversion_preserves_runtime_state_but_replay_exposes_only_visible_history() {
    let state = LegacyRuntimeConverter::new(availability())
        .convert(decoded("legacy-session"))
        .unwrap();

    assert_eq!(state.session_id.as_str(), "legacy-session");
    assert_eq!(
        state.source,
        SessionSource::LegacyNativeGlm { profile: None }
    );
    assert_eq!(state.lineage.parent_session_id.unwrap().as_str(), "parent");
    assert_eq!(state.lineage.root_session_id.as_str(), "root");
    assert_eq!(state.workspace_roots.len(), 2);
    assert_eq!(state.provider_id.as_str(), "zai");
    assert_eq!(state.model.model_id.as_str(), "glm-5.2");
    assert_eq!(state.endpoint_id.as_str(), "zai-coding");
    assert_eq!(
        state.configuration_status,
        SessionConfigurationStatus::Ready
    );
    assert_eq!(state.cumulative_usage.input.value, Some(8));
    assert_eq!(state.cumulative_usage.output.value, Some(5));
    assert_eq!(state.cumulative_usage.total.value, Some(13));

    assert_eq!(state.history.len(), 3);
    assert_eq!(state.history[0].role, MessageRole::User);
    assert!(
        state.history[0]
            .content
            .iter()
            .any(|part| matches!(part, ContentPart::Image(_)))
    );
    assert!(
        state.history[1]
            .content
            .iter()
            .any(|part| matches!(part, ContentPart::ToolCall(_)))
    );
    assert!(
        state.history[1]
            .content
            .iter()
            .any(|part| matches!(part, ContentPart::Reasoning(_)))
    );
    assert!(
        state.history[2]
            .content
            .iter()
            .any(|part| matches!(part, ContentPart::ToolResult(_)))
    );

    let replay_messages: Vec<_> = state
        .replay
        .updates()
        .filter_map(|update| match update {
            ReplayUpdate::VisibleMessage(message) => Some(message),
            _ => None,
        })
        .collect();
    assert_eq!(replay_messages.len(), 2);
    assert_eq!(replay_messages[0].text.as_str(), "inspect");
    assert_eq!(replay_messages[1].text.as_str(), "checking");
    let rendered = format!("{:?}", state.compatibility);
    assert!(!rendered.contains("opaque-private"));
    let vesper_sessions::SessionCompatibilityData::Legacy(compatibility) = &state.compatibility
    else {
        panic!("expected legacy compatibility data")
    };
    assert_eq!(
        compatibility.expose_for_compatibility().unknown_fields["future_field"],
        json!({"preserve": true})
    );
}

#[test]
fn generated_message_identities_are_stable_role_distinct_and_content_independent() {
    let first = LegacyRuntimeConverter::new(availability())
        .convert(decoded("stable-session"))
        .unwrap();
    let second = LegacyRuntimeConverter::new(availability())
        .convert(decoded("stable-session"))
        .unwrap();
    let first_ids: Vec<_> = first
        .history
        .iter()
        .map(|message| message.id.clone())
        .collect();
    let second_ids: Vec<_> = second
        .history
        .iter()
        .map(|message| message.id.clone())
        .collect();
    assert_eq!(first_ids, second_ids);
    assert_ne!(first_ids[0], first_ids[1]);
    assert!(first_ids.iter().all(|id| !id.as_str().contains("inspect")));
    assert!(first_ids.iter().all(|id| id.as_str().len() < 256));
}

#[test]
fn unavailable_provider_configuration_remains_inspectable_and_replayable() {
    let mut value = decoded("unknown-provider");
    value
        .session
        .unknown_fields
        .insert("provider".into(), json!("future-provider"));
    value.session.model = "future-model".into();
    value.session.api_endpoint = "future-endpoint".into();
    let state = LegacyRuntimeConverter::new(CompatibilityAvailability::default())
        .convert(value)
        .unwrap();
    assert_eq!(state.provider_id.as_str(), "future-provider");
    assert_eq!(state.model.model_id.as_str(), "future-model");
    assert_eq!(state.endpoint_id.as_str(), "future-endpoint");
    let SessionConfigurationStatus::ConfigurationRequired(issues) = &state.configuration_status
    else {
        panic!("unknown provider state was incorrectly dispatchable")
    };
    assert!(
        issues
            .iter()
            .any(|issue| matches!(issue, ConfigurationIssue::UnknownProvider(_)))
    );
    assert!(state.replay.updates().len() >= 4);
}

#[derive(Clone)]
struct RecordingSink {
    accepted: Arc<Mutex<Vec<&'static str>>>,
}

impl ReplaySink for RecordingSink {
    fn accept<'a>(&'a mut self, update: &'a ReplayUpdate) -> ReplayFuture<'a> {
        let accepted = Arc::clone(&self.accepted);
        Box::pin(async move {
            let label = match update {
                ReplayUpdate::VisibleMessage(_) => "message",
                ReplayUpdate::Plan(_) => "plan",
                ReplayUpdate::Metadata(_) => "metadata",
                ReplayUpdate::AvailableCommands(_) => "commands",
            };
            accepted.lock().unwrap().push(label);
            Ok::<(), ReplayError>(())
        })
    }
}

#[tokio::test]
async fn replay_delivery_is_ordered_and_completion_waits_for_all_acceptances() {
    let state = LegacyRuntimeConverter::new(availability())
        .convert(decoded("replay-order"))
        .unwrap();
    let accepted = Arc::new(Mutex::new(Vec::new()));
    let mut sink = RecordingSink {
        accepted: Arc::clone(&accepted),
    };
    state.replay.deliver(&mut sink).await.unwrap();
    // A lifecycle response is permitted only after `deliver` has returned.
    accepted.lock().unwrap().push("completion");
    assert_eq!(
        *accepted.lock().unwrap(),
        [
            "message",
            "message",
            "plan",
            "metadata",
            "commands",
            "completion"
        ]
    );
}
