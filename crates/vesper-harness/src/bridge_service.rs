//! VB-PRD-001 Phase 2: the hosted Bridge tool service.
//!
//! Composition boundary for [`vesper_bridge`]. The pure core owns every
//! decision; this module only (a) advertises the minimal tool surface,
//! (b) routes calls into the core's `authorize` gate, and (c) maps core
//! denials to model-visible text. It performs **no I/O**: there is no
//! driver, transport, capture or application adapter in Phase 2 — those
//! land behind the `vesper_bridge` ports in later phases, each still
//! passing through the same gate.
//!
//! Default-off: hosts construct a [`BridgeToolService`] only when Bridge
//! is explicitly enabled (`with_bridge`). Without it, zero bridge tools
//! are advertised and no bridge state exists (BR-30, NF-01, AT-01).

use vesper_agent::{ToolError, ToolResult, schema_definition};
use vesper_bridge::capability::{
    Availability, CapabilityId, CapabilityManifest, CapabilityRecord, DeliveryMode, Implementation,
    Mutability, RouteKind, VerificationMethod,
};
use vesper_bridge::identity::{ApplicationInstanceId, BridgeSessionId, Generation};
use vesper_bridge::journal::MemoryJournal;
use vesper_bridge::lease::{LeaseState, ResourceKey};
use vesper_bridge::observation::{Observation, ObservationId, ObservationKind};
use vesper_bridge::operation::OperationSpec;
use vesper_bridge::session::{BridgeSession, DenialReason};
use vesper_domain::{ToolCall, ToolDefinition, ToolExecutionClass};

/// The minimal model-facing surface (PRD §6.3). These are Vesper tools,
/// not MCP/Cua/Resolve names.
pub const BRIDGE_TOOL_DISCOVER: &str = "bridge_discover";
pub const BRIDGE_TOOL_CONNECT: &str = "bridge_connect";
pub const BRIDGE_TOOL_CAPABILITIES: &str = "bridge_capabilities";
pub const BRIDGE_TOOL_OBSERVE: &str = "bridge_observe";
pub const BRIDGE_TOOL_EXECUTE: &str = "bridge_execute";
pub const BRIDGE_TOOL_JOB_STATUS: &str = "bridge_job_status";
pub const BRIDGE_TOOL_VERIFY: &str = "bridge_verify";
pub const BRIDGE_TOOL_DISCONNECT: &str = "bridge_disconnect";

pub const BRIDGE_TOOL_NAMES: [&str; 8] = [
    BRIDGE_TOOL_DISCOVER,
    BRIDGE_TOOL_CONNECT,
    BRIDGE_TOOL_CAPABILITIES,
    BRIDGE_TOOL_OBSERVE,
    BRIDGE_TOOL_EXECUTE,
    BRIDGE_TOOL_JOB_STATUS,
    BRIDGE_TOOL_VERIFY,
    BRIDGE_TOOL_DISCONNECT,
];

/// L1: the ONE truthful no-adapter discovery answer, shared by the tool
/// surface and the `/bridge` command so the strings cannot drift.
pub const BRIDGE_NO_ADAPTER_DISCOVERY: &str = "Bridge discovery (host): no adapters are configured in this build. Nothing was installed, launched or altered.";

/// Definitions advertised when Bridge is enabled. Read-only tools are
/// classed ReadOnly; `bridge_execute` is Mutating because compound tools
/// are classified per selected operation at the core gate (§8).
#[must_use]
pub fn bridge_definitions() -> Vec<ToolDefinition> {
    vec![
        schema_definition(
            BRIDGE_TOOL_DISCOVER,
            "Enumerate configured Bridge adapters and discover local application candidates. Passive: never installs, launches or alters the target.",
            ToolExecutionClass::ReadOnly,
            &[],
        ),
        schema_definition(
            BRIDGE_TOOL_CONNECT,
            "Bind a Bridge session to one exact application instance. Ambiguous matches are refused.",
            ToolExecutionClass::Mutating,
            &[("application", "string", true)],
        ),
        schema_definition(
            BRIDGE_TOOL_CAPABILITIES,
            "List the versioned capability manifest of the connected session: availability, implementation status, mutability and limitations. Unknown is not supported.",
            ToolExecutionClass::ReadOnly,
            &[],
        ),
        schema_definition(
            BRIDGE_TOOL_OBSERVE,
            "Take a bounded observation (semantic state first; images only when permitted and the model accepts them).",
            ToolExecutionClass::ReadOnly,
            &[("include_images", "boolean", false)],
        ),
        schema_definition(
            BRIDGE_TOOL_EXECUTE,
            "Execute one typed, validated operation chosen from the capability manifest. The host supplies authoritative identity, lease and approval; denials outrank fallback.",
            ToolExecutionClass::Mutating,
            &[
                ("capability", "string", true),
                ("arguments", "object", true),
            ],
        ),
        schema_definition(
            BRIDGE_TOOL_JOB_STATUS,
            "Report settlement status of asynchronous jobs (renders). A submitted job is not a finished artifact.",
            ToolExecutionClass::ReadOnly,
            &[],
        ),
        schema_definition(
            BRIDGE_TOOL_VERIFY,
            "Evaluate a postcondition independently of the executor's acknowledgment and record the evidence.",
            ToolExecutionClass::ReadOnly,
            &[("request_id", "string", true)],
        ),
        schema_definition(
            BRIDGE_TOOL_DISCONNECT,
            "Close the Bridge session. Never kills a user-owned application or unrelated processes.",
            ToolExecutionClass::Mutating,
            &[],
        ),
    ]
}

/// The hosted Bridge state (single owner per session, §8.1).
pub struct BridgeToolService {
    session: tokio::sync::Mutex<Option<BridgeSession>>,
    journal: MemoryJournal,
    manifest: CapabilityManifest,
    /// Observation revision counter (host-owned monotonic freshness).
    observation_revision: std::sync::atomic::AtomicU64,
    /// The most recent observation handed to the model; execute binds
    /// preconditions to THIS one so stop/resume staleness is real.
    last_observation: std::sync::Mutex<Option<Observation>>,
    /// The live application adapter (None = no-adapter composition).
    adapter: Option<std::sync::Arc<dyn vesper_bridge::AdapterPort>>,
}

impl BridgeToolService {
    /// M1 (NF-05): argument payloads are bounded at the service boundary.
    pub const MAX_ARGUMENT_BYTES: usize = 64 * 1024;

    /// A conservative default manifest: no application adapter is attached
    /// in Phase 2, so every operation reports `dependency_missing` with
    /// `unknown` implementation. This is the truthful no-adapter state —
    /// it must never be presented as application support.
    #[must_use]
    pub fn no_adapter() -> Self {
        Self {
            session: tokio::sync::Mutex::new(None),
            journal: MemoryJournal::new(),
            manifest: CapabilityManifest::new(
                "no-adapter",
                1,
                vec![CapabilityRecord {
                    id: CapabilityId::new("bridge.placeholder.none").unwrap(),
                    schema_version: 1,
                    availability: Availability::DependencyMissing,
                    implementation: Implementation::Unknown,
                    mutability: Mutability::ReadOnly,
                    route: RouteKind::NativeApi,
                    delivery: DeliveryMode::Background,
                    verification: VerificationMethod::None,
                    limitations: "No application adapter is attached in this build; Bridge is enabled but has nothing to control.".into(),
                }],
            ),
            observation_revision: std::sync::atomic::AtomicU64::new(0),
            last_observation: std::sync::Mutex::new(None),
            adapter: None,
        }
    }

    /// A service bound to an explicit manifest (adapter composition entry
    /// point; tests use it to prove mode/denial routing with mutating
    /// capabilities without any application existing).
    #[must_use]
    pub fn with_manifest(manifest: CapabilityManifest) -> Self {
        Self {
            session: tokio::sync::Mutex::new(None),
            journal: MemoryJournal::new(),
            manifest,
            observation_revision: std::sync::atomic::AtomicU64::new(0),
            last_observation: std::sync::Mutex::new(None),
            adapter: None,
        }
    }

    /// Attach a live application adapter. Takes effect on the next
    /// `bridge_connect` (a connected session keeps its binding until
    /// disconnect — §6 authority generation semantics).
    #[must_use]
    pub fn with_adapter(mut self, adapter: std::sync::Arc<dyn vesper_bridge::AdapterPort>) -> Self {
        self.manifest = adapter.manifest().clone();
        self.adapter = Some(adapter);
        self
    }

    /// The adapter in effect for new connections (reporting surface).
    pub fn adapter_description(&self) -> Option<String> {
        self.adapter.as_ref().map(|a| a.describe())
    }

    fn next_observation(&self) -> Observation {
        // Each call mints a fresh revision — used by bridge_observe. The
        // execute path replays the LAST observation (preconditions must
        // bind to what was actually observed, not a synthetic fresh one).
        let observation = self.fresh_observation();
        self.last_observation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .replace(observation.clone());
        observation
    }

    /// Replay the most recent observation for precondition binding.
    fn replay_observation(&self) -> Observation {
        // Preconditions must bind to what was actually observed by a prior
        // `bridge_observe` call — never a synthetic fresh one minted here.
        // With no stored observation we return an explicitly degraded,
        // zero-revision observation so `authorize` refuses it at the
        // freshness gate (BR-15) while preserving denial precedence:
        // mode/permission and capability diagnostics still come first.
        self.last_observation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .unwrap_or_else(|| Observation {
                id: ObservationId("unbound".into()),
                kind: ObservationKind::Semantic,
                revision: 0,
                captured_at_ms: 0,
                semantic: serde_json::json!({
                    "adapter": "none",
                    "state": "no observation bound; call bridge_observe first"
                }),
                images: vec![],
                transforms: vec![],
                bounds: vec![],
                degraded_capture: true,
            })
    }

    fn fresh_observation(&self) -> Observation {
        let revision = self
            .observation_revision
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        Observation {
            id: ObservationId(format!("obs-{revision}")),
            kind: ObservationKind::Semantic,
            revision,
            captured_at_ms: 0,
            // Truthful empty state: no adapter means no semantic app state.
            semantic: serde_json::json!({"adapter": "none", "state": "unavailable"}),
            images: vec![],
            transforms: vec![],
            bounds: vec![],
            degraded_capture: false,
        }
    }

    async fn execute_call(
        &self,
        call: &ToolCall,
        context: &vesper_agent::ToolContext,
    ) -> Result<ToolResult, vesper_agent::ToolError> {
        use vesper_agent::ToolError;
        let mut guard = self.session.lock().await;
        match call.tool_id.as_str() {
            BRIDGE_TOOL_DISCOVER => {
                // Real discovery: enumerate live MPRIS players plus the
                // configured adapter (if any). Still passive — nothing is
                // installed, launched or altered.
                let mut parts = vec![String::from("Bridge discovery (host):")];
                if let Some(adapter) = &self.adapter {
                    let health = adapter.health().map_or("unknown".to_string(), |h| h.to_string());
                    parts.push(format!("configured adapter: {} (live={})", adapter.describe(), health));
                } else {
                    parts.push("no adapter configured in this build".into());
                }
                let players = crate::bridge_adapters::discover_mpris_players();
                if players.is_empty() {
                    parts.push("no live MPRIS players on the session bus".into());
                } else {
                    parts.push(format!("live MPRIS players: {}", players.join(", ")));
                }
                tool_result(parts.join(" | "))
            }
            BRIDGE_TOOL_CONNECT => {
                if guard.is_some() {
                    return Err(ToolError::Failed(
                        "A Bridge session is already active; disconnect first.".into(),
                    ));
                }
                let mut session = BridgeSession::new(
                    BridgeSessionId::new("bridge-primary").map_err(ToolError::Failed)?,
                    ApplicationInstanceId::new("none/seat-0", Generation(1))
                        .map_err(ToolError::Failed)?,
                    self.manifest.clone(),
                );
                session.connect().map_err(|e| ToolError::Failed(e.to_string()))?;
                session.mark_ready();
                *guard = Some(session);
                let binding = match &self.adapter {
                    Some(adapter) => {
                        let health = adapter.health();
                        if health == Some(false) {
                            return Err(ToolError::Failed(format!(
                                "Bridge adapter {} is not live; start the application/worker before connecting.",
                                adapter.describe()
                            )));
                        }
                        format!(
                            "Bridge session bound to {} ({} capabilities). {}",
                            adapter.describe(),
                            self.manifest.records.len(),
                            "Operations dispatch through the adapter after authorization."
                        )
                    }
                    None => "Bridge session bound to the no-adapter composition. No application is attached; every operation reports capability_unavailable truthfully.".into(),
                };
                tool_result(binding)
            }
            BRIDGE_TOOL_CAPABILITIES => {
                let Some(session) = guard.as_ref() else {
                    return Err(ToolError::Failed("No Bridge session is connected.".into()));
                };
                let _ = session;
                let records: Vec<&CapabilityRecord> = self.manifest.records.iter().collect();
                let text = serde_json::to_string_pretty(&records)
                    .map_err(|_| ToolError::Failed("capability serialization failed".into()))?;
                tool_result(text)
            }
            BRIDGE_TOOL_OBSERVE => {
                let Some(_session) = guard.as_mut() else {
                    return Err(ToolError::Failed("No Bridge session is connected.".into()));
                };
                let observation = self.next_observation();
                // M4 (NF-07): the budget is a GATE here, not a library
                // call nothing makes. Over-budget or degraded captures
                // are refused at admission — they may not ground actions.
                if !observation.within_pixel_budget() {
                    return Err(ToolError::Failed(
                        "Observation exceeds the 16,777,216 decoded-pixel budget (NF-07); refusing to admit it.".into(),
                    ));
                }
                if observation.degraded_capture {
                    return Err(ToolError::Failed(
                        "Observation capture is degraded (black/protected/stale); it cannot ground actions — retry the observation (AT-14).".into(),
                    ));
                }
                // H6: the model sees the observation IDENTITY it must bind
                // plans to (§6 precondition loop): id, revision, capture
                // time — not just the semantic blob.
                let identity = serde_json::json!({
                    "id": observation.id.0,
                    "revision": observation.revision,
                    "captured_at_ms": observation.captured_at_ms,
                    "kind": if observation.kind == ObservationKind::SemanticWithImages { "semantic_with_images" } else { "semantic" },
                    "degraded_capture": observation.degraded_capture,
                });
                let mut combined = serde_json::Map::new();
                if let serde_json::Value::Object(semantic) = observation.semantic {
                    combined.extend(semantic);
                }
                combined.insert("observation".into(), identity);
                let text = serde_json::to_string(&serde_json::Value::Object(combined))
                    .map_err(|_| ToolError::Failed("observation serialization failed".into()))?;
                tool_result(text)
            }
            BRIDGE_TOOL_EXECUTE => {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Args {
                    capability: String,
                    arguments: serde_json::Value,
                }
                let args: Args = serde_json::from_value(call.arguments.clone())
                    .map_err(|_| ToolError::Failed("bridge_execute requires capability and arguments".into()))?;
                // M1 (NF-05): bound the argument payload BEFORE it enters
                // the core or the journal — an unbounded `arguments` value
                // would be retained 512 times over by the bounded record
                // window.
                let serialized = serde_json::to_string(&args.arguments)
                    .map_err(|_| ToolError::Failed("arguments are not serializable".into()))?;
                if serialized.len() > Self::MAX_ARGUMENT_BYTES {
                    return Err(ToolError::Failed(format!(
                        "bridge_execute arguments exceed the {} KiB bound (got {} bytes); split the work into smaller operations",
                        Self::MAX_ARGUMENT_BYTES / 1024,
                        serialized.len()
                    )));
                }
                let capability = CapabilityId::new(&args.capability)
                    .map_err(ToolError::Failed)?;
                // M2: the VALIDATED spec carries the caller's arguments —
                // never silently swap them for an empty object; the
                // denial the model receives must reflect what it sent.
                let spec = OperationSpec { capability, arguments: args.arguments };
                let Some(session) = guard.as_mut() else {
                    return Err(ToolError::Failed("No Bridge session is connected.".into()));
                };
                let observation = self.replay_observation();
                // Real dispatch shape: the lease carries the CURRENT fence
                // for the target resource (a zero fence would be refused
                // as stale by the core after any prior grant).
                let lease = LeaseState {
                    id: vesper_bridge::lease::LeaseId(format!(
                        "bridge-{}",
                        observation.revision
                    )),
                    resource: ResourceKey::Document("none".into()),
                    fence: session.current_fence_for_tests(&ResourceKey::Document("none".into())),
                    remaining_ms: 60_000,
                    holds_input: false,
                    owner: None,
                };
                match session.authorize(
                    &self.journal,
                    &spec,
                    matches!(context.operating_mode, vesper_domain::SessionOperatingMode::Plan),
                    matches!(context.permission_mode, vesper_domain::SessionPermissionMode::ReadOnly),
                    Generation(1),
                    &observation,
                    lease,
                    session.authority_epoch(),
                    0,
                ) {
                    Ok(request) => {
                        // C2 wiring: revalidate IMMEDIATELY before the
                        // adapter handoff (§6). Refuse, never dispatch,
                        // when it fails.
                        let _ = &request;
                        let granted = session
                            .granted_lease_for_tests()
                            .ok_or_else(|| ToolError::Failed(
                                "bridge_execute authorization did not retain the lease for revalidation".into(),
                            ))?;
                        if let Err(denial) = session.revalidate(&granted, Generation(1), 0) {
                            return denial_text(denial);
                        }
                        // Adapter dispatch: authorized + revalidated. With
                        // no adapter attached this remains the honest
                        // refusal — never a fabricated effect.
                        let Some(adapter) = self.adapter.clone() else {
                            return Err(ToolError::Failed(
                                "bridge_execute revalidated clean, but no adapter exists in this build; refusing rather than fabricating an application effect".into(),
                            ));
                        };
                        // Serialize the outcome through the session core so
                        // the record/journal/duplicate-suppression path is
                        // the ONLY way effects settle.
                        let request_id = request.request_id.clone();
                        let outcome = adapter.dispatch(&request_id, &spec);
                        drop(guard);
                        // Settle the adapter result through the core so
                        // BR-16 duplicate suppression sees it.
                        let mut guard2 = self.session.lock().await;
                        let Some(session) = guard2.as_mut() else {
                            return Err(ToolError::Failed("No Bridge session is connected.".into()));
                        };
                        let settled: Result<(), String> = match &outcome {
                            Ok(adapter_outcome) => {
                                let summary = adapter_outcome.summary.clone();
                                match &adapter_outcome.outcome {
                                    vesper_bridge::operation::OperationOutcome::Failed(reason) => {
                                        let reason = reason.clone();
                                        session.settle(&request_id, vesper_bridge::operation::OperationOutcome::Failed(format!("{summary} | {reason}")));
                                        Ok(())
                                    }
                                    other => {
                                        session.settle(&request_id, other.clone());
                                        Ok(())
                                    }
                                }
                            }
                            Err(e) => {
                                // §7 hardening: a timeout (or any transport
                                // loss) AFTER dispatch may have committed —
                                // the honest classification is
                                // UnknownOutcome, never Failed, because
                                // Failed invites a blind retry that could
                                // double-apply a mutation. Other errors
                                // that occur BEFORE any bytes reach the
                                // application (invalid arguments, missing
                                // dependency) remain honest failures.
                                let outcome = match e {
                                    vesper_bridge::BridgeError::Timeout { .. }
                                    | vesper_bridge::BridgeError::Transport { .. } => {
                                        vesper_bridge::operation::OperationOutcome::UnknownOutcome
                                    }
                                    err => vesper_bridge::operation::OperationOutcome::Failed(
                                        format!("adapter error: {err}"),
                                    ),
                                };
                                session.settle(&request_id, outcome);
                                Ok(())
                            }
                        };
                        match (outcome, settled) {
                            (Ok(adapter_outcome), Ok(())) => {
                                let state = match adapter_outcome.outcome {
                                    vesper_bridge::operation::OperationOutcome::Verified => "verified",
                                    vesper_bridge::operation::OperationOutcome::Failed(_) => "failed",
                                    _ => "applied",
                                };
                                let evidence_line = if adapter_outcome.evidence.is_empty() {
                                    "no independent verification evidence".to_string()
                                } else {
                                    format!("evidence: {}", adapter_outcome.evidence)
                                };
                                let mut text = format!(
                                    "Bridge operation {}: {}\nsummary: {}",
                                    state, evidence_line, adapter_outcome.summary
                                );
                                if adapter_outcome.job_created {
                                    text.push_str("\nasync job created: poll bridge_job_status");
                                }
                                tool_result(text)
                            }
                            (Ok(_), Err(e)) => Err(ToolError::Failed(format!(
                                "adapter succeeded but settlement failed: {e}"
                            ))),
                            (Err(e), _) => Err(ToolError::Failed(format!(
                                "bridge dispatch uncertain: {e}; the operation may or may not have been applied — reconcile the application state before retrying (unknown_outcome)"
                            ))),
                        }
                    }
                    Err(denial) => denial_text(denial),
                }
            }
            BRIDGE_TOOL_JOB_STATUS => tool_result(
                "No asynchronous Bridge jobs exist (no adapter attached).",
            ),
            BRIDGE_TOOL_VERIFY => Err(ToolError::Failed(
                "bridge_verify requires an applied operation with independent evidence; none exist in this composition.".into(),
            )),
            BRIDGE_TOOL_DISCONNECT => {
                let Some(mut session) = guard.take() else {
                    return Err(ToolError::Failed("No Bridge session is connected.".into()));
                };
                // C3: closing returns a report of unresolved state —
                // a superseded quarantine, inputs pending release and
                // outstanding jobs all surface in the final message;
                // nothing is silently erased with the session object.
                let report = session.close();
                let answer = if report.was_quarantined {
                    format!(
                        "Bridge session closed. WARNING: the session was QUARANTINED when closed — cleanup/effects were uncertain and remain unresolved ({} outstanding job(s) may still be running). A new session must reconcile application state before any mutation.",
                        report.outstanding_jobs.len()
                    )
                } else if report.inputs_pending_release > 0 || !report.outstanding_jobs.is_empty() {
                    format!(
                        "Bridge session closed. No user-owned application was touched. WARNING: {} input lease(s) still pending release and {} outstanding job(s) may still be running — they outlive this session and remain unobserved.",
                        report.inputs_pending_release,
                        report.outstanding_jobs.len()
                    )
                } else {
                    "Bridge session closed. No user-owned application was touched; no inputs or jobs remained outstanding.".to_string()
                };
                tool_result(answer)
            }
            _ => Err(ToolError::Failed("unknown bridge tool".into())),
        }
    }

    // ---- Test-only helpers: deterministic host-side stop/resume paths.
    // These never touch a driver, model, or OS process.

    /// Connect a session for deterministic tests (no adapter is involved).
    pub fn connect_for_tests(&self) {
        let manifest = self.manifest.clone();
        futures_executor_block(async move {
            let mut guard = self.session.lock().await;
            if guard.is_none() {
                let mut session = BridgeSession::new(
                    BridgeSessionId::new("bridge-test").expect("bounded"),
                    ApplicationInstanceId::new("none/seat-0", Generation(1)).expect("bounded"),
                    manifest,
                );
                session.connect().ok();
                session.mark_ready();
                *guard = Some(session);
            }
        });
    }

    /// Host-side stop (AT-21 core route): synchronous state manipulation.
    pub fn stop_for_tests(&self, outstanding_jobs: Vec<String>) {
        futures_executor_block(async {
            if let Some(session) = self.session.lock().await.as_mut() {
                session.stop(outstanding_jobs);
            }
        });
    }

    /// Production stop (NF-02): closes admission WITHOUT a model turn,
    /// reachable from both hosts' `/bridge stop` command path. Returns a
    /// truthful bounded report: closed admission, any inputs pending
    /// emergency release, and outstanding jobs that may still be running.
    /// Never requires or awaits model inference; never kills a
    /// user-owned application.
    pub fn stop(&self) -> String {
        futures_executor_block(async {
            let mut guard = self.session.lock().await;
            match guard.as_mut() {
                None => "Bridge: no session is connected; nothing to stop.".to_string(),
                Some(session) => {
                    let jobs = session.outstanding_jobs().to_vec();
                    let evicted = session.evicted_jobs();
                    let outcome = session.stop(jobs.clone());
                    let inputs = outcome.inputs_to_release.len();
                    let settled = match outcome.input_release {
                        vesper_bridge::lease::InputLeaseState::Released => "inputs released",
                        vesper_bridge::lease::InputLeaseState::Unconfirmed => {
                            "input release UNCONFIRMED — unsafe input state possible"
                        }
                    };
                    // M6: the job list is bounded — evicted entries are
                    // counted, never silently forgotten.
                    let eviction_note = if evicted > 0 {
                        format!(
                            " {evicted} earlier job(s) were evicted from the bounded window — this list is not exhaustive."
                        )
                    } else {
                        String::new()
                    };
                    // L2: if emergency release already ran for these
                    // generations, say so instead of implying a fresh
                    // release request.
                    let release_mark = {
                        let handle = session.fences_for_release();
                        let fences = handle
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        fences.emergency_released_through()
                    };
                    let release_note = if release_mark > 0 {
                        format!(
                            " Emergency release already ran through generation {release_mark} for unconfirmed inputs; re-reporting, not re-releasing."
                        )
                    } else {
                        String::new()
                    };
                    format!(
                        "Bridge stopped: admission closed. {} outstanding job(s) may still be running in the application and remain visible;{} {} input lease(s) pending release ({settled}).{release_note} Resume requires a fresh observation and renewed authority.",
                        jobs.len(),
                        eviction_note,
                        inputs
                    )
                }
            }
        })
    }

    /// Production input-release confirmation (BR-17): the host reports
    /// the driver acknowledged releasing its inputs. Settlement is an
    /// explicit act; it is never inferred from a driver ack string alone.
    pub fn confirm_input_release(&self) -> String {
        futures_executor_block(async {
            let mut guard = self.session.lock().await;
            match guard.as_mut() {
                None => "Bridge: no session is connected; nothing to confirm.".to_string(),
                Some(session) => {
                    session.confirm_input_release();
                    "Bridge input release confirmed: settled inputs are no longer reported for emergency release.".to_string()
                }
            }
        })
    }

    /// Production resume (NF-02): reopens admission and requires a fresh
    /// observation before the next dispatch (AT-21).
    pub fn resume(&self) -> String {
        futures_executor_block(async {
            let mut guard = self.session.lock().await;
            match guard.as_mut() {
                None => "Bridge: no session is connected; nothing to resume.".to_string(),
                Some(session) => match session.resume() {
                    Ok(()) => "Bridge resumed: admission open; a fresh observation is required before the next dispatch.".to_string(),
                    Err(error) => format!("Bridge resume refused: {error}. The session is closed; connect a new session instead."),
                },
            }
        })
    }

    /// Production disconnect (H4): closes the session for real and
    /// surfaces the C3 close report — quarantine superseded, inputs
    /// pending release, outstanding jobs. Shared by both hosts (BR-21);
    /// never kills a user-owned application.
    pub fn disconnect(&self) -> String {
        futures_executor_block(async {
            let mut guard = self.session.lock().await;
            match guard.take() {
                None => "Bridge: no session is connected; nothing to disconnect.".to_string(),
                Some(mut session) => {
                    let report = session.close();
                    if report.was_quarantined {
                        format!(
                            "Bridge disconnected. WARNING: the session was QUARANTINED — cleanup/effects were uncertain and remain unresolved ({} outstanding job(s) may still be running). A new session must reconcile application state before any mutation.",
                            report.outstanding_jobs.len()
                        )
                    } else if report.inputs_pending_release > 0
                        || !report.outstanding_jobs.is_empty()
                    {
                        format!(
                            "Bridge disconnected. WARNING: {} input lease(s) still pending release and {} outstanding job(s) may still be running — they outlive this session and remain unobserved.",
                            report.inputs_pending_release,
                            report.outstanding_jobs.len()
                        )
                    } else {
                        "Bridge disconnected: session closed cleanly; no inputs or jobs remained outstanding.".to_string()
                    }
                }
            }
        })
    }

    /// Host-side resume: forces a fresh observation on the next dispatch.
    pub fn resume_for_tests(&self) {
        futures_executor_block(async {
            if let Some(session) = self.session.lock().await.as_mut() {
                session.resume().ok();
            }
        });
    }

    /// Outstanding jobs snapshot for assertions.
    pub fn job_status_for_tests(&self) -> Vec<String> {
        futures_executor_block(async {
            self.session
                .lock()
                .await
                .as_ref()
                .map(|session| session.outstanding_jobs().to_vec())
                .unwrap_or_default()
        })
    }
}

fn futures_executor_block<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let waker = std::task::Waker::noop();
    let mut cx = std::task::Context::from_waker(waker);
    loop {
        match future.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(output) => return output,
            std::task::Poll::Pending => std::thread::yield_now(),
        }
    }
}

/// Map a core denial to model-visible, bounded text (§13.2 failure language).
fn denial_text(denial: DenialReason) -> Result<ToolResult, ToolError> {
    let text = match denial {
        DenialReason::PlanMode => "Denied: plan mode forbids application mutation; only read-only Bridge operations are available.".to_string(),
        DenialReason::ReadOnlyPermission => "Denied: read-only permission forbids application mutation.".to_string(),
        DenialReason::NotReady(state) => format!("Bridge session is not ready (state: {state:?}); connect first."),
        DenialReason::Quarantined => "Bridge session is quarantined: uncertain cleanup blocks new writes until reconciled.".to_string(),
        DenialReason::AdmissionClosed => "Bridge admission is closed (stop); resume with renewed authority and a fresh observation.".to_string(),
        DenialReason::Capability(error) => format!("Bridge capability error: {error}"),
        // H7: actionable lease detail — the two facts a retry decision
        // needs, instead of a bare error word.
        DenialReason::LeaseConflict { holder, expires_in_ms } => format!(
            "Bridge lease conflict: `{holder}` holds the resource; its lease expires in {expires_in_ms} ms. Retry after expiry or coordinate with the holder."
        ),
        DenialReason::StaleFence { observed, required } => format!(
            "Bridge lease fence is stale: request carries generation {observed}, the resource is at {required}. Re-observe and re-authorize."
        ),
        DenialReason::Lease(error) => format!("Bridge lease error: {error}"),
        DenialReason::StalePrecondition => "The target, observation or generation changed; a fresh observation and renewed validation are required.".to_string(),
        DenialReason::DuplicateSuppressed => "Duplicate request suppressed: the same request id was already accepted.".to_string(),
    };
    tool_result(text)
}

impl vesper_agent::ToolService for BridgeToolService {
    fn definitions(&self) -> Vec<ToolDefinition> {
        bridge_definitions()
    }

    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
        context: &'a vesper_agent::ToolContext,
    ) -> vesper_agent::ToolFuture<'a, Result<ToolResult, vesper_agent::ToolError>> {
        Box::pin(self.execute_call(call, context))
    }
}

fn tool_result(text: impl Into<String>) -> Result<ToolResult, ToolError> {
    ToolResult::new(text)
}

#[cfg(test)]
#[path = "bridge_service_tests.rs"]
mod service_tests;

#[cfg(test)]
#[path = "bridge_adapter_guard_tests.rs"]
mod adapter_guard_tests;

#[cfg(test)]
#[path = "bridge_adapter_tests.rs"]
mod adapter_tests;

#[cfg(test)]
#[path = "bridge_hardening_tests.rs"]
mod hardening_tests;

#[cfg(test)]
#[path = "bridge_latency_tests.rs"]
mod latency_tests;

#[cfg(test)]
#[path = "bridge_stop_tests.rs"]
mod stop_tests;

#[cfg(test)]
#[path = "bridge_production_stop_tests.rs"]
mod production_stop_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eight_tools_are_advertised_with_honest_classes() {
        let definitions = bridge_definitions();
        assert_eq!(definitions.len(), 8);
        let names: Vec<String> = definitions
            .iter()
            .map(|d| d.harness_name.as_str().to_owned())
            .collect();
        for expected in BRIDGE_TOOL_NAMES {
            assert!(names.contains(&expected.to_string()), "missing {expected}");
        }
        let execute = definitions
            .iter()
            .find(|d| d.harness_name.as_str() == BRIDGE_TOOL_EXECUTE)
            .unwrap();
        assert_eq!(execute.execution_class, ToolExecutionClass::Mutating);
        let observe = definitions
            .iter()
            .find(|d| d.harness_name.as_str() == BRIDGE_TOOL_OBSERVE)
            .unwrap();
        assert_eq!(observe.execution_class, ToolExecutionClass::ReadOnly);
    }

    #[test]
    fn no_adapter_manifest_is_truthfully_unavailable() {
        let service = BridgeToolService::no_adapter();
        assert!(service.manifest.records.iter().all(|record| matches!(
            record.implementation,
            Implementation::Unknown
        ) && matches!(
            record.availability,
            Availability::DependencyMissing
        )));
        assert!(
            service
                .manifest
                .records
                .iter()
                .all(|record| !record.dispatchable())
        );
    }
}
