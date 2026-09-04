//! VRO-13 PR-8 — cross-feature end-to-end fixture.
//!
//! Exercises the full pipeline the PRD §5.2 PR-8 exit names:
//!
//! **cron/watcher fire → shell command → composed CommandFirewall →
//! opt-in sandbox route → scope-keyed transcript**, in one composed flow
//! at the layer that owns the composition (`vesper-harness`):
//!
//! 1. **Trigger (PR-6/PR-7).** A watcher recorded in `watchers.jsonl`
//!    under the project's scope id fires through the real
//!    `run_sweep_once` sweep. The fire closure mirrors the scheduler
//!    discipline (`run_cron_scheduler`): exactly-once `claim_slot`, one
//!    bounded agent turn, then the durable `mark_fired` outcome row.
//! 2. **Command (Tier C loop).** A scripted `FakeProviderSession` emits a
//!    `run_command` tool call inside a real `AgentLoop` — no live provider
//!    (Project Contract: no live provider calls in foundation
//!    verification).
//! 3. **Firewall (PR-1/PR-2/PR-5).** The loop carries the scope-composed
//!    `CommandFirewall` (global base ∪ project rules, deny-precedence). A
//!    destructive command is denied with the model-facing
//!    `tool error: [VRO-13 Firewall] denied:` observation even under
//!    Bypass, and a project rule denies its own command class.
//! 4. **Sandbox (PR-3/PR-4).** A benign command under an active
//!    `[sandbox]` demand (parsed by the real `vesper-config` reader)
//!    routes through the fail-closed `satisfies_demand` gate into the
//!    sandboxed execution port — never silently unsandboxed. When the
//!    resolved backend cannot satisfy the demand, the executor refuses
//!    with the model-facing `sandbox unavailable` text.
//! 5. **Transcript (PR-5/PR-6).** The fire outcome is retained on the
//!    cron entry (`last_output`, the bounded transcript the harness
//!    surfaces) and the sweep event ledger (`watcher-events.jsonl`)
//!    carries the scope id — audit trails keyed by scope identity, not
//!    by session or path.
//!
//! Determinism: every clock input to the sweep/scheduler seam is injected
//! (`SystemTime` values), the provider is scripted, and no test depends on
//! a Docker daemon. The real-daemon layer stays behind the same
//! `#[ignore]` + env gate as `vesper-sandbox/tests/docker.rs`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;
use vesper_agent::sandbox_route::{
    CapabilityStatus, IsolationRequirement, SandboxBackendChoice, SandboxBackendPort,
    SandboxCapabilities, SandboxDemand, SandboxOutcome, SandboxRoute, SandboxRunError,
    SecurityStrength,
};
use vesper_agent::vro::scope::{ScopeInputs, StampPolicy, WorkspaceScope};
use vesper_agent::{AgentLoop, AgentLoopConfig, AgentTurnOutcome, ToolRegistry};
use vesper_domain::{
    BoundedString, ContentPart, ContentText, ExtensionMap, ExtensionNamespace, FinishOutcome,
    MessageId, MessageRole, ModelId, ProviderId, QualifiedModelId, SchemaVersion,
    SessionOperatingMode, SessionPermissionMode, ToolCall, ToolCallId, ToolId,
    VersionedExtensionEnvelope,
};
use vesper_harness::sandbox_backend::BackendPort;
use vesper_harness::watcher_sweep::run_sweep_once;
use vesper_policy::firewall::RuleDecision;
use vesper_policy::firewall::rules::CommandFirewall;
use vesper_provider::{
    CancellationSignal, ProviderConfiguration, ProviderError, ProviderFactory, ProviderFuture,
    ProviderStreamEvent,
};
use vesper_runtime::ProviderRegistry;
use vesper_testkit::FakeProviderSession;

// ---------------------------------------------------------------------------
// fixture
// ---------------------------------------------------------------------------

/// One isolated project scope: project root, global layer root, and the
/// `.agent-vesper/` state directory carrying an active `[sandbox]` demand.
struct Fixture {
    #[allow(dead_code)]
    project: tempfile::TempDir,
    #[allow(dead_code)]
    global: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let project = tempfile::tempdir().expect("project root");
        let global = tempfile::tempdir().expect("global layer root");
        let state = project.path().join(".agent-vesper");
        std::fs::create_dir_all(&state).expect("create state dir");
        // PR-4 scope demand: the real config surface the hosts parse.
        std::fs::write(state.join("config.toml"), "[sandbox]\nfilesystem = true\n")
            .expect("write sandbox demand");
        Self { project, global }
    }

    fn root(&self) -> &Path {
        self.project.path()
    }

    fn state_dir(&self) -> PathBuf {
        self.root().join(".agent-vesper")
    }
}

/// Resolves the pure PR-5 scope for the fixture root (no process-global
/// holder: parallel tests must not contend; holder parity is pinned by
/// `scope_parity.rs`).
fn resolve_scope(fixture: &Fixture) -> WorkspaceScope {
    WorkspaceScope::resolve(&ScopeInputs {
        root: fixture.root().to_path_buf(),
        global_root: fixture.global.path().to_path_buf(),
        extra_roots: Vec::new(),
        project_cognition_override: None,
        global_cognition_override: None,
        stamp_policy: StampPolicy::Write,
    })
    .expect("fixture scope resolves")
}

/// The scope-composed firewall: global base ruleset ∪ one project rule,
/// deny-precedence (PR-5 composition over the PR-1/PR-2 ruleset). The
/// project rule denies a command class the base ruleset allows, proving
/// the composed pipeline enforces project-stricter policy.
fn composed_firewall() -> CommandFirewall {
    let project_rules: &[(&str, RuleDecision, &'static str)] = &[(
        r"\bdeploy-prod\b",
        RuleDecision::Deny,
        "project policy: deploy-prod is never agent-authorized",
    )];
    vesper_agent::vro::scope::compose_scope_firewall(
        Some(CommandFirewall::default_ruleset()),
        project_rules,
    )
    .expect("project rules compose")
    .expect("non-empty project rules yield a composed firewall")
}

/// The sandbox demand the fixture's `[sandbox] filesystem = true`
/// declares, parsed by the real `vesper-config` reader exactly the way
/// `sandbox_backend::holder` resolves it at host boot.
fn fixture_sandbox_demand(fixture: &Fixture) -> SandboxDemand {
    let config = vesper_config::read_sandbox_scope(fixture.root()).expect("sandbox config parses");
    assert!(
        config.is_active(),
        "fixture config must declare an active demand"
    );
    SandboxDemand {
        requirement: config.resolved_requirement(),
        allow_network: config.network,
        cpu_limit: config.cpu_limit.map(f64::from),
        memory_limit_bytes: config
            .memory_limit_mib
            .map(|mib| u64::from(mib) * 1024 * 1024),
    }
}

/// Honest sandbox port that records every routed command and answers with
/// a bounded sandbox-shaped outcome. Deterministic, daemon-free.
struct RecordingBackend {
    commands: Arc<Mutex<Vec<String>>>,
}

impl SandboxBackendPort for RecordingBackend {
    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            backend: "recording-stub (deterministic e2e)".to_owned(),
            process_tree: CapabilityStatus::Available,
            filesystem: CapabilityStatus::Available,
            network: CapabilityStatus::Unavailable,
            strength: SecurityStrength::Full,
        }
    }

    fn run_command(
        &self,
        command: &str,
        _cwd: &Path,
        timeout_seconds: u64,
        _cancellation: &Arc<dyn CancellationSignal>,
    ) -> Result<SandboxOutcome, SandboxRunError> {
        self.commands
            .lock()
            .expect("recorder lock")
            .push(command.to_owned());
        Ok(SandboxOutcome {
            output: format!("sandbox-exec[{timeout_seconds}s] {command}"),
            timed_out: false,
        })
    }
}

// ---------------------------------------------------------------------------
// scripted provider (no live provider calls — Project Contract)
// ---------------------------------------------------------------------------

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
    ) -> ProviderFuture<'a, Result<Self::Session, ProviderError>> {
        let session = self.session.clone();
        Box::pin(async move { Ok(session) })
    }
}

fn provider_id() -> ProviderId {
    ProviderId::new("test.e2e").expect("valid provider id")
}

fn configuration(provider_id: &ProviderId) -> ProviderConfiguration {
    ProviderConfiguration {
        provider_id: provider_id.clone(),
        values: VersionedExtensionEnvelope {
            namespace: ExtensionNamespace::new("provider.test").expect("valid namespace"),
            version: SchemaVersion::new(1).expect("valid schema version"),
            values: ExtensionMap::default(),
        },
    }
}

fn user_message(text: &str) -> vesper_domain::ConversationMessage {
    vesper_domain::ConversationMessage {
        id: MessageId::new("e2e-user-1").expect("valid message id"),
        role: MessageRole::User,
        content: vec![ContentPart::Text(
            ContentText::new(text).expect("bounded text"),
        )],
        extensions: ExtensionMap::default(),
    }
}

fn run_command_event(command: &str) -> ProviderStreamEvent {
    ProviderStreamEvent::ToolCallCompleted(ToolCall {
        id: ToolCallId::new("e2e-call-1").expect("valid tool call id"),
        tool_id: ToolId::new("run_command").expect("valid tool id"),
        arguments: json!({"command": command}),
        extensions: ExtensionMap::default(),
    })
}

fn completed_event(finish: FinishOutcome) -> ProviderStreamEvent {
    ProviderStreamEvent::Completed {
        finish,
        metadata: ExtensionMap::default(),
    }
}

/// A two-turn script: turn 1 calls `run_command <command>`, turn 2 stops
/// with a final assistant line.
fn scripted_shell_turn(command: &str, final_line: &str) -> FakeProviderSession {
    FakeProviderSession::with_scripts([
        Ok(vec![
            Ok(run_command_event(command)),
            Ok(completed_event(FinishOutcome::ToolCalls)),
        ]),
        Ok(vec![
            Ok(ProviderStreamEvent::ContentDelta {
                stream_id: BoundedString::new("content").expect("bounded stream id"),
                part: ContentPart::Text(ContentText::new(final_line).expect("bounded text")),
            }),
            Ok(completed_event(FinishOutcome::Stop)),
        ]),
    ])
}

fn loop_config(
    fixture: &Fixture,
    firewall: Option<Arc<CommandFirewall>>,
    sandbox: Option<Arc<SandboxRoute>>,
) -> AgentLoopConfig {
    let id = provider_id();
    AgentLoopConfig {
        provider_id: id.clone(),
        provider_configuration: configuration(&id),
        model: QualifiedModelId {
            provider_id: id,
            model_id: ModelId::new("fixture-model").expect("valid model id"),
        },
        system_instructions: Vec::new(),
        workspace_roots: vec![vesper_domain::WorkspaceRoot {
            name: BoundedString::new("workspace").expect("bounded root name"),
            path: BoundedString::new(fixture.root().display().to_string())
                .expect("bounded root path"),
            primary: true,
        }],
        max_tool_iterations: 10,
        firewall,
        sandbox,
    }
}

/// Every observation text a turn accumulated (tool results only; a
/// denied/failed call surfaces as the `tool error: …` observation the
/// loop feeds back to the model).
fn observations(outcome: &AgentTurnOutcome) -> String {
    match outcome {
        AgentTurnOutcome::Completed { tool_results, .. } => tool_results
            .iter()
            .map(|result| result.text.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Runs one bounded fire turn inside an ambient tokio runtime and records
/// the outcome in the scope-keyed cron slot ledger, mirroring
/// `run_cron_scheduler`'s per-fire discipline (claim → turn → outcome).
#[allow(clippy::too_many_arguments)]
async fn fire_once(
    fixture: &Fixture,
    cron: &vesper_checkpoints::CronRegistry,
    job_id: &str,
    slot: u64,
    now: SystemTime,
    session: FakeProviderSession,
    permission: SessionPermissionMode,
    firewall: Option<Arc<CommandFirewall>>,
    sandbox: Option<Arc<SandboxRoute>>,
) -> String {
    if !cron
        .claim_slot(job_id, slot, now)
        .expect("slot claim is durable")
    {
        return "silent: slot owned by a coexisting scheduler".to_owned();
    }
    let registry = Arc::new(ProviderRegistry::new());
    registry
        .register(FakeFactory {
            id: provider_id(),
            session,
        })
        .await
        .expect("fake provider registers");
    let outcome = AgentLoop::new(
        registry,
        ToolRegistry::parity_default(),
        loop_config(fixture, firewall, sandbox),
    )
    .run_prompt(
        user_message("run the scheduled pipeline step"),
        SessionOperatingMode::Code,
        permission,
    )
    .await
    .expect("fire turn reaches a terminal outcome");
    let text = observations(&outcome);
    let status = if text.contains("tool error:") {
        "error"
    } else {
        "ok"
    };
    cron.mark_fired(job_id, slot, status, &text, now)
        .expect("fire outcome is recorded");
    text
}

fn slot_ordinal(now: SystemTime) -> u64 {
    now.duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Stage tests: the full pipeline, one capability per case
// ---------------------------------------------------------------------------

/// Full pipeline, deny arm: a watcher fire whose bounded turn attempts a
/// destructive shell command under Bypass is denied by the composed
/// firewall — the base deny rules survive the project composition, the
/// observation is the model-facing `tool error: [VRO-13 Firewall]
/// denied:` text, the sweep records the fire, and nothing executed.
#[test]
fn pipeline_denies_destructive_command_from_a_watcher_fire() {
    let fixture = Fixture::new();
    let scope = resolve_scope(&fixture);
    let scope_id = scope.id().as_str().to_owned();
    let state = fixture.state_dir();

    // PR-7 trigger: one watcher on a tail file, bound to the scope.
    let trigger = fixture.root().join("pipeline.log");
    std::fs::write(&trigger, "step 1 ok\nTRIGGER pipeline due\n").expect("write trigger");
    let store = vesper_checkpoints::WatcherStore::open(&state).expect("watcher store opens");
    store
        .register(
            &scope_id,
            &trigger.display().to_string(),
            vesper_checkpoints::WatcherTargetKind::Path,
            "TRIGGER",
            None,
        )
        .expect("watcher registers");

    // PR-6 exactly-once seam: the job this fire executes on behalf of.
    let cron = vesper_checkpoints::CronRegistry::open(&state).expect("cron registry opens");
    let job = cron
        .register(
            "e2e-pipeline",
            "run the scheduled pipeline step",
            "every 1h",
        )
        .expect("job registers");

    let firewall = Arc::new(composed_firewall());
    let now = SystemTime::now();
    let slot = slot_ordinal(now);

    let outcome = run_sweep_once(&state, &scope_id, now, |entry| {
        assert_eq!(entry.scope_id, scope_id, "the watcher is scope-keyed");
        let session = scripted_shell_turn("rm -rf /", "pipeline halted by policy");
        let text = tokio::runtime::Runtime::new()
            .expect("fire runtime")
            .block_on(fire_once(
                &fixture,
                &cron,
                &job.id,
                slot,
                now,
                session,
                SessionPermissionMode::Bypass,
                Some(Arc::clone(&firewall)),
                None,
            ));
        Ok(text)
    });

    // The sweep fired exactly the one matching watcher.
    assert_eq!(outcome.fired, vec!["watch-1"], "the trigger must fire");

    // The deny surfaced as the exact model-facing observation (PR-2) even
    // though the fire ran under Bypass — deny outranks every mode.
    let entry = cron.get(&job.id).expect("job survives the fire");
    let output = entry.last_output.as_deref().unwrap_or_default();
    assert!(
        output.contains("[VRO-13 Firewall] denied:"),
        "the recorded outcome must carry the firewall denial: {output}"
    );
    assert_eq!(entry.last_status.as_deref(), Some("error"));

    // Nothing executed: no sandbox was demanded on this arm and the
    // destructive command never reached a shell.
    assert!(!fixture.root().join("danger.txt").exists());
    // The slot ledger carries the claim + outcome rows (audit trail).
    let rows = cron.list_slot_records();
    assert!(
        rows.iter()
            .any(|row| row.job == job.id && row.status.is_some()),
        "the fire outcome must land in cron-slots.jsonl"
    );
}

/// Full pipeline, allowed arm: the same watcher fire under the fixture's
/// active `[sandbox]` demand routes a benign command through the
/// fail-closed sandbox gate into the sandboxed port exactly once, and the
/// outcome is recorded in the scope-keyed ledgers.
#[test]
fn pipeline_sandboxes_allowed_commands_and_logs_scope_keyed_transcript() {
    let fixture = Fixture::new();
    let scope = resolve_scope(&fixture);
    let scope_id = scope.id().as_str().to_owned();
    let state = fixture.state_dir();

    // The PR-4 demand parses from the fixture config and is active.
    let demand = fixture_sandbox_demand(&fixture);
    assert_eq!(demand.requirement, IsolationRequirement::Filesystem);
    assert!(demand.is_active());

    let recorder = Arc::new(RecordingBackend {
        commands: Arc::new(Mutex::new(Vec::new())),
    });
    let route = Arc::new(SandboxRoute::new(
        demand,
        SandboxBackendChoice::Docker,
        recorder.clone(),
    ));
    assert!(
        route.satisfies_demand(),
        "the recording backend satisfies the Filesystem demand"
    );

    let trigger = fixture.root().join("build.log");
    std::fs::write(&trigger, "compiling\nTRIGGER nightly build\n").expect("write trigger");
    let store = vesper_checkpoints::WatcherStore::open(&state).expect("watcher store opens");
    store
        .register(
            &scope_id,
            &trigger.display().to_string(),
            vesper_checkpoints::WatcherTargetKind::Path,
            "TRIGGER",
            None,
        )
        .expect("watcher registers");

    let cron = vesper_checkpoints::CronRegistry::open(&state).expect("cron registry opens");
    let job = cron
        .register("e2e-build", "run the nightly build", "every 1h")
        .expect("job registers");

    let firewall = Arc::new(composed_firewall());
    let now = SystemTime::now();
    let slot = slot_ordinal(now);

    let first = run_sweep_once(&state, &scope_id, now, |entry| {
        assert_eq!(entry.scope_id, scope_id, "the watcher is scope-keyed");
        let session = scripted_shell_turn("echo build-ok", "nightly build finished");
        let text = tokio::runtime::Runtime::new()
            .expect("fire runtime")
            .block_on(fire_once(
                &fixture,
                &cron,
                &job.id,
                slot,
                now,
                session,
                SessionPermissionMode::Bypass,
                Some(Arc::clone(&firewall)),
                Some(Arc::clone(&route)),
            ));
        Ok(text)
    });

    assert_eq!(first.fired, vec!["watch-1"], "the trigger fires once");

    // The command was allowed by the firewall and executed INSIDE the
    // sandbox route — exactly once, never unsandboxed.
    let commands = recorder.commands.lock().expect("recorder lock").clone();
    assert_eq!(
        commands,
        vec!["echo build-ok".to_owned()],
        "the allowed command must execute exactly once through the sandbox port"
    );

    // Scope-keyed transcript: the bounded outcome is retained on the job
    // (the transcript surface the harness surfaces) and the sweep event
    // names the scope.
    let entry = cron.get(&job.id).expect("job survives the fire");
    let output = entry.last_output.as_deref().unwrap_or_default();
    assert!(
        output.contains("sandbox-exec["),
        "the recorded outcome must carry the sandboxed execution: {output}"
    );
    assert_eq!(entry.last_status.as_deref(), Some("ok"));
    let events = vesper_harness::watcher_sweep::list_sweep_events(&state, 10);
    let event = events
        .iter()
        .find(|event| event.action == "fired")
        .expect("the sweep records the fire");
    assert_eq!(event.scope, scope_id, "the event ledger is scope-keyed");

    // Rate limit: an immediate re-sweep queues the still-matching watcher
    // instead of double-firing (PR-7 discipline), and nothing re-executes.
    let second = run_sweep_once(&state, &scope_id, now + Duration::from_secs(30), |_| {
        panic!("the rate window must suppress a second fire")
    });
    assert_eq!(second.fired, Vec::<String>::new());
    assert_eq!(second.queued, vec!["watch-1"]);
    assert_eq!(
        recorder.commands.lock().expect("recorder lock").len(),
        1,
        "no second execution inside the rate window"
    );
}

/// The project rule composed into the scope firewall denies its own
/// command class — the pipeline enforces project-stricter policy, not
/// just the global base.
#[tokio::test]
async fn pipeline_enforces_composed_project_rules() {
    let fixture = Fixture::new();
    let registry = Arc::new(ProviderRegistry::new());
    registry
        .register(FakeFactory {
            id: provider_id(),
            session: scripted_shell_turn("make deploy-prod", "deploy blocked"),
        })
        .await
        .expect("fake provider registers");
    let outcome = AgentLoop::new(
        registry,
        ToolRegistry::parity_default(),
        loop_config(&fixture, Some(Arc::new(composed_firewall())), None),
    )
    .run_prompt(
        user_message("deploy"),
        SessionOperatingMode::Code,
        SessionPermissionMode::Bypass,
    )
    .await
    .expect("project-rule arm reaches a terminal outcome");
    let text = observations(&outcome);
    assert!(
        text.contains("[VRO-13 Firewall] denied:"),
        "the composed project rule must deny: {text}"
    );
    assert!(
        text.contains("deploy-prod is never agent-authorized"),
        "the denial must carry the project rule's reason: {text}"
    );
}

/// Unattended `Ask` fires gain no free authority: with the fail-closed
/// `DenyPermissionPort` (the default port, the daemon's unattended shape)
/// a shell command is denied at the permission gate before any firewall
/// scan or execution — the PRD §4.2 safety shape.
#[tokio::test]
async fn unattended_ask_fire_fails_closed_before_execution() {
    let fixture = Fixture::new();
    let recorder = Arc::new(RecordingBackend {
        commands: Arc::new(Mutex::new(Vec::new())),
    });
    let demand = fixture_sandbox_demand(&fixture);
    let route = Arc::new(SandboxRoute::new(
        demand,
        SandboxBackendChoice::Docker,
        recorder.clone(),
    ));
    let registry = Arc::new(ProviderRegistry::new());
    registry
        .register(FakeFactory {
            id: provider_id(),
            session: scripted_shell_turn("echo unattended", "unattended turn done"),
        })
        .await
        .expect("fake provider registers");
    let outcome = AgentLoop::new(
        registry,
        ToolRegistry::parity_default(),
        loop_config(&fixture, Some(Arc::new(composed_firewall())), Some(route)),
    )
    .run_prompt(
        user_message("unattended step"),
        SessionOperatingMode::Code,
        SessionPermissionMode::Ask,
    )
    .await
    .expect("unattended arm reaches a terminal outcome");
    let text = observations(&outcome);
    assert!(
        text.contains("approval"),
        "an unattended Ask fire must fail closed at the approval gate: {text}"
    );
    assert!(
        !text.contains("sandbox-exec["),
        "nothing may execute inside the sandbox on a denied fire: {text}"
    );
    assert!(
        recorder.commands.lock().expect("recorder lock").is_empty(),
        "the sandbox port must never be reached"
    );
}

/// Fail-closed sandbox routing: when the resolved backend cannot satisfy
/// the scope's demand (an all-Unavailable backend — what a feature-off
/// build resolves for a Docker demand), the executor refuses with the
/// model-facing `sandbox unavailable` text instead of running unsandboxed.
#[tokio::test]
async fn unsatisfiable_docker_demand_refuses_instead_of_running_unsandboxed() {
    let fixture = Fixture::new();
    let demand = fixture_sandbox_demand(&fixture);
    let port: Arc<dyn SandboxBackendPort> = Arc::new(BackendPort::new(
        Arc::new(vesper_sandbox::UnavailableBackend),
        demand.clone(),
    ));
    let route = Arc::new(SandboxRoute::new(
        demand,
        SandboxBackendChoice::Docker,
        port,
    ));
    assert!(
        !route.satisfies_demand(),
        "an all-Unavailable backend must fail the Filesystem demand"
    );

    let registry = Arc::new(ProviderRegistry::new());
    registry
        .register(FakeFactory {
            id: provider_id(),
            session: scripted_shell_turn("echo needs-isolation", "refused"),
        })
        .await
        .expect("fake provider registers");
    let outcome = AgentLoop::new(
        registry,
        ToolRegistry::parity_default(),
        loop_config(&fixture, Some(Arc::new(composed_firewall())), Some(route)),
    )
    .run_prompt(
        user_message("step that needs isolation"),
        SessionOperatingMode::Code,
        SessionPermissionMode::Bypass,
    )
    .await
    .expect("fail-closed arm reaches a terminal outcome");
    let text = observations(&outcome);
    assert!(
        text.contains("sandbox unavailable"),
        "the refusal must be model-facing and honest: {text}"
    );
    assert!(
        text.contains("refusing to run unsandboxed"),
        "the refusal must state the fail-closed shape: {text}"
    );
    assert!(!fixture.root().join("needs-isolation.txt").exists());
}

/// Real-DockerBackend arm, feature-gated and daemon-free: with the docker
/// feature enabled but the daemon unreachable (a binary that cannot
/// exist), the cold-start guard refuses before any `docker run` — the
/// same honest-refusal contract `vesper-sandbox/tests/docker.rs` pins at
/// the backend layer, here through the composed executor path.
#[cfg(feature = "docker")]
#[tokio::test]
async fn docker_feature_cold_start_guard_refuses_through_the_composed_path() {
    let fixture = Fixture::new();
    let demand = fixture_sandbox_demand(&fixture);
    let backend = vesper_sandbox::DockerBackend::new(vesper_sandbox::DockerSandboxConfig {
        docker_bin: Some(PathBuf::from("/nonexistent/vesper-docker-stub")),
        ..vesper_sandbox::DockerSandboxConfig::default()
    });
    let port: Arc<dyn SandboxBackendPort> =
        Arc::new(BackendPort::new(Arc::new(backend), demand.clone()));
    let route = Arc::new(SandboxRoute::new(
        demand,
        SandboxBackendChoice::Docker,
        port,
    ));
    assert!(!route.satisfies_demand(), "the probe must fail closed");

    let registry = Arc::new(ProviderRegistry::new());
    registry
        .register(FakeFactory {
            id: provider_id(),
            session: scripted_shell_turn("echo cold-start", "refused"),
        })
        .await
        .expect("fake provider registers");
    let outcome = AgentLoop::new(
        registry,
        ToolRegistry::parity_default(),
        loop_config(&fixture, Some(Arc::new(composed_firewall())), Some(route)),
    )
    .run_prompt(
        user_message("cold-start step"),
        SessionOperatingMode::Code,
        SessionPermissionMode::Bypass,
    )
    .await
    .expect("cold-start arm reaches a terminal outcome");
    let text = observations(&outcome);
    assert!(
        text.contains("sandbox unavailable"),
        "cold-start guard must surface through the executor: {text}"
    );
}
