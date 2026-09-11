//! Offline historical-gap replay with real Rust compilation/execution. Reviewer
//! replies are controlled fixtures; these results do not certify an LLM's judgment.
use super::*;
use std::sync::atomic::AtomicUsize;
use vesper_agent::acceptance::CompletionPort;
use vesper_domain::{BoundedString, ContentText, ExtensionMap, ToolCallId, ToolId};

struct Reviewer {
    calls: AtomicUsize,
    finding: Mutex<bool>,
}
impl AcceptanceReviewer for Reviewer {
    fn inspect<'a>(
        &'a self,
        _root: &'a Path,
        request: String,
        _cancel: Arc<dyn vesper_agent::CancellationSignal>,
    ) -> ToolFuture<'a, Result<String, String>> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Box::pin(async move {
            if request.starts_with("Create the complete") {
                return json(&contract());
            }
            let finding = *self.finding.lock().unwrap();
            json(&AcceptanceReview {
                inspected_source_ids: vec!["source-001".into()],
                inspected_scenario_ids: vec!["S1".into()],
                findings: if finding {
                    vec![AcceptanceFinding {
                        subject: "S1".into(),
                        evidence: "src/lib.rs lacks a tested cancellation path".into(),
                        repair: "implement cancellation and its actual lifecycle assertion".into(),
                    }]
                } else {
                    vec![]
                },
            })
        })
    }
}
fn contract() -> AcceptanceContract {
    AcceptanceContract {
        version: 1,
        objective: "Return the required answer".into(),
        requirements: vec![AcceptanceRequirement {
            id: "R1".into(),
            description: "answer returns 42".into(),
            source_ids: vec!["source-001".into()],
            scenarios: vec![AcceptanceScenario {
                id: "S1".into(),
                assertion: "Calling answer returns exactly 42".into(),
                scope: "Rust library".into(),
                evidence: AcceptanceEvidenceKind::Unit,
                platform: AcceptancePlatform::Any,
            }],
        }],
        context: vec![],
    }
}
fn check() -> AcceptanceCheck {
    AcceptanceCheck {
        id: "C1".into(),
        scenario_ids: vec!["S1".into()],
        evidence: AcceptanceEvidenceKind::Unit,
        platform: AcceptancePlatform::Any,
        scope: "Rust library".into(),
        package: "acceptance-fixture".into(),
        target: None,
        test: "answer_is_42".into(),
        features: vec![],
        all_features: false,
        ignored: false,
        timeout_seconds: 30,
    }
}
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"acceptance-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("Cargo.lock"),
        "version = 3\n\n[[package]]\nname = \"acceptance-fixture\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("PRD.md"),
        "The answer function must return 42.",
    )
    .unwrap();
    implementation(dir.path(), false);
    dir
}
fn implementation(root: &Path, fixed: bool) {
    std::fs::write(root.join("src/lib.rs"), format!("pub fn answer() -> u32 {{ {} }}\n#[test]\nfn answer_is_42() {{ assert_eq!(answer(), 42); }}\n", if fixed { 42 } else { 0 })).unwrap();
}
fn context(root: &Path) -> ToolContext {
    vesper_agent::executor::uncancellable_context(
        vec![WorkspaceRoot {
            name: BoundedString::new("fixture").unwrap(),
            path: BoundedString::new(root.to_string_lossy()).unwrap(),
            primary: true,
        }],
        SessionOperatingMode::Code,
        SessionPermissionMode::Bypass,
    )
}
fn reviewer() -> Arc<Reviewer> {
    Arc::new(Reviewer {
        calls: AtomicUsize::new(0),
        finding: Mutex::new(false),
    })
}

#[tokio::test]
async fn real_failing_check_repairs_then_source_edit_invalidates_success() {
    let root = project();
    let reviewer = reviewer();
    let session = AcceptanceSession::open(root.path(), "PRD.md", reviewer.clone()).unwrap();
    let context = context(root.path());
    session.prepare(&context).await.unwrap();
    assert!(!session.evaluate(&context).await.is_verified());
    session.configure(vec![check()], &context).await.unwrap();
    let failed = session.verify(&context).await.unwrap();
    assert!(failed.contains("INCOMPLETE"), "{failed}");
    assert_eq!(session.status().receipts[0].state, AcceptanceState::Failed);
    implementation(root.path(), true);
    let passed = session.verify(&context).await.unwrap();
    assert!(session.status().is_verified(), "{passed}");
    let run_id = session.status().receipts[0].run_id;
    session.verify(&context).await.unwrap();
    assert_eq!(
        session.status().receipts[0].run_id,
        run_id,
        "unchanged evidence is reused"
    );
    implementation(root.path(), false);
    assert!(!session.refresh_status().is_verified());
    assert!(
        session
            .refresh_status()
            .gaps
            .iter()
            .any(|g| g.state == AcceptanceState::Stale)
    );
    assert!(
        !root.path().join(".agent-vesper").exists(),
        "no implicit durable state"
    );
    assert!(
        !root.path().join("target").exists(),
        "checks ran on captured copies"
    );
}

#[tokio::test]
async fn zero_matches_and_ignored_tests_are_not_passing_receipts() {
    let root = project();
    implementation(root.path(), true);
    let session = AcceptanceSession::open(root.path(), "PRD.md", reviewer()).unwrap();
    let context = context(root.path());
    session.prepare(&context).await.unwrap();
    let mut c = check();
    c.test = "does_not_exist".into();
    session.configure(vec![c], &context).await.unwrap();
    session.verify(&context).await.unwrap();
    assert_eq!(
        session.status().receipts[0].state,
        AcceptanceState::Inconclusive
    );
    std::fs::write(
        root.path().join("src/lib.rs"),
        "#[test]\n#[ignore]\nfn answer_is_42() { panic!(\"must actually run\"); }\n",
    )
    .unwrap();
    session.configure(vec![check()], &context).await.unwrap();
    session.verify(&context).await.unwrap();
    assert_eq!(
        session.status().receipts[0].state,
        AcceptanceState::Inconclusive
    );
}

#[tokio::test]
async fn forged_receipts_scope_changes_and_cross_workspace_use_refuse() {
    let root = project();
    let session = AcceptanceSession::open(root.path(), "PRD.md", reviewer()).unwrap();
    let context = context(root.path());
    session.prepare(&context).await.unwrap();
    let call = ToolCall {
        id: ToolCallId::new("forged").unwrap(),
        tool_id: ToolId::new("acceptance_verify").unwrap(),
        arguments: serde_json::json!({"receipts":[{"state":"verified"}]}),
        extensions: ExtensionMap::default(),
    };
    assert!(session.execute(&call, &context).await.is_err());
    let foreign = project();
    assert!(
        session
            .verify(&super::tests::context(foreign.path()))
            .await
            .is_err()
    );
    std::fs::write(root.path().join("PRD.md"), "A reduced objective.").unwrap();
    assert!(!session.evaluate(&context).await.is_verified());
    assert!(session.status().render().contains("original PRD changed"));
    assert_eq!(
        session.status().total_scenarios,
        1,
        "errors cannot erase required scenarios"
    );
    assert!(session.status().gaps.iter().any(|gap| gap.subject == "S1"));
}

#[tokio::test]
async fn unresolved_independent_findings_block_even_real_green_tests() {
    let root = project();
    implementation(root.path(), true);
    let reviewer = reviewer();
    let session = AcceptanceSession::open(root.path(), "PRD.md", reviewer.clone()).unwrap();
    let context = context(root.path());
    session.prepare(&context).await.unwrap();
    session.configure(vec![check()], &context).await.unwrap();
    *reviewer.finding.lock().unwrap() = true;
    session.verify(&context).await.unwrap();
    assert_eq!(
        session.status().receipts[0].state,
        AcceptanceState::Verified
    );
    assert!(!session.status().is_verified());
    assert!(session.status().render().contains("cancellation"));
}

#[test]
fn captured_inputs_are_independent_and_include_untracked_hidden_build_files() {
    let root = project();
    std::fs::create_dir(root.path().join(".cargo")).unwrap();
    std::fs::write(root.path().join(".cargo/config.toml"), "[build]\njobs=1\n").unwrap();
    let first = SourceSnapshot::capture(root.path()).unwrap();
    implementation(root.path(), true);
    assert_ne!(
        first.digest,
        SourceSnapshot::capture(root.path()).unwrap().digest
    );
    let copy = first.materialize().unwrap();
    assert!(
        std::fs::read_to_string(copy.path().join("src/lib.rs"))
            .unwrap()
            .contains("{ 0 }")
    );
    assert!(copy.path().join(".cargo/config.toml").exists());
}

#[test]
fn arbitrary_pass_prose_and_failure_exit_do_not_count() {
    for output in [
        "PASS",
        "all tests passed",
        "test result: ok. 0 passed; 0 failed; 0 ignored;",
        "running 1 test\ntest answer_is_42 ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored;\ntest result: ok. 0 passed; 0 failed; 0 ignored;",
        "test answer_is_42 ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored;",
    ] {
        assert_ne!(
            crate::acceptance_runner::classify(&check(), true, output).0,
            AcceptanceState::Verified
        );
    }
    assert_eq!(
        crate::acceptance_runner::classify(
            &check(),
            false,
            "test answer_is_42 ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored;"
        )
        .0,
        AcceptanceState::Failed
    );
    let _ = ContentText::new("bounded").unwrap();
}

#[derive(Clone)]
struct ProviderFixture {
    session: vesper_testkit::FakeProviderSession,
    id: vesper_domain::ProviderId,
}
impl vesper_provider::ProviderFactory for ProviderFixture {
    type Session = vesper_testkit::FakeProviderSession;
    fn provider_id(&self) -> &vesper_domain::ProviderId {
        &self.id
    }
    fn create_session<'a>(
        &'a self,
        _: &'a vesper_provider::ProviderConfiguration,
        _: Arc<dyn vesper_agent::CancellationSignal>,
    ) -> vesper_provider::ProviderFuture<'a, Result<Self::Session, vesper_provider::ProviderError>>
    {
        Box::pin(async move { Ok(self.session.clone()) })
    }
}
fn stop(
    text: &str,
) -> Vec<Result<vesper_provider::ProviderStreamEvent, vesper_provider::ProviderError>> {
    vec![
        Ok(vesper_provider::ProviderStreamEvent::ContentDelta {
            stream_id: BoundedString::new("text").unwrap(),
            part: vesper_domain::ContentPart::Text(ContentText::new(text).unwrap()),
        }),
        Ok(vesper_provider::ProviderStreamEvent::Completed {
            finish: vesper_domain::FinishOutcome::Stop,
            metadata: ExtensionMap::default(),
        }),
    ]
}
fn tool(
    name: &str,
    args: serde_json::Value,
) -> Vec<Result<vesper_provider::ProviderStreamEvent, vesper_provider::ProviderError>> {
    vec![
        Ok(vesper_provider::ProviderStreamEvent::ToolCallCompleted(
            ToolCall {
                id: ToolCallId::new(format!("call-{name}")).unwrap(),
                tool_id: ToolId::new(name).unwrap(),
                arguments: args,
                extensions: ExtensionMap::default(),
            },
        )),
        Ok(vesper_provider::ProviderStreamEvent::Completed {
            finish: vesper_domain::FinishOutcome::ToolCalls,
            metadata: ExtensionMap::default(),
        }),
    ]
}
fn config(root: &Path, id: &vesper_domain::ProviderId) -> vesper_agent::AgentLoopConfig {
    vesper_agent::AgentLoopConfig {
        provider_id: id.clone(),
        provider_configuration: vesper_provider::ProviderConfiguration {
            provider_id: id.clone(),
            values: vesper_domain::VersionedExtensionEnvelope {
                namespace: vesper_domain::ExtensionNamespace::new("provider.fixture").unwrap(),
                version: vesper_domain::SchemaVersion::new(1).unwrap(),
                values: ExtensionMap::default(),
            },
        },
        model: vesper_domain::QualifiedModelId {
            provider_id: id.clone(),
            model_id: vesper_domain::ModelId::new("fixture").unwrap(),
        },
        context_window_tokens: 128_000,
        system_instructions: vec![],
        workspace_roots: context(root).workspace_roots,
        max_tool_iterations: 16,
        firewall: None,
        sandbox: None,
    }
}
#[derive(Default)]
struct Progress(Mutex<Vec<vesper_agent::AgentProgressEvent>>);
impl vesper_agent::AgentProgressPort for Progress {
    fn emit(&self, e: vesper_agent::AgentProgressEvent) {
        self.0.lock().unwrap().push(e);
    }
}

#[tokio::test]
async fn native_agent_cannot_stop_early_then_repairs_and_earns_real_evidence() {
    let root = project();
    let acceptance = AcceptanceSession::open(root.path(), "PRD.md", reviewer()).unwrap();
    let fake = vesper_testkit::FakeProviderSession::with_scripts([
        Ok(stop("Everything is completed and all tests pass.")),
        Ok(tool(
            "write_file",
            serde_json::json!({"path":"src/lib.rs", "content":"pub fn answer() -> u32 { 42 }\n#[test]\nfn answer_is_42() { assert_eq!(answer(), 42); }\n"}),
        )),
        Ok(tool(
            "acceptance_configure",
            serde_json::json!({"checks":[check()]}),
        )),
        Ok(tool("acceptance_verify", serde_json::json!({}))),
        Ok(stop("The whole application is bug free!")),
    ]);
    let observed = fake.clone();
    let id = vesper_domain::ProviderId::new("fixture.acceptance").unwrap();
    let registry = Arc::new(vesper_runtime::ProviderRegistry::new());
    registry
        .register(ProviderFixture {
            id: id.clone(),
            session: fake,
        })
        .await
        .unwrap();
    let progress = Arc::new(Progress::default());
    let agent = acceptance
        .attach(vesper_agent::AgentLoop::new(
            registry,
            vesper_agent::ToolRegistry::parity_default(),
            config(root.path(), &id),
        ))
        .with_progress_port(progress.clone());
    let (outcome, history) = agent
        .run_prompt_with_history(
            vec![crate::build_user_message("Implement this PRD fully")],
            SessionOperatingMode::Code,
            SessionPermissionMode::Bypass,
        )
        .await
        .unwrap();
    let vesper_agent::AgentTurnOutcome::Acceptance { report, .. } = outcome else {
        panic!("missing authoritative outcome")
    };
    assert!(report.is_verified(), "{}", report.render());
    assert_eq!(
        observed.requests().len(),
        5,
        "early stop required autonomous continuation"
    );
    let requests = observed.requests();
    assert!(format!("{:?}", requests[1]).contains("Completion refused"));
    assert!(
        std::fs::read_to_string(root.path().join("src/lib.rs"))
            .unwrap()
            .contains("{ 42 }")
    );
    assert!(
        !progress
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, vesper_agent::AgentProgressEvent::ContentDelta { .. })),
        "unverified prose never streamed"
    );
    let final_text = history
        .last()
        .unwrap()
        .content
        .iter()
        .filter_map(|p| {
            if let vesper_domain::ContentPart::Text(t) = p {
                Some(t.as_str())
            } else {
                None
            }
        })
        .collect::<String>();
    assert!(final_text.contains("Implementation acceptance: VERIFIED"));
    assert!(!final_text.contains("bug free"));
}

#[tokio::test]
async fn repeated_false_completion_stops_incomplete_at_the_bound() {
    let root = project();
    let acceptance = AcceptanceSession::open(root.path(), "PRD.md", reviewer()).unwrap();
    let fake = vesper_testkit::FakeProviderSession::with_scripts(
        (0..4).map(|_| Ok(stop("Done, all requirements implemented."))),
    );
    let observed = fake.clone();
    let id = vesper_domain::ProviderId::new("fixture.acceptance").unwrap();
    let registry = Arc::new(vesper_runtime::ProviderRegistry::new());
    registry
        .register(ProviderFixture {
            id: id.clone(),
            session: fake,
        })
        .await
        .unwrap();
    let agent = acceptance
        .attach(vesper_agent::AgentLoop::new(
            registry,
            vesper_agent::ToolRegistry::parity_default(),
            config(root.path(), &id),
        ))
        .with_active_plan(Some("[x] Everything".into()));
    let outcome = agent
        .run_prompt(
            crate::build_user_message("Implement fully"),
            SessionOperatingMode::Code,
            SessionPermissionMode::Bypass,
        )
        .await
        .unwrap();
    assert!(!outcome.is_success());
    assert_eq!(observed.requests().len(), 4);
    assert!(matches!(
        outcome,
        vesper_agent::AgentTurnOutcome::Acceptance { .. }
    ));
}

async fn fixture_factory(
    root: &Path,
    scripts: Vec<Vec<Result<vesper_provider::ProviderStreamEvent, vesper_provider::ProviderError>>>,
) -> (crate::WorkerFactory, vesper_testkit::FakeProviderSession) {
    let fake = vesper_testkit::FakeProviderSession::with_scripts(scripts.into_iter().map(Ok));
    let id = vesper_domain::ProviderId::new("fixture.acceptance").unwrap();
    let registry = Arc::new(vesper_runtime::ProviderRegistry::new());
    registry
        .register(ProviderFixture {
            id: id.clone(),
            session: fake.clone(),
        })
        .await
        .unwrap();
    (crate::WorkerFactory::new(registry, config(root, &id)), fake)
}

#[tokio::test]
async fn delegated_finish_and_empty_deleted_or_replaced_plans_cannot_certify_parent() {
    for plan in [
        None,
        Some(""),
        Some("[x] unrelated replacement"),
        Some("[x] all done"),
    ] {
        let root = project();
        let acceptance = AcceptanceSession::open(root.path(), "PRD.md", reviewer()).unwrap();
        let (factory, observed) = fixture_factory(
            root.path(),
            (0..4).map(|_| stop("All workers passed. Done.")).collect(),
        )
        .await;
        let agent = acceptance
            .attach(vesper_agent::AgentLoop::new(
                factory.registry,
                vesper_agent::ToolRegistry::parity_default(),
                factory.config,
            ))
            .with_active_plan(plan.map(str::to_owned));
        let (outcome, history) = agent
            .finish_delegated_acceptance(
                vec![crate::build_user_message("Implement")],
                "ReAct Finish: success; VRO candidate: verified; Swarm workers: successful",
                SessionOperatingMode::Code,
                SessionPermissionMode::Bypass,
                Arc::new(vesper_runtime::RuntimeCancellation::new()),
            )
            .await
            .unwrap();
        assert!(!outcome.is_success());
        assert_eq!(observed.requests().len(), 4);
        assert!(format!("{:?}", history.last()).contains("INCOMPLETE"));
    }
}

#[tokio::test]
async fn cancelled_and_exhausted_turns_publish_incomplete_history() {
    for cancelled in [true, false] {
        let root = project();
        let acceptance = AcceptanceSession::open(root.path(), "PRD.md", reviewer()).unwrap();
        let (mut factory, _) = fixture_factory(
            root.path(),
            (0..4)
                .map(|_| tool("read_file", serde_json::json!({"path":"PRD.md"})))
                .collect(),
        )
        .await;
        factory.config.max_tool_iterations = 1;
        let agent = acceptance.attach(vesper_agent::AgentLoop::new(
            factory.registry,
            vesper_agent::ToolRegistry::parity_default(),
            factory.config,
        ));
        let cancel = Arc::new(vesper_runtime::RuntimeCancellation::new());
        if cancelled {
            cancel.cancel();
        }
        let (outcome, history) = agent
            .run_prompt_with_history_with_cancellation(
                vec![crate::build_user_message("Implement")],
                SessionOperatingMode::Code,
                SessionPermissionMode::Bypass,
                cancel,
            )
            .await
            .unwrap();
        assert!(!outcome.is_success());
        assert!(format!("{:?}", history.last()).contains("INCOMPLETE"));
    }
}

#[tokio::test]
async fn reviewer_unavailable_or_without_source_inspection_cannot_approve() {
    let root = project();
    let (factory, _) = fixture_factory(root.path(), vec![stop(r#"{"inspected_source_ids":["source-001"],"inspected_scenario_ids":["S1"],"findings":[]}"#)]).await;
    let reviewer = NativeAcceptanceReviewer::new(factory);
    let result = reviewer
        .inspect(
            root.path(),
            "Final gap review: inspect src/lib.rs".into(),
            Arc::new(vesper_runtime::RuntimeCancellation::new()),
        )
        .await;
    assert!(result.unwrap_err().contains("did not inspect"));
    struct Unavailable;
    impl AcceptanceReviewer for Unavailable {
        fn inspect<'a>(
            &'a self,
            _: &'a Path,
            _: String,
            _: Arc<dyn vesper_agent::CancellationSignal>,
        ) -> ToolFuture<'a, Result<String, String>> {
            Box::pin(async { Err("reviewer unavailable".into()) })
        }
    }
    let session = AcceptanceSession::open(root.path(), "PRD.md", Arc::new(Unavailable)).unwrap();
    assert!(session.prepare(&context(root.path())).await.is_err());
    assert!(!session.evaluate(&context(root.path())).await.is_verified());
}

#[tokio::test]
async fn independent_reexamination_resolves_spurious_finding_without_forged_override() {
    let root = project();
    implementation(root.path(), true);
    let reviewer = reviewer();
    let session = AcceptanceSession::open(root.path(), "PRD.md", reviewer.clone()).unwrap();
    let context = context(root.path());
    session.prepare(&context).await.unwrap();
    session.configure(vec![check()], &context).await.unwrap();
    *reviewer.finding.lock().unwrap() = true;
    session.verify(&context).await.unwrap();
    assert!(!session.status().is_verified());
    let receipt_id = session.status().receipts[0].run_id;
    *reviewer.finding.lock().unwrap() = false;
    let call = ToolCall {
        id: ToolCallId::new("review").unwrap(),
        tool_id: ToolId::new("acceptance_review").unwrap(),
        arguments: serde_json::json!({}),
        extensions: ExtensionMap::default(),
    };
    session.execute(&call, &context).await.unwrap();
    assert!(session.status().is_verified());
    assert_eq!(receipt_id, session.status().receipts[0].run_id);
}

#[tokio::test]
async fn wrong_platform_and_verification_timeout_fail_closed() {
    let root = project();
    implementation(root.path(), true);
    let mut check = check();
    check.platform = if cfg!(windows) {
        AcceptancePlatform::Linux
    } else {
        AcceptancePlatform::Windows
    };
    assert!(
        crate::acceptance_runner::run(
            check.clone(),
            SourceSnapshot::capture(root.path()).unwrap(),
            &context(root.path())
        )
        .await
        .unwrap_err()
        .contains("platform")
    );
    check.platform = AcceptancePlatform::Any;
    check.timeout_seconds = 1;
    std::fs::write(
        root.path().join("src/lib.rs"),
        "#[test] fn answer_is_42() { std::thread::sleep(std::time::Duration::from_secs(20)); }",
    )
    .unwrap();
    let started = std::time::Instant::now();
    let result = crate::acceptance_runner::run(
        check,
        SourceSnapshot::capture(root.path()).unwrap(),
        &context(root.path()),
    )
    .await;
    assert!(result.is_err());
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
}

#[tokio::test]
async fn explicit_scope_revision_and_resume_preserve_history_but_never_import_verification() {
    let root = project();
    let session = AcceptanceSession::open(root.path(), "PRD.md", reviewer()).unwrap();
    session.prepare(&context(root.path())).await.unwrap();
    let (factory, _) = fixture_factory(root.path(), vec![]).await;
    let mut active = Some(session);
    std::fs::write(
        root.path().join("revised.md"),
        "The answer must be 43 instead.",
    )
    .unwrap();
    control(
        &mut active,
        "revise revised.md",
        root.path(),
        factory.clone(),
    )
    .unwrap();
    let revised = active.as_ref().unwrap();
    assert_eq!(revised.lineage.len(), 1);
    assert!(!revised.lineage[0].is_verified());
    let mut exported: serde_json::Value = serde_json::from_str(&revised.export().unwrap()).unwrap();
    // Even an edited historical report is data, never live authority.
    exported["report"]["gaps"] = serde_json::json!([]);
    exported["report"]["total_scenarios"] = serde_json::json!(1);
    exported["report"]["verified_scenarios"] = serde_json::json!(1);
    std::fs::write(root.path().join("audit.json"), exported.to_string()).unwrap();
    active = None;
    control(&mut active, "resume audit.json", root.path(), factory).unwrap();
    assert!(!active.unwrap().status().is_verified());
    assert!(!root.path().join(".agent-vesper").exists());
}

#[tokio::test]
async fn verification_never_bypasses_shell_permission_or_command_firewall() {
    let root = project();
    implementation(root.path(), true);
    let acceptance = AcceptanceSession::open(root.path(), "PRD.md", reviewer()).unwrap();
    acceptance.prepare(&context(root.path())).await.unwrap();
    acceptance
        .configure(vec![check()], &context(root.path()))
        .await
        .unwrap();
    let mut scripts = vec![tool("acceptance_verify", serde_json::json!({}))];
    scripts.extend((0..4).map(|_| stop("Done")));
    let (factory, _) = fixture_factory(root.path(), scripts).await;
    let agent = acceptance.attach(vesper_agent::AgentLoop::new(
        factory.registry,
        vesper_agent::ToolRegistry::parity_default(),
        factory.config,
    ));
    let result = agent
        .run_prompt(
            crate::build_user_message("Implement"),
            SessionOperatingMode::Code,
            SessionPermissionMode::Ask,
        )
        .await
        .unwrap();
    assert!(!result.is_success());
    assert!(
        acceptance.status().receipts.is_empty(),
        "denied shell never executes verification"
    );
    let mut context = context(root.path());
    context.firewall = vesper_agent::vro::scope::compose_scope_firewall(
        None,
        &[(
            r"\bcargo\b",
            vesper_policy::firewall::RuleDecision::Deny,
            "test policy denies cargo",
        )],
    )
    .unwrap()
    .map(Arc::new);
    acceptance.verify(&context).await.unwrap();
    assert!(!acceptance.status().is_verified());
    assert!(
        acceptance.status().receipts[0]
            .diagnostic
            .contains("firewall")
    );
}

#[tokio::test]
async fn publication_rechecks_live_source_after_snapshot_review() {
    struct MutatingReview {
        root: PathBuf,
    }
    impl AcceptanceReviewer for MutatingReview {
        fn inspect<'a>(
            &'a self,
            _: &'a Path,
            request: String,
            _: Arc<dyn vesper_agent::CancellationSignal>,
        ) -> ToolFuture<'a, Result<String, String>> {
            Box::pin(async move {
                if request.starts_with("Create the complete") {
                    return json(&contract());
                }
                if request.starts_with("Final gap review") {
                    implementation(&self.root, false);
                }
                json(&AcceptanceReview {
                    inspected_source_ids: vec!["source-001".into()],
                    inspected_scenario_ids: vec!["S1".into()],
                    findings: vec![],
                })
            })
        }
    }
    let root = project();
    implementation(root.path(), true);
    let session = AcceptanceSession::open(
        root.path(),
        "PRD.md",
        Arc::new(MutatingReview {
            root: root.path().into(),
        }),
    )
    .unwrap();
    let context = context(root.path());
    session.prepare(&context).await.unwrap();
    session.configure(vec![check()], &context).await.unwrap();
    session.verify(&context).await.unwrap();
    assert_eq!(
        session.status().receipts[0].state,
        AcceptanceState::Verified,
        "the captured source really passed"
    );
    assert!(
        !session.status().is_verified(),
        "changed live source cannot inherit that receipt"
    );
    assert!(
        session
            .status()
            .gaps
            .iter()
            .any(|gap| gap.state == AcceptanceState::Stale)
    );
}

#[tokio::test]
async fn project_rules_are_frozen_and_runtime_configuration_is_snapshotted() {
    let root = project();
    std::fs::write(
        root.path().join("AGENTS.md"),
        "All native hosts must expose this behavior.",
    )
    .unwrap();
    let session = AcceptanceSession::open(root.path(), "PRD.md", reviewer()).unwrap();
    assert_eq!(session.sources.len(), 2);
    assert!(session.sources[1].text.contains("AGENTS.md"));
    std::fs::write(
        root.path().join("AGENTS.md"),
        "The requirement was removed.",
    )
    .unwrap();
    assert!(session.source_unchanged().is_ok());
    assert!(
        session.sources[1].text.contains("All native hosts"),
        "edits cannot replace the original obligations"
    );
    std::fs::create_dir(root.path().join(".agent-vesper")).unwrap();
    std::fs::write(
        root.path().join(".agent-vesper/config.toml"),
        "[sandbox]\nfilesystem=true\n",
    )
    .unwrap();
    let before = SourceSnapshot::capture(root.path()).unwrap().digest;
    std::fs::write(
        root.path().join(".agent-vesper/config.toml"),
        "[sandbox]\nfilesystem=false\n",
    )
    .unwrap();
    assert_ne!(before, SourceSnapshot::capture(root.path()).unwrap().digest);
}

#[tokio::test]
async fn saved_activation_restores_unverified_scope_and_cannot_disable_an_active_contract() {
    let root = project();
    crate::acceptance_settings::AcceptanceSettings {
        enabled: true,
        prd: "PRD.md".into(),
    }
    .save(root.path())
    .unwrap();
    let (factory, _) = fixture_factory(root.path(), vec![]).await;
    let mut active = None;
    control(&mut active, "status", root.path(), factory.clone()).unwrap();
    let identity = active.as_ref().unwrap().clone();
    assert!(!identity.status().is_verified());
    crate::acceptance_settings::AcceptanceSettings::default()
        .save(root.path())
        .unwrap();
    activate_saved(&mut active, root.path(), factory).unwrap();
    assert!(
        Arc::ptr_eq(&identity, active.as_ref().unwrap()),
        "workspace edits cannot switch off live authority"
    );
}
