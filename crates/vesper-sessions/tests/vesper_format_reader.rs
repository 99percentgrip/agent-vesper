use std::fs;

use vesper_domain::{
    BoundedString, EndpointId, ExtensionMap, ExtensionNamespace, ModelId, NormalizedUsage,
    ProviderId, QualifiedModelId, Revision, SchemaVersion, SessionId, SessionLineage,
    SessionOperatingMode, SessionPermissionMode, UsageMode, VersionedExtensionEnvelope,
    WorkspaceRoot,
};
use vesper_sessions::{
    CompatibilityAvailability, DiscoveryBounds, FilesystemSessionStore,
    PersistedProviderConfiguration, SessionLister, SessionSource, VesperDecodeBounds,
    VesperLoadOutcome, VesperSessionDecoder, VesperSessionV1,
};

fn envelope(namespace: &str) -> VersionedExtensionEnvelope {
    VersionedExtensionEnvelope {
        namespace: ExtensionNamespace::new(namespace).unwrap(),
        version: SchemaVersion::new(1).unwrap(),
        values: ExtensionMap::default(),
    }
}

#[tokio::test]
async fn fixture_file_is_read_through_the_bounded_read_only_store() {
    let provider = ProviderId::new("zai").unwrap();
    let model = QualifiedModelId {
        provider_id: provider.clone(),
        model_id: ModelId::new("glm-5.2").unwrap(),
    };
    let record = VesperSessionV1 {
        format: BoundedString::new(VesperSessionV1::format_name()).unwrap(),
        version: VesperSessionV1::current_version(),
        session_id: SessionId::new("vesper-read-only-fixture").unwrap(),
        title: Some(BoundedString::new("Fixture").unwrap()),
        updated_at: None,
        lineage: SessionLineage {
            root_session_id: SessionId::new("vesper-read-only-fixture").unwrap(),
            parent_session_id: None,
        },
        workspace_roots: vec![WorkspaceRoot {
            name: BoundedString::new("workspace").unwrap(),
            path: BoundedString::new("/fixture").unwrap(),
            primary: true,
        }],
        provider_id: provider.clone(),
        model: model.clone(),
        endpoint_id: EndpointId::new("zai-coding").unwrap(),
        provider_configuration: PersistedProviderConfiguration {
            provider_id: provider.clone(),
            values: envelope("provider.zai"),
        },
        operating_mode: SessionOperatingMode::Code,
        permission_mode: SessionPermissionMode::Ask,
        history: Vec::new(),
        cumulative_usage: NormalizedUsage::unavailable(UsageMode::Cumulative),
        revision: Revision::new(0),
        plan: Vec::new(),
        extensions: envelope("compat.agent-vesper"),
    };
    let root = std::env::temp_dir().join(format!("vesper-format-reader-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("vesper-read-only-fixture.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    let store = FilesystemSessionStore::new(
        root.clone(),
        SessionSource::AgentVesper,
        DiscoveryBounds::default(),
    )
    .unwrap();
    let decoder = VesperSessionDecoder::new(
        VesperDecodeBounds::default(),
        CompatibilityAvailability::default()
            .with_provider(provider.clone())
            .with_model(model)
            .with_endpoint(provider, EndpointId::new("zai-coding").unwrap()),
    );
    assert!(matches!(
        decoder
            .load(&store, &SessionId::new("vesper-read-only-fixture").unwrap())
            .await,
        VesperLoadOutcome::Loaded(_)
    ));
    let listed = store.list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].cwd, "/fixture");
    assert_eq!(listed[0].provider.as_deref(), Some("zai"));
    assert_eq!(listed[0].model.as_deref(), Some("glm-5.2"));
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    fs::remove_dir_all(root).unwrap();
}
