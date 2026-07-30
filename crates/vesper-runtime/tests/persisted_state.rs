use serde_json::json;
use vesper_domain::{EndpointId, ModelId, ProviderId, QualifiedModelId, SessionId};
use vesper_runtime::SessionSnapshot;
use vesper_sessions::{
    CompatibilityAvailability, LegacyLoadOutcome, LegacyRuntimeConverter, LegacySessionDecoder,
    MetadataOrigin, SessionConfigurationStatus, SessionMetadata, SessionSource,
};

fn metadata() -> SessionMetadata {
    SessionMetadata {
        session_id: SessionId::new("legacy-runtime").unwrap(),
        source: SessionSource::LegacyNativeGlm { profile: None },
        byte_len: 0,
        modified: None,
        record_path: None,
        metadata_path: None,
        origin: MetadataOrigin::JsonFallback,
        title: None,
        cwd: "/workspace".into(),
        updated_at: None,
        model: None,
        provider: None,
        parent_session_id: None,
        branch_root_id: None,
        safe_preview: None,
        read_only: true,
    }
}

#[test]
fn converted_compatibility_state_populates_runtime_snapshot_without_io() {
    let provider = ProviderId::new("zai").unwrap();
    let availability = CompatibilityAvailability::default()
        .with_provider(provider.clone())
        .with_model(QualifiedModelId {
            provider_id: provider.clone(),
            model_id: ModelId::new("glm-5.2").unwrap(),
        })
        .with_endpoint(provider, EndpointId::new("zai-coding").unwrap());
    let bytes = serde_json::to_vec(&json!({
        "cwd": "/workspace",
        "model": "glm-5.2",
        "api_endpoint": "coding",
        "messages": [{"role": "user", "content": "restored"}]
    }))
    .unwrap();
    let LegacyLoadOutcome::Loaded(decoded) =
        LegacySessionDecoder::default().decode_record(metadata(), &bytes)
    else {
        panic!("record did not decode")
    };
    let converted = LegacyRuntimeConverter::new(availability)
        .convert(*decoded)
        .unwrap();
    let snapshot = SessionSnapshot::from_persisted(converted);
    assert_eq!(snapshot.session_id.as_str(), "legacy-runtime");
    assert_eq!(
        snapshot.source,
        SessionSource::LegacyNativeGlm { profile: None }
    );
    assert_eq!(
        snapshot.configuration_status,
        SessionConfigurationStatus::Ready
    );
    assert_eq!(snapshot.history.len(), 1);
    assert!(snapshot.replay.is_some());
    assert!(snapshot.compatibility.is_some());
}
