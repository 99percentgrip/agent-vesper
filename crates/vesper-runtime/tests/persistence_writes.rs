//! Stage 6 runtime write integration: snapshot -> VesperSessionV1 ->
//! transactional writer, and supervisor `save_session` wiring.

use std::{path::PathBuf, sync::Arc};

use vesper_domain::{
    BoundedString, EndpointId, ExtensionMap, ExtensionNamespace, ModelId, NormalizedUsage,
    ProviderId, QualifiedModelId, Revision, SchemaVersion, SessionId, SessionLineage,
    SessionOperatingMode, SessionPermissionMode, UsageMode, VersionedExtensionEnvelope,
    WorkspaceRoot,
};
use vesper_provider::ProviderConfiguration;
use vesper_runtime::{RuntimeError, RuntimeSessionWrites, RuntimeSupervisor, SessionSnapshot};
use vesper_sessions::{
    CompatibilityAvailability, FilesystemSessionStore, MetadataOrigin, SessionLister,
    SessionSource, SessionWriter, VesperDecodeBounds, VesperLoadOutcome, VesperSessionWriter,
    WriteBounds,
};

fn saveable_snapshot(id: &str) -> SessionSnapshot {
    let provider = ProviderId::new("zai").unwrap();
    SessionSnapshot {
        session_id: SessionId::new(id).unwrap(),
        lineage: SessionLineage {
            root_session_id: SessionId::new(id).unwrap(),
            parent_session_id: None,
        },
        workspace_roots: vec![WorkspaceRoot {
            name: BoundedString::new("workspace").unwrap(),
            path: BoundedString::new("/fixture").unwrap(),
            primary: true,
        }],
        provider_id: provider.clone(),
        model: QualifiedModelId {
            provider_id: provider.clone(),
            model_id: ModelId::new("glm-5.2").unwrap(),
        },
        endpoint_id: Some(EndpointId::new("zai-coding").unwrap()),
        provider_configuration: ProviderConfiguration {
            provider_id: provider,
            values: VersionedExtensionEnvelope {
                namespace: ExtensionNamespace::new("provider.zai").unwrap(),
                version: SchemaVersion::new(1).unwrap(),
                values: ExtensionMap::default(),
            },
        },
        source: SessionSource::AgentVesper,
        configuration_status: vesper_sessions::SessionConfigurationStatus::Ready,
        operating_mode: SessionOperatingMode::Code,
        permission_mode: SessionPermissionMode::Ask,
        history: Vec::new(),
        cumulative_usage: NormalizedUsage::unavailable(UsageMode::Cumulative),
        revision: Revision::new(5),
        active_turn: None,
        closed: false,
        replay: None,
        compatibility: None,
        reasoning: None,
    }
}

fn temp_root(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "agent-vesper-runtime-writer-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn create_command(id: &str, number: u64) -> vesper_domain::HarnessCommand {
    vesper_domain::HarnessCommand {
        schema_version: vesper_domain::CommandSchemaVersion::CURRENT,
        command_id: vesper_domain::CommandId::new(format!("command-{number}")).unwrap(),
        correlation_id: vesper_domain::CorrelationId::new(format!("correlation-{number}")).unwrap(),
        initiator: vesper_domain::CommandInitiator::Acp,
        expected_revision: None,
        payload: vesper_domain::HarnessCommandPayload::CreateSession {
            workspace_roots: Vec::new(),
            requested_session_id: Some(SessionId::new(id).unwrap()),
        },
    }
}

#[tokio::test]
async fn store_snapshot_writes_a_round_trippable_record() {
    let root = temp_root("store-snapshot");
    let sessions = root.join("sessions");
    let writer: Arc<dyn SessionWriter> = Arc::new(
        VesperSessionWriter::new(
            sessions.clone(),
            SessionSource::AgentVesper,
            WriteBounds::default(),
        )
        .unwrap(),
    );
    let writes = RuntimeSessionWrites::new(writer);

    let outcome = writes
        .store_snapshot(&saveable_snapshot("runtime-save"))
        .await
        .unwrap();
    assert!(outcome.session_bytes > 0);
    assert!(outcome.sidecar_bytes > 0);
    assert!(sessions.join("runtime-save.json").is_file());
    assert!(sessions.join("runtime-save.meta").is_file());

    // The written record must decode back to the persisted state, proving the
    // snapshot -> record converter is loss-free for structural fields.
    let reader = FilesystemSessionStore::new(
        sessions,
        SessionSource::AgentVesper,
        vesper_sessions::DiscoveryBounds::default(),
    )
    .unwrap();
    let provider = ProviderId::new("zai").unwrap();
    let model = QualifiedModelId {
        provider_id: provider.clone(),
        model_id: ModelId::new("glm-5.2").unwrap(),
    };
    let decoder = vesper_sessions::VesperSessionDecoder::new(
        VesperDecodeBounds::default(),
        CompatibilityAvailability::default()
            .with_provider(provider.clone())
            .with_model(model)
            .with_endpoint(provider, EndpointId::new("zai-coding").unwrap()),
    );
    let VesperLoadOutcome::Loaded(state) = decoder
        .load(&reader, &SessionId::new("runtime-save").unwrap())
        .await
    else {
        panic!("written record did not decode")
    };
    assert_eq!(state.session_id.as_str(), "runtime-save");
    assert_eq!(state.revision.get(), 5);
    assert_eq!(state.endpoint_id.as_str(), "zai-coding");

    // The sidecar powers listing without parsing the record body.
    let listed = reader.list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].origin, MetadataOrigin::Sidecar);
    assert_eq!(listed[0].session_id.as_str(), "runtime-save");

    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn save_session_without_a_writer_is_a_noop() {
    let provider_id = ProviderId::new("test.fake").unwrap();
    let providers = Arc::new(vesper_runtime::ProviderRegistry::new());
    providers
        .register(vesper_testkit_helpers::fake_factory(provider_id.clone()))
        .await
        .unwrap();
    let runtime = RuntimeSupervisor::new(
        providers,
        vesper_runtime::RuntimeDefaults {
            provider_configuration: ProviderConfiguration {
                provider_id: provider_id.clone(),
                values: VersionedExtensionEnvelope {
                    namespace: ExtensionNamespace::new("provider.test").unwrap(),
                    version: SchemaVersion::new(1).unwrap(),
                    values: ExtensionMap::default(),
                },
            },
            model: QualifiedModelId {
                provider_id,
                model_id: ModelId::new("fixture-model").unwrap(),
            },
            endpoint: EndpointId::new("test-endpoint").unwrap(),
            system_instructions: Vec::new(),
            reasoning: None,
            sampling: None,
            maximum_output_tokens: None,
        },
    );
    let id = SessionId::new("ephemeral").unwrap();
    runtime
        .execute(create_command("ephemeral", 1))
        .await
        .unwrap();
    // No writer injected: save is an optional no-op.
    assert_eq!(runtime.save_session(&id).await.unwrap(), None);
}

#[tokio::test]
async fn store_snapshot_without_endpoint_surfaces_sanitized_failure() {
    let root = temp_root("save-failure");
    let writer: Arc<dyn SessionWriter> = Arc::new(
        VesperSessionWriter::new(
            root.join("sessions"),
            SessionSource::AgentVesper,
            WriteBounds::default(),
        )
        .unwrap(),
    );
    let writes = RuntimeSessionWrites::new(writer);

    // Session creation now always assigns a default endpoint, so an endpointless
    // snapshot is a defensive invariant. The write port still guards against it
    // and reports a bounded, sanitized failure before touching the filesystem.
    let mut snapshot = saveable_snapshot("defensive-no-endpoint");
    snapshot.endpoint_id = None;

    assert_eq!(
        writes.store_snapshot(&snapshot).await,
        Err(RuntimeError::PersistentSessionWriteFailed)
    );
    let _ = std::fs::remove_dir_all(&root);
}

// A tiny local module so the test can construct a fake provider registry
// without pulling the full scripted provider wiring from runtime.rs.
mod vesper_testkit_helpers {
    use std::sync::Arc;
    use vesper_domain::ProviderId;
    use vesper_provider::{
        CancellationSignal, ProviderConfiguration, ProviderError, ProviderFactory, ProviderFuture,
    };
    use vesper_testkit::FakeProviderSession;

    pub(super) fn fake_factory(id: ProviderId) -> impl ProviderFactory {
        struct F {
            id: ProviderId,
            session: FakeProviderSession,
        }
        impl ProviderFactory for F {
            type Session = FakeProviderSession;
            fn provider_id(&self) -> &ProviderId {
                &self.id
            }
            fn create_session<'a>(
                &'a self,
                _config: &'a ProviderConfiguration,
                _cancellation: Arc<dyn CancellationSignal>,
            ) -> ProviderFuture<'a, Result<Self::Session, ProviderError>> {
                let session = self.session.clone();
                Box::pin(async move { Ok(session) })
            }
        }
        F {
            id,
            session: FakeProviderSession::default(),
        }
    }
}
