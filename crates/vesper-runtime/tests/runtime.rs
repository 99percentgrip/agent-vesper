use std::{fs, sync::Arc};

use vesper_domain::{
    BoundedString, CommandId, CommandInitiator, CommandSchemaVersion, ContentPart, ContentText,
    ConversationMessage, CorrelationId, EndpointId, EventSequence, ExtensionMap, FinishOutcome,
    HarnessCommand, HarnessCommandPayload, MessageId, MessageRole, ModelId, PromptSubmission,
    ProviderId, QualifiedModelId, SchemaVersion, SessionId, SessionListFilter,
    VersionedExtensionEnvelope, WorkspaceRoot,
};
use vesper_provider::{
    CancellationSignal, ProviderConfiguration, ProviderFactory, ProviderFuture, ProviderStreamEvent,
};
use vesper_runtime::{
    ProviderRegistry, RuntimeDefaults, RuntimeResponse, RuntimeSessionReads, RuntimeSupervisor,
};
use vesper_sessions::{
    BoxSessionFuture, CompatibilityAvailability, CompositeSessionRepository, DiscoveryBounds,
    EmptySessionRepository, FilesystemSessionStore, LegacyDecodeBounds,
    SessionListFilter as PersistentSessionListFilter, SessionLister, SessionMetadata,
    SessionReadIntent, SessionReader, SessionRecord, SessionRepository, SessionSource,
    SessionStoreError, VesperDecodeBounds,
};
use vesper_testkit::FakeProviderSession;

#[derive(Clone)]
struct FakeFactory {
    id: ProviderId,
    session: FakeProviderSession,
}

impl ProviderFactory for FakeFactory {
    type Session = FakeProviderSession;

    fn provider_id(&self) -> &ProviderId {
        &self.id
    }

    fn create_session<'a>(
        &'a self,
        _config: &'a ProviderConfiguration,
        _cancellation: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<Self::Session, vesper_provider::ProviderError>> {
        let session = self.session.clone();
        Box::pin(async move { Ok(session) })
    }
}

fn configuration(provider_id: &ProviderId) -> ProviderConfiguration {
    ProviderConfiguration {
        provider_id: provider_id.clone(),
        values: VersionedExtensionEnvelope {
            namespace: vesper_domain::ExtensionNamespace::new("provider.test").unwrap(),
            version: SchemaVersion::new(1).unwrap(),
            values: ExtensionMap::default(),
        },
    }
}

async fn runtime(
    scripts: impl IntoIterator<Item = vesper_testkit::ScriptedProviderResponse>,
) -> Arc<RuntimeSupervisor> {
    let provider_id = ProviderId::new("test.fake").unwrap();
    let providers = Arc::new(ProviderRegistry::new());
    providers
        .register(FakeFactory {
            id: provider_id.clone(),
            session: FakeProviderSession::with_scripts(scripts),
        })
        .await
        .unwrap();
    Arc::new(RuntimeSupervisor::new(
        providers,
        RuntimeDefaults {
            provider_configuration: configuration(&provider_id),
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
    ))
}

async fn runtime_with_reads(root: std::path::PathBuf) -> Arc<RuntimeSupervisor> {
    let legacy: Arc<dyn SessionRepository> = Arc::new(
        FilesystemSessionStore::new(
            root,
            SessionSource::LegacyNativeGlm { profile: None },
            DiscoveryBounds::default(),
        )
        .unwrap(),
    );
    runtime_with_repository(legacy).await
}

async fn runtime_with_repository(legacy: Arc<dyn SessionRepository>) -> Arc<RuntimeSupervisor> {
    let provider_id = ProviderId::new("test.fake").unwrap();
    let model = QualifiedModelId {
        provider_id: provider_id.clone(),
        model_id: ModelId::new("fixture-model").unwrap(),
    };
    let providers = Arc::new(ProviderRegistry::new());
    providers
        .register(FakeFactory {
            id: provider_id.clone(),
            session: FakeProviderSession::default(),
        })
        .await
        .unwrap();
    let memory: Arc<dyn SessionRepository> =
        Arc::new(EmptySessionRepository::new(SessionSource::InMemory).unwrap());
    let agent: Arc<dyn SessionRepository> =
        Arc::new(EmptySessionRepository::new(SessionSource::AgentVesper).unwrap());
    let repository = Arc::new(CompositeSessionRepository::new(memory, agent, legacy).unwrap());
    let availability = CompatibilityAvailability::default()
        .with_provider(provider_id.clone())
        .with_model(model.clone())
        .with_endpoint(
            provider_id.clone(),
            vesper_domain::EndpointId::new("zai-coding").unwrap(),
        );
    Arc::new(
        RuntimeSupervisor::new(
            providers,
            RuntimeDefaults {
                provider_configuration: configuration(&provider_id),
                model,
                endpoint: EndpointId::new("test-endpoint").unwrap(),
                system_instructions: Vec::new(),
                reasoning: None,
                sampling: None,
                maximum_output_tokens: None,
            },
        )
        .with_session_reads(Arc::new(RuntimeSessionReads::new(
            repository,
            availability,
            LegacyDecodeBounds::default(),
            VesperDecodeBounds::default(),
        ))),
    )
}

#[derive(Clone)]
struct DelayedRepository {
    inner: FilesystemSessionStore,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

impl SessionReader for DelayedRepository {
    fn source(&self) -> SessionSource {
        self.inner.source()
    }

    fn read<'a>(
        &'a self,
        session_id: &'a SessionId,
        intent: SessionReadIntent,
    ) -> BoxSessionFuture<'a, Result<Option<SessionRecord>, SessionStoreError>> {
        Box::pin(async move {
            self.entered.notify_one();
            self.release.notified().await;
            self.inner.read(session_id, intent).await
        })
    }
}

impl SessionLister for DelayedRepository {
    fn list_filtered(
        &self,
        filter: PersistentSessionListFilter,
    ) -> BoxSessionFuture<'_, Result<Vec<SessionMetadata>, SessionStoreError>> {
        self.inner.list_filtered(filter)
    }
}

fn command(payload: HarnessCommandPayload, number: u64) -> HarnessCommand {
    HarnessCommand {
        schema_version: CommandSchemaVersion::CURRENT,
        command_id: CommandId::new(format!("command-{number}")).unwrap(),
        correlation_id: CorrelationId::new(format!("correlation-{number}")).unwrap(),
        initiator: CommandInitiator::Acp,
        expected_revision: None,
        payload,
    }
}

fn create(number: u64) -> HarnessCommand {
    command(
        HarnessCommandPayload::CreateSession {
            workspace_roots: vec![WorkspaceRoot {
                name: BoundedString::new("workspace").unwrap(),
                path: BoundedString::new("/fixture").unwrap(),
                primary: true,
            }],
            requested_session_id: Some(SessionId::new(format!("session-{number}")).unwrap()),
        },
        number,
    )
}

#[tokio::test]
async fn validated_compaction_history_replaces_runtime_history_atomically() {
    let runtime = runtime([]).await;
    let RuntimeResponse::Session(created) = runtime.execute(create(1)).await.unwrap() else {
        panic!("expected created session");
    };
    let before = runtime.snapshot(&created.session_id).await.unwrap();
    let replacement = vec![ConversationMessage {
        id: MessageId::new("compaction-covered-4").unwrap(),
        role: MessageRole::User,
        content: vec![ContentPart::Text(
            ContentText::new("<agent-vesper-context-summary>state</agent-vesper-context-summary>")
                .unwrap(),
        )],
        extensions: ExtensionMap::default(),
    }];

    runtime
        .replace_history(&created.session_id, replacement.clone())
        .await
        .unwrap();

    let after = runtime.snapshot(&created.session_id).await.unwrap();
    assert_eq!(after.history, replacement);
    assert!(after.revision.get() > before.revision.get());
}

#[tokio::test]
async fn ephemeral_lifecycle_and_lineage_are_actor_owned() {
    let runtime = runtime([]).await;
    let RuntimeResponse::Session(parent) = runtime.execute(create(1)).await.unwrap() else {
        panic!("expected session");
    };
    let RuntimeResponse::Session(child) = runtime
        .execute(command(
            HarnessCommandPayload::ForkSession {
                session_id: parent.session_id.clone(),
                requested_session_id: Some(SessionId::new("session-child").unwrap()),
            },
            2,
        ))
        .await
        .unwrap()
    else {
        panic!("expected child");
    };
    assert_eq!(
        child.lineage.parent_session_id,
        Some(parent.session_id.clone())
    );
    assert_eq!(child.lineage.root_session_id, parent.session_id);

    let RuntimeResponse::Sessions(list) = runtime
        .execute(command(
            HarnessCommandPayload::ListSessions(SessionListFilter::default()),
            3,
        ))
        .await
        .unwrap()
    else {
        panic!("expected list");
    };
    assert_eq!(list.len(), 2);
}

#[tokio::test]
async fn persistent_read_path_lists_loads_caches_forks_and_closes_without_writing() {
    let root = std::env::temp_dir().join(format!(
        "vesper-runtime-read-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let record_path = root.join("persisted-session.json");
    fs::write(
        &record_path,
        serde_json::to_vec(&serde_json::json!({
            "cwd": "/fixture",
            "model": "fixture-model",
            "api_endpoint": "coding",
            "messages": [
                {"role": "user", "content": "remember this"},
                {"role": "assistant", "content": "restored"}
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let original = fs::read(&record_path).unwrap();
    let runtime = runtime_with_reads(root.clone()).await;

    let RuntimeResponse::Sessions(list) = runtime
        .execute(command(
            HarnessCommandPayload::ListSessions(SessionListFilter::default()),
            80,
        ))
        .await
        .unwrap()
    else {
        panic!("expected list")
    };
    assert_eq!(list.len(), 1);

    let roots = vec![WorkspaceRoot {
        name: BoundedString::new("workspace").unwrap(),
        path: BoundedString::new("/fixture").unwrap(),
        primary: true,
    }];
    let RuntimeResponse::Session(loaded) = runtime
        .execute(command(
            HarnessCommandPayload::LoadSession {
                session_id: SessionId::new("persisted-session").unwrap(),
                workspace_roots: roots.clone(),
            },
            81,
        ))
        .await
        .unwrap()
    else {
        panic!("expected loaded session")
    };
    assert_eq!(loaded.history.len(), 2);
    assert!(loaded.replay.is_some());

    fs::write(&record_path, b"{corrupt").unwrap();
    let RuntimeResponse::Session(resumed) = runtime
        .execute(command(
            HarnessCommandPayload::ResumeSession {
                session_id: loaded.session_id.clone(),
                workspace_roots: roots,
            },
            82,
        ))
        .await
        .unwrap()
    else {
        panic!("expected cached session")
    };
    assert_eq!(resumed.history.len(), 2);

    let RuntimeResponse::Session(child) = runtime
        .execute(command(
            HarnessCommandPayload::ForkSession {
                session_id: loaded.session_id.clone(),
                requested_session_id: Some(SessionId::new("persisted-child").unwrap()),
            },
            83,
        ))
        .await
        .unwrap()
    else {
        panic!("expected child")
    };
    assert_eq!(
        child.lineage.parent_session_id,
        Some(loaded.session_id.clone())
    );

    runtime
        .execute(command(
            HarnessCommandPayload::CloseSession {
                session_id: loaded.session_id,
            },
            84,
        ))
        .await
        .unwrap();
    assert_eq!(fs::read(&record_path).unwrap(), b"{corrupt");
    fs::write(&record_path, original).unwrap();
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn missing_persistent_id_creates_only_an_ephemeral_session() {
    let root = std::env::temp_dir().join(format!("vesper-runtime-missing-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let runtime = runtime_with_reads(root.clone()).await;
    let RuntimeResponse::Session(snapshot) = runtime
        .execute(command(
            HarnessCommandPayload::LoadSession {
                session_id: SessionId::new("missing-session").unwrap(),
                workspace_roots: vec![],
            },
            90,
        ))
        .await
        .unwrap()
    else {
        panic!("expected ephemeral fallback")
    };
    assert_eq!(snapshot.source, SessionSource::InMemory);
    assert!(!root.exists());
}

#[tokio::test]
async fn concurrent_same_id_loads_adopt_one_actor_while_distinct_ids_and_listing_progress() {
    let root =
        std::env::temp_dir().join(format!("vesper-runtime-concurrent-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    for id in ["same", "other"] {
        fs::write(
            root.join(format!("{id}.json")),
            serde_json::to_vec(&serde_json::json!({
                "cwd": "/fixture",
                "model": "fixture-model",
                "api_endpoint": "coding",
                "messages": [{"role": "user", "content": id}]
            }))
            .unwrap(),
        )
        .unwrap();
    }
    let runtime = runtime_with_reads(root.clone()).await;
    let roots = vec![WorkspaceRoot {
        name: BoundedString::new("workspace").unwrap(),
        path: BoundedString::new("/fixture").unwrap(),
        primary: true,
    }];
    let same_a = runtime.execute(command(
        HarnessCommandPayload::LoadSession {
            session_id: SessionId::new("same").unwrap(),
            workspace_roots: roots.clone(),
        },
        100,
    ));
    let same_b = runtime.execute(command(
        HarnessCommandPayload::ResumeSession {
            session_id: SessionId::new("same").unwrap(),
            workspace_roots: roots.clone(),
        },
        101,
    ));
    let other = runtime.execute(command(
        HarnessCommandPayload::LoadSession {
            session_id: SessionId::new("other").unwrap(),
            workspace_roots: roots,
        },
        102,
    ));
    let listing = runtime.execute(command(
        HarnessCommandPayload::ListSessions(SessionListFilter::default()),
        103,
    ));
    let (same_a, same_b, other, listing) = tokio::join!(same_a, same_b, other, listing);
    let RuntimeResponse::Session(same_a) = same_a.unwrap() else {
        panic!("expected first same-ID session")
    };
    let RuntimeResponse::Session(same_b) = same_b.unwrap() else {
        panic!("expected second same-ID session")
    };
    let RuntimeResponse::Session(other) = other.unwrap() else {
        panic!("expected distinct session")
    };
    assert_eq!(same_a.session_id, same_b.session_id);
    assert_ne!(same_a.session_id, other.session_id);
    assert!(matches!(listing.unwrap(), RuntimeResponse::Sessions(_)));

    let RuntimeResponse::Sessions(final_list) = runtime
        .execute(command(
            HarnessCommandPayload::ListSessions(SessionListFilter::default()),
            104,
        ))
        .await
        .unwrap()
    else {
        panic!("expected final list")
    };
    assert_eq!(
        final_list
            .iter()
            .filter(|summary| summary.session_id.as_str() == "same")
            .count(),
        1
    );
    assert_eq!(final_list.len(), 2);
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn completed_persistent_read_cannot_overwrite_a_newer_in_memory_actor() {
    let root = std::env::temp_dir().join(format!("vesper-runtime-stale-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("stale.json"),
        serde_json::to_vec(&serde_json::json!({
            "cwd": "/fixture",
            "model": "fixture-model",
            "api_endpoint": "coding",
            "messages": [{"role": "assistant", "content": "stale-disk-content"}]
        }))
        .unwrap(),
    )
    .unwrap();
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let repository: Arc<dyn SessionRepository> = Arc::new(DelayedRepository {
        inner: FilesystemSessionStore::new(
            root.clone(),
            SessionSource::LegacyNativeGlm { profile: None },
            DiscoveryBounds::default(),
        )
        .unwrap(),
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    });
    let runtime = runtime_with_repository(repository).await;
    let roots = vec![WorkspaceRoot {
        name: BoundedString::new("workspace").unwrap(),
        path: BoundedString::new("/fixture").unwrap(),
        primary: true,
    }];
    let load_runtime = Arc::clone(&runtime);
    let load_roots = roots.clone();
    let load = tokio::spawn(async move {
        load_runtime
            .execute(command(
                HarnessCommandPayload::LoadSession {
                    session_id: SessionId::new("stale").unwrap(),
                    workspace_roots: load_roots,
                },
                110,
            ))
            .await
    });
    entered.notified().await;
    let RuntimeResponse::Session(created) = runtime
        .execute(command(
            HarnessCommandPayload::CreateSession {
                workspace_roots: roots,
                requested_session_id: Some(SessionId::new("stale").unwrap()),
            },
            111,
        ))
        .await
        .unwrap()
    else {
        panic!("expected newer in-memory session")
    };
    release.notify_one();
    let RuntimeResponse::Session(loaded) = load.await.unwrap().unwrap() else {
        panic!("expected load to adopt existing actor")
    };
    assert_eq!(created.source, SessionSource::InMemory);
    assert_eq!(loaded.source, SessionSource::InMemory);
    assert!(loaded.history.is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn no_tools_turn_preserves_event_order_and_message_identity() {
    let events = vec![
        Ok(ProviderStreamEvent::ResponseStarted {
            response_id: None,
            metadata: ExtensionMap::default(),
        }),
        Ok(ProviderStreamEvent::ReasoningDelta {
            stream_id: BoundedString::new("reasoning").unwrap(),
            text: ContentText::new("think").unwrap(),
            kind: vesper_domain::ReasoningKind::ProviderVisible,
            retention: vesper_domain::ReasoningRetention::Persist,
        }),
        Ok(ProviderStreamEvent::ContentDelta {
            stream_id: BoundedString::new("content").unwrap(),
            part: ContentPart::Text(ContentText::new("answer").unwrap()),
        }),
        Ok(ProviderStreamEvent::Completed {
            finish: FinishOutcome::Stop,
            metadata: ExtensionMap::default(),
        }),
    ];
    let runtime = runtime([Ok(events)]).await;
    let mut event_receiver = runtime.take_events().await.unwrap();
    let RuntimeResponse::Session(session) = runtime.execute(create(1)).await.unwrap() else {
        panic!("expected session");
    };
    let _ = event_receiver.recv().await;
    let message_id = MessageId::new("client-message").unwrap();
    let response = runtime
        .execute(command(
            HarnessCommandPayload::SubmitPrompt {
                session_id: session.session_id,
                prompt: PromptSubmission {
                    message_id: message_id.clone(),
                    content: vec![ContentPart::Text(ContentText::new("hello").unwrap())],
                    extensions: ExtensionMap::default(),
                },
            },
            2,
        ))
        .await
        .unwrap();
    let result = response.wait_prompt().await.unwrap();
    assert_eq!(result.user_message_id, message_id);
    assert_eq!(result.outcome, FinishOutcome::Stop);

    let mut kinds = Vec::new();
    while let Some(event) = event_receiver.recv().await {
        assert_eq!(event.sequence, EventSequence::new(kinds.len() as u64));
        kinds.push(match event.payload {
            vesper_domain::HarnessEventPayload::UserMessageAccepted { .. } => "user",
            vesper_domain::HarnessEventPayload::ResponseStarted { .. } => "started",
            vesper_domain::HarnessEventPayload::ReasoningDelta { .. } => "reasoning",
            vesper_domain::HarnessEventPayload::ContentDelta { .. } => "content",
            vesper_domain::HarnessEventPayload::TurnCompleted { .. } => "terminal",
            _ => "other",
        });
        if kinds.last() == Some(&"terminal") {
            break;
        }
    }
    assert_eq!(
        kinds,
        ["user", "started", "reasoning", "content", "terminal"]
    );
}

#[tokio::test]
async fn duplicate_and_unknown_providers_are_rejected() {
    let provider_id = ProviderId::new("test.fake").unwrap();
    let registry = ProviderRegistry::new();
    let factory = FakeFactory {
        id: provider_id.clone(),
        session: FakeProviderSession::default(),
    };
    registry.register(factory.clone()).await.unwrap();
    assert!(registry.register(factory).await.is_err());
    assert!(registry.contains(&provider_id).await);
    assert!(
        !registry
            .contains(&ProviderId::new("missing").unwrap())
            .await
    );
}

#[tokio::test]
async fn session_reasoning_override_threads_into_the_provider_request() {
    // ADR 0009 / Tier A: a session-scoped reasoning override set via the
    // UpdateSessionReasoning command must reach the provider request that the
    // next turn dispatches. This test holds the FakeProviderSession handle so
    // it can inspect the captured request directly.
    let provider_id = ProviderId::new("test.fake").unwrap();
    let events = vec![
        Ok(ProviderStreamEvent::ResponseStarted {
            response_id: None,
            metadata: ExtensionMap::default(),
        }),
        Ok(ProviderStreamEvent::ContentDelta {
            stream_id: BoundedString::new("content").unwrap(),
            part: ContentPart::Text(ContentText::new("ok").unwrap()),
        }),
        Ok(ProviderStreamEvent::Completed {
            finish: FinishOutcome::Stop,
            metadata: ExtensionMap::default(),
        }),
    ];
    let fake = FakeProviderSession::with_scripts([Ok(events)]);
    let providers = Arc::new(ProviderRegistry::new());
    providers
        .register(FakeFactory {
            id: provider_id.clone(),
            session: fake.clone(),
        })
        .await
        .unwrap();
    // No default reasoning: proves the override — not a default — drives the
    // request.
    let runtime = Arc::new(RuntimeSupervisor::new(
        providers,
        RuntimeDefaults {
            provider_configuration: configuration(&provider_id),
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
    ));

    let RuntimeResponse::Session(session) = runtime.execute(create(1)).await.unwrap() else {
        panic!("expected session");
    };

    // Apply the session-scoped reasoning override (the `/thinking max` path).
    runtime
        .execute(command(
            HarnessCommandPayload::UpdateSessionReasoning {
                session_id: session.session_id.clone(),
                mode: Some(BoundedString::new("max").unwrap()),
            },
            2,
        ))
        .await
        .unwrap();

    // Submit one prompt and let the turn finish so the request is captured.
    let response = runtime
        .execute(command(
            HarnessCommandPayload::SubmitPrompt {
                session_id: session.session_id.clone(),
                prompt: PromptSubmission {
                    message_id: MessageId::new("client-message").unwrap(),
                    content: vec![ContentPart::Text(ContentText::new("hello").unwrap())],
                    extensions: ExtensionMap::default(),
                },
            },
            3,
        ))
        .await
        .unwrap();
    response.wait_prompt().await.unwrap();

    let captured = fake.requests();
    assert_eq!(captured.len(), 1, "exactly one provider turn should run");
    let reasoning = captured[0]
        .reasoning
        .as_ref()
        .expect("request must carry the session reasoning override");
    assert_eq!(
        reasoning.mode.as_ref().map(|mode| mode.as_str()),
        Some("max"),
        "the session override must thread into ProviderRequest.reasoning.mode"
    );
}

#[tokio::test]
async fn clearing_the_session_reasoning_falls_back_to_the_runtime_default() {
    // With no session override, the runtime default reasoning (here `high`)
    // must be the value carried into the request. Then clearing an explicit
    // override must restore that fallback.
    let provider_id = ProviderId::new("test.fake").unwrap();
    let events = || {
        vec![Ok(ProviderStreamEvent::Completed {
            finish: FinishOutcome::Stop,
            metadata: ExtensionMap::default(),
        })]
    };
    let fake = FakeProviderSession::with_scripts([Ok(events()), Ok(events())]);
    let providers = Arc::new(ProviderRegistry::new());
    providers
        .register(FakeFactory {
            id: provider_id.clone(),
            session: fake.clone(),
        })
        .await
        .unwrap();
    let runtime = Arc::new(RuntimeSupervisor::new(
        providers,
        RuntimeDefaults {
            provider_configuration: configuration(&provider_id),
            model: QualifiedModelId {
                provider_id,
                model_id: ModelId::new("fixture-model").unwrap(),
            },
            endpoint: EndpointId::new("test-endpoint").unwrap(),
            system_instructions: Vec::new(),
            reasoning: Some(vesper_provider::ReasoningIntent {
                mode: Some(BoundedString::new("high").unwrap()),
                stream_visible: true,
                retention: vesper_domain::ReasoningRetention::SessionOnly,
            }),
            sampling: None,
            maximum_output_tokens: None,
        },
    ));

    let RuntimeResponse::Session(session) = runtime.execute(create(1)).await.unwrap() else {
        panic!("expected session");
    };

    let prompt = |number: u64| {
        command(
            HarnessCommandPayload::SubmitPrompt {
                session_id: session.session_id.clone(),
                prompt: PromptSubmission {
                    message_id: MessageId::new(format!("m-{number}")).unwrap(),
                    content: vec![ContentPart::Text(ContentText::new("hi").unwrap())],
                    extensions: ExtensionMap::default(),
                },
            },
            number,
        )
    };

    // First turn: no session override → default `high` applies.
    runtime
        .execute(prompt(2))
        .await
        .unwrap()
        .wait_prompt()
        .await
        .unwrap();
    // Set an override.
    runtime
        .execute(command(
            HarnessCommandPayload::UpdateSessionReasoning {
                session_id: session.session_id.clone(),
                mode: Some(BoundedString::new("max").unwrap()),
            },
            3,
        ))
        .await
        .unwrap();
    // Clear the override.
    runtime
        .execute(command(
            HarnessCommandPayload::UpdateSessionReasoning {
                session_id: session.session_id.clone(),
                mode: None,
            },
            4,
        ))
        .await
        .unwrap();
    // Second turn: cleared → falls back to default `high`.
    runtime
        .execute(prompt(5))
        .await
        .unwrap()
        .wait_prompt()
        .await
        .unwrap();

    let captured = fake.requests();
    assert_eq!(captured.len(), 2);
    assert_eq!(
        captured[1]
            .reasoning
            .as_ref()
            .and_then(|intent| intent.mode.as_ref())
            .map(|mode| mode.as_str()),
        Some("high"),
        "clearing the override must restore the runtime default reasoning"
    );
}
