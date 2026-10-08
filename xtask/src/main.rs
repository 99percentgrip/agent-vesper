#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

mod naming_baseline;
mod swarm_gate;

use clap::{Parser, Subcommand};
use serde::Deserialize;
use vesper_testkit::FixtureCorpus;

const SOURCE_COMMIT: &str = "bf4d4287e2e3320aa3f09015f678e6169d520045";

#[derive(Parser)]
#[command(about = "Agent Vesper repository maintenance")]
struct Cli {
    #[command(subcommand)]
    command: Task,
}

#[derive(Subcommand)]
enum Task {
    /// Run the complete local contract-foundation verification set.
    Verify,
    /// Run exact completion-assurance regression cases, refusing empty/ignored selections.
    Acceptance,
    /// Prove critical acceptance tests kill two bounded evaluator mutations.
    AcceptanceMutations,
    /// Validate architecture dependency and source boundaries.
    Architecture,
    /// Run checks under Rust 1.88.
    Msrv,
    /// Fixture corpus operations.
    Fixtures {
        #[command(subcommand)]
        command: FixtureTask,
    },
    /// Contract conformance operations.
    Contracts {
        #[command(subcommand)]
        command: ContractTask,
    },
    /// Provider-adapter verification.
    Provider {
        #[command(subcommand)]
        command: ProviderTask,
    },
    /// Minimal runtime verification.
    Runtime {
        #[command(subcommand)]
        command: RuntimeTask,
    },
    /// ACP adapter and process-transcript verification.
    Acp {
        #[command(subcommand)]
        command: AcpTask,
    },
    /// Read-only session-store verification.
    Sessions {
        #[command(subcommand)]
        command: SessionsTask,
    },
    /// Enforce the upstream-brand naming embargo (VRO-15 PR-1).
    NamingGuard {
        /// Regenerate the frozen baseline instead of enforcing it.
        #[arg(long)]
        regenerate: bool,
    },
}

#[derive(Subcommand)]
enum FixtureTask {
    /// Validate all manifests/results.
    Validate,
    /// Verify the authoritative SHA-256 index.
    VerifyIndex,
    /// Generate and validate a stage coverage map.
    Coverage {
        /// Migration stage number.
        #[arg(long)]
        stage: u32,
    },
}

#[derive(Subcommand)]
enum ContractTask {
    /// Verify Stage 2 contract vectors and coverage ownership.
    Verify,
}

#[derive(Subcommand)]
enum ProviderTask {
    /// Verify the production GLM adapter and Stage 3 coverage.
    Glm {
        #[command(subcommand)]
        command: GlmTask,
    },
}

#[derive(Subcommand)]
enum GlmTask {
    /// Run GLM provider conformance.
    Verify,
}

#[derive(Subcommand)]
enum RuntimeTask {
    /// Run minimal runtime conformance.
    Verify,
}

#[derive(Subcommand)]
enum AcpTask {
    /// Run ACP adapter and process transcript conformance.
    Verify,
}

#[derive(Subcommand)]
enum SessionsTask {
    /// Verify bounded read-only persistence and Stage 5 coverage.
    Verify,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let root = match env::current_dir()
        .map_err(|error| error.to_string())
        .and_then(|cwd| resolve_repository_root(&cwd))
    {
        Ok(root) => root,
        Err(error) => {
            eprintln!("xtask failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("verification workspace: {}", root.display());
    let result = match cli.command {
        Task::Verify => verify(),
        Task::Acceptance => acceptance_verify(),
        Task::AcceptanceMutations => acceptance_mutations(),
        Task::Architecture => architecture(),
        Task::Msrv => msrv(),
        Task::Fixtures {
            command: FixtureTask::Validate,
        } => fixtures_validate(),
        Task::Fixtures {
            command: FixtureTask::VerifyIndex,
        } => fixtures_verify_index(),
        Task::Fixtures {
            command: FixtureTask::Coverage { stage },
        } => fixtures_coverage(stage),
        Task::Contracts {
            command: ContractTask::Verify,
        } => contracts_verify(),
        Task::Provider {
            command: ProviderTask::Glm {
                command: GlmTask::Verify,
            },
        } => provider_glm_verify(),
        Task::Runtime {
            command: RuntimeTask::Verify,
        } => runtime_verify(),
        Task::Acp {
            command: AcpTask::Verify,
        } => acp_verify(),
        Task::Sessions {
            command: SessionsTask::Verify,
        } => sessions_verify(),
        Task::NamingGuard { regenerate } => naming_guard(regenerate),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn repository_root() -> PathBuf {
    resolve_repository_root(&env::current_dir().expect("validated invocation directory"))
        .expect("validated invocation workspace")
}

fn resolve_repository_root(cwd: &Path) -> Result<PathBuf, String> {
    let cwd = cwd.canonicalize().map_err(|error| error.to_string())?;
    for root in cwd.ancestors() {
        if root.join("xtask/src/main.rs").is_file()
            && root.join("xtask/Cargo.toml").is_file()
            && root.join("fixtures").is_dir()
            && fs::read_to_string(root.join("Cargo.toml"))
                .is_ok_and(|manifest| manifest.lines().any(|line| line.trim() == "[workspace]"))
        {
            return Ok(root.to_path_buf());
        }
    }
    Err(
        "invocation is not inside an Agent Vesper workspace; refusing cached build-path fallback"
            .into(),
    )
}

fn fixture_root() -> PathBuf {
    repository_root().join("fixtures")
}

fn fixtures_validate() -> Result<(), String> {
    let corpus = FixtureCorpus::load(fixture_root()).map_err(|error| error.to_string())?;
    if corpus.scenarios.len() != 76 {
        return Err(format!(
            "expected 76 fixture scenarios, found {}",
            corpus.scenarios.len()
        ));
    }
    println!(
        "validated {} scenarios: {:?}",
        corpus.scenarios.len(),
        corpus.category_counts()
    );
    Ok(())
}

fn fixtures_verify_index() -> Result<(), String> {
    let corpus = FixtureCorpus::load(fixture_root()).map_err(|error| error.to_string())?;
    let count = corpus.verify_index().map_err(|error| error.to_string())?;
    println!("verified {count} indexed fixture payloads");
    Ok(())
}

fn stage1_fixture_ids() -> BTreeSet<&'static str> {
    [
        "policy.ask-channel-failure",
        "policy.bypass-deny",
        "policy.matrix",
        "policy.nested-workflow-denial",
        "policy.plan-mcp",
        "policy.readonly-destructive",
        "security.canary-sinks",
        "security.promptware-wrapping",
        "security.secret-redaction",
        "session.reasoning-disabled",
        "session.reasoning-enabled",
        "session.unknown-fields",
    ]
    .into_iter()
    .collect()
}

fn owning_stage(category: &str) -> &'static str {
    match category {
        "acp" => "ACP transport stage",
        "provider/glm" => "GLM provider stage",
        "sessions/v1" => "session persistence stage",
        "tools" | "process" => "tools/process supervision stage",
        "security" => "owning security subsystem stage",
        "policy" => "policy integration stage",
        "contracts" => "stage-2-contracts",
        _ => "owning migration stage",
    }
}

#[derive(Debug, Deserialize)]
struct Stage2Plan {
    scenarios: Vec<Stage2PlanScenario>,
}

#[derive(Debug, Deserialize)]
struct Stage2PlanScenario {
    scenario_id: String,
    stage2_contract_surfaces: Vec<String>,
    runtime_surfaces: Vec<String>,
    owning_future_stage: String,
    evidence_strength: String,
}

fn fixtures_coverage(stage: u32) -> Result<(), String> {
    if stage == 5 {
        return fixtures_coverage_stage5();
    }
    if stage == 4 {
        return fixtures_coverage_stage4();
    }
    if stage == 3 {
        return fixtures_coverage_stage3();
    }
    if stage != 2 {
        return Err(
            "only Stage 2, Stage 3, Stage 4, and Stage 5 coverage generation is supported".into(),
        );
    }
    let corpus = FixtureCorpus::load(fixture_root()).map_err(|error| error.to_string())?;
    let plan_path = repository_root().join("fixtures/coverage-stage2-plan.json");
    let plan: Stage2Plan =
        serde_json::from_slice(&fs::read(&plan_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let planned = plan
        .scenarios
        .into_iter()
        .map(|scenario| (scenario.scenario_id.clone(), scenario))
        .collect::<BTreeMap<_, _>>();
    let stage1 = stage1_fixture_ids();
    let mut implemented_count = 0;
    let mut deferred_count = 0;
    let scenarios = corpus
        .scenarios
        .iter()
        .map(|fixture| {
            let id = fixture.manifest.scenario_id.as_str();
            let plan = planned.get(id);
            let synthetic = fixture.manifest.category == "contracts";
            let contract_surfaces = if let Some(plan) = plan {
                plan.stage2_contract_surfaces.clone()
            } else if synthetic {
                vec![format!(
                    "synthetic:{}",
                    id.strip_prefix("contract.").unwrap_or(id)
                )]
            } else if stage1.contains(id) {
                vec!["stage1-foundational-invariant".into()]
            } else {
                vec!["shared-domain-representation".into()]
            };
            let implemented_contracts = implemented_contracts(id, &fixture.manifest.category);
            if !implemented_contracts.is_empty() {
                implemented_count += 1;
            }
            let deferred = plan
                .map(|entry| entry.runtime_surfaces.clone())
                .unwrap_or_else(|| {
                    if synthetic {
                        Vec::new()
                    } else {
                        deferred_for_existing(id, &fixture.manifest.category)
                    }
                });
            if !deferred.is_empty() {
                deferred_count += 1;
            }
            let owner = plan
                .map(|entry| entry.owning_future_stage.clone())
                .unwrap_or_else(|| owning_stage(&fixture.manifest.category).into());
            let evidence = plan
                .map(|entry| entry.evidence_strength.clone())
                .unwrap_or_else(|| {
                    if synthetic {
                        "synthetic-future-contract"
                    } else {
                        "source-fixture"
                    }
                    .into()
                });
            serde_json::json!({
                "scenario_id": id,
                "category": fixture.manifest.category,
                "parsed": true,
                "schema_validated": true,
                "contract_surfaces": contract_surfaces,
                "implemented_contracts": implemented_contracts,
                "deferred_runtime_behavior": deferred,
                "owning_future_stage": owner,
                "test_references": test_references(id, &fixture.manifest.category),
                "evidence_strength": evidence,
                "synthetic_or_source": if synthetic { "synthetic-future-contract" } else { "frozen-source" }
            })
        })
        .collect::<Vec<_>>();
    let coverage = serde_json::json!({
        "schema_version": 1,
        "stage": 2,
        "source_commit": SOURCE_COMMIT,
        "fixture_index_sha256": corpus.index_sha256().map_err(|error| error.to_string())?,
        "generated_by": "cargo xtask fixtures coverage --stage 2",
        "summary": {
            "total": scenarios.len(),
            "parsed": scenarios.len(),
            "schema_validated": scenarios.len(),
            "contract_scenarios_with_implemented_contracts": implemented_count,
            "scenarios_with_deferred_runtime_behavior": deferred_count,
            "by_category": corpus.category_counts()
        },
        "scenarios": scenarios
    });
    let output = repository_root().join("fixtures/coverage-stage2.json");
    let bytes = serde_json::to_vec_pretty(&coverage).map_err(|error| error.to_string())?;
    fs::write(output, [bytes, b"\n".to_vec()].concat()).map_err(|error| error.to_string())?;
    contracts_verify()
}

fn fixtures_coverage_stage5() -> Result<(), String> {
    let corpus = FixtureCorpus::load(fixture_root()).map_err(|error| error.to_string())?;
    let scenarios = corpus
        .scenarios
        .iter()
        .map(|fixture| {
            let id = fixture.manifest.scenario_id.as_str();
            let category = fixture.manifest.category.as_str();
            let source_captured = category != "contracts";
            let session_fixture = category == "sessions/v1";
            let acp_lifecycle = matches!(
                id,
                "acp.list-session"
                    | "acp.load-session"
                    | "acp.resume-session"
                    | "acp.fork-session"
                    | "acp.close-session"
                    | "acp.replay-order"
            );
            let session_contract = matches!(
                id,
                "contract.invalid-session-bound"
                    | "contract.unknown-extension-roundtrip"
                    | "contract.error-redaction"
            );
            let security_contract = matches!(id, "security.secret-redaction");
            let stage5_owned =
                session_fixture || acp_lifecycle || session_contract || security_contract;
            let process_transcript = session_fixture || acp_lifecycle || security_contract;
            let future_owner = if stage5_owned {
                serde_json::Value::Null
            } else {
                serde_json::Value::String(owning_stage(category).into())
            };
            serde_json::json!({
                "scenario_id": id,
                "category": category,
                "parsed": true,
                "schema_validated": true,
                "source_or_synthetic": if source_captured { "source-captured" } else { "synthetic-contract" },
                "stage5_contract_represented": stage5_owned,
                "read_only_decode_implemented": session_fixture || session_contract,
                "metadata_listing_implemented": session_fixture || id == "acp.list-session",
                "runtime_load_resume_implemented": session_fixture || matches!(id, "acp.load-session" | "acp.resume-session"),
                "safe_replay_implemented": session_fixture || id == "acp.replay-order",
                "process_transcript_tested": process_transcript,
                "disk_invariance_proven": process_transcript,
                "persistent_write_implemented": false,
                "deferred_behavior": if stage5_owned {
                    Vec::<String>::new()
                } else {
                    vec![format!("not owned by Stage 5 read-only persistence: {}", owning_stage(category))]
                },
                "future_owner": future_owner,
                "test_references": if session_fixture {
                    vec![
                        "vesper-sessions compatibility/metadata/conversion/replay tests",
                        "agent-vesper-acp persistence process vectors"
                    ]
                } else if acp_lifecycle {
                    vec![
                        "vesper-runtime persistent lifecycle tests",
                        "vesper-acp lifecycle mapping tests",
                        "agent-vesper-acp persistence process vectors"
                    ]
                } else if session_contract || security_contract {
                    vec![
                        "vesper-sessions adversarial tests",
                        "vesper-testkit session-store tests"
                    ]
                } else {
                    vec!["deferred to named future owner"]
                }
            })
        })
        .collect::<Vec<_>>();
    let coverage = serde_json::json!({
        "schema_version": 1,
        "stage": 5,
        "source_commit": SOURCE_COMMIT,
        "fixture_index_sha256": corpus.index_sha256().map_err(|error| error.to_string())?,
        "generated_by": "cargo xtask fixtures coverage --stage 5",
        "fixture_provenance": {
            "source_captured": scenarios.iter().filter(|scenario| scenario["source_or_synthetic"] == "source-captured").count(),
            "synthetic_contract": scenarios.iter().filter(|scenario| scenario["source_or_synthetic"] == "synthetic-contract").count()
        },
        "summary": {
            "total": scenarios.len(),
            "stage5_contract_represented": scenarios.iter().filter(|scenario| scenario["stage5_contract_represented"] == true).count(),
            "session_source_scenarios": scenarios.iter().filter(|scenario| scenario["category"] == "sessions/v1").count(),
            "process_transcript_scenarios": scenarios.iter().filter(|scenario| scenario["process_transcript_tested"] == true).count(),
            "persistent_writes": 0,
            "by_category": corpus.category_counts()
        },
        "scenarios": scenarios
    });
    let output = repository_root().join("fixtures/coverage-stage5.json");
    let bytes = serde_json::to_vec_pretty(&coverage).map_err(|error| error.to_string())?;
    fs::write(output, [bytes, b"\n".to_vec()].concat()).map_err(|error| error.to_string())?;
    validate_stage5_coverage(&corpus)
}

fn validate_stage5_coverage(corpus: &FixtureCorpus) -> Result<(), String> {
    let path = repository_root().join("fixtures/coverage-stage5.json");
    let coverage: serde_json::Value =
        serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let scenarios = coverage["scenarios"]
        .as_array()
        .ok_or("Stage 5 coverage scenarios must be an array")?;
    if scenarios.len() != corpus.scenarios.len() {
        return Err("Stage 5 coverage does not cover the complete corpus".into());
    }
    if scenarios
        .iter()
        .any(|scenario| scenario["persistent_write_implemented"] != false)
    {
        return Err("Stage 5 coverage must not claim a persistent writer".into());
    }
    if scenarios.iter().any(|scenario| {
        scenario["stage5_contract_represented"] == false
            && scenario["future_owner"].as_str().is_none()
    }) {
        return Err("a non-Stage 5 scenario lacks a future owner".into());
    }
    let sessions = scenarios
        .iter()
        .filter(|scenario| scenario["category"] == "sessions/v1")
        .count();
    if sessions != 7 {
        return Err(format!("expected seven session fixtures, found {sessions}"));
    }
    println!(
        "validated Stage 5 coverage for {} scenarios ({sessions} sessions; no writes)",
        scenarios.len()
    );
    Ok(())
}

fn fixtures_coverage_stage4() -> Result<(), String> {
    let corpus = FixtureCorpus::load(fixture_root()).map_err(|error| error.to_string())?;
    let scenarios = corpus
        .scenarios
        .iter()
        .map(|fixture| {
            let id = fixture.manifest.scenario_id.as_str();
            let category = fixture.manifest.category.as_str();
            let acp = category == "acp";
            let runtime_contract = matches!(
                id,
                "contract.command-event-correlation"
                    | "contract.terminal-uniqueness"
                    | "contract.fragmented-parallel-tools"
                    | "contract.usage-provenance"
                    | "contract.error-redaction"
                    | "security.secret-redaction"
            );
            let stage41_provider = matches!(
                id,
                "glm.retryable-status"
                    | "glm.output-length-continuation"
                    | "glm.incomplete-eof-visible-output"
            );
            let temporary = match id {
                "acp.slash-command" => Some("catalog is advertised and parsed; /help matches the oracle fixture byte-exactly; host-owned commands (compact/undo/diff/export/checkpoint/rollback/plugins/mcp/usage/sessions/lineage/release/ci) return a truthful not-available response instead of full host implementations"),
                "acp.load-session" | "acp.resume-session" | "acp.list-session" => Some("lifecycle is current-process ephemeral; persistent behavior is deferred"),
                _ => None,
            };
            serde_json::json!({
                "scenario_id": id,
                "category": category,
                "parsed": true,
                "schema_validated": true,
                "contract_represented": acp || runtime_contract || category == "provider/glm",
                "acp_adapter_implemented": acp,
                "runtime_behavior_implemented": acp || runtime_contract || stage41_provider,
                "process_transcript_tested": matches!(
                    id,
                    "acp.initialization"
                        | "acp.capability-negotiation"
                        | "acp.new-session"
                        | "acp.list-session"
                        | "acp.load-session"
                        | "acp.resume-session"
                        | "acp.fork-session"
                        | "acp.close-session"
                        | "acp.replay-order"
                        | "acp.cancellation"
                        | "acp.usage-update-order"
                        | "acp.slash-command"
                        | "glm.retryable-status"
                        | "glm.output-length-continuation"
                        | "glm.incomplete-eof-visible-output"
                ),
                "exact_or_semantic": if matches!(id, "acp.initialization" | "acp.new-session" | "acp.cancellation" | "acp.usage-update-order") { "exact-wire" } else if acp || stage41_provider { "semantic-stage4" } else { "not-stage4-owned" },
                "temporary_difference": temporary,
                "runtime_behavior_deferred": !acp && !runtime_contract && !stage41_provider,
                "future_owner": if acp || runtime_contract || stage41_provider {
                    serde_json::Value::Null
                } else {
                    serde_json::Value::String(owning_stage(category).into())
                },
                "test_references": if stage41_provider {
                    vec!["agent-vesper-acp process_blockers"]
                } else if acp {
                    vec!["vesper-acp unit tests", "agent-vesper-acp process_transcript"]
                } else if runtime_contract {
                    vec!["vesper-runtime integration tests", "vesper-testkit conformance tests"]
                } else {
                    vec!["deferred to named future owner"]
                }
            })
        })
        .collect::<Vec<_>>();
    let coverage = serde_json::json!({
        "schema_version": 1,
        "stage": 4,
        "source_commit": SOURCE_COMMIT,
        "fixture_index_sha256": corpus.index_sha256().map_err(|error| error.to_string())?,
        "generated_by": "cargo xtask fixtures coverage --stage 4",
        "stage4_1_process_vectors": [
            "retry-before-visible-output",
            "output-limit-continuation",
            "post-output-interruption-no-replay",
            "cancel-before-provider-dispatch",
            "cross-session-concurrency",
            "same-session-serialization",
            "slow-consumer-bounded-backpressure"
        ],
        "summary": {
            "total": scenarios.len(),
            "acp_scenarios_implemented": scenarios.iter().filter(|scenario| scenario["acp_adapter_implemented"] == true).count(),
            "process_transcript_scenarios": scenarios.iter().filter(|scenario| scenario["process_transcript_tested"] == true).count(),
            "runtime_contract_scenarios": scenarios.iter().filter(|scenario| scenario["runtime_behavior_implemented"] == true).count(),
            "by_category": corpus.category_counts()
        },
        "scenarios": scenarios
    });
    let output = repository_root().join("fixtures/coverage-stage4.json");
    let bytes = serde_json::to_vec_pretty(&coverage).map_err(|error| error.to_string())?;
    fs::write(output, [bytes, b"\n".to_vec()].concat()).map_err(|error| error.to_string())?;
    validate_stage4_coverage(&corpus)
}

fn validate_stage4_coverage(corpus: &FixtureCorpus) -> Result<(), String> {
    let path = repository_root().join("fixtures/coverage-stage4.json");
    let coverage: serde_json::Value =
        serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let scenarios = coverage["scenarios"]
        .as_array()
        .ok_or("Stage 4 coverage scenarios must be an array")?;
    if scenarios.len() != corpus.scenarios.len() {
        return Err("Stage 4 coverage does not cover the complete corpus".into());
    }
    let acp = scenarios
        .iter()
        .filter(|scenario| scenario["acp_adapter_implemented"] == true)
        .count();
    if acp != 12 {
        return Err(format!("expected 12 ACP scenarios, found {acp}"));
    }
    if scenarios.iter().any(|scenario| {
        scenario["runtime_behavior_deferred"] == true && scenario["future_owner"].as_str().is_none()
    }) {
        return Err("a deferred Stage 4 scenario lacks a future owner".into());
    }
    println!(
        "validated Stage 4 coverage for {} scenarios ({acp} ACP)",
        scenarios.len()
    );
    Ok(())
}

fn fixtures_coverage_stage3() -> Result<(), String> {
    let corpus = FixtureCorpus::load(fixture_root()).map_err(|error| error.to_string())?;
    let scenarios = corpus
        .scenarios
        .iter()
        .map(|fixture| {
            let id = fixture.manifest.scenario_id.as_str();
            let category = fixture.manifest.category.as_str();
            let glm = category == "provider/glm";
            let contract = matches!(
                id,
                "contract.error-redaction"
                    | "contract.fallback-observable"
                    | "contract.fragmented-parallel-tools"
                    | "contract.opaque-reasoning"
                    | "contract.terminal-uniqueness"
                    | "contract.unknown-finish"
                    | "contract.usage-provenance"
                    | "security.secret-redaction"
            );
            let owned = glm || contract;
            serde_json::json!({
                "scenario_id": id,
                "category": category,
                "parsed": true,
                "schema_validated": true,
                "contract_represented": owned,
                "glm_adapter_implemented": glm,
                "wire_serialization_tested": glm,
                "stream_behavior_tested": glm || matches!(id, "contract.fragmented-parallel-tools" | "contract.terminal-uniqueness" | "contract.unknown-finish" | "contract.usage-provenance"),
                "error_behavior_tested": glm || matches!(id, "contract.error-redaction" | "security.secret-redaction"),
                "cancellation_tested": matches!(id, "glm.cancel-before-connect" | "glm.cancel-before-headers" | "glm.cancel-mid-stream"),
                "runtime_behavior_deferred": !owned,
                "future_owner": if owned { serde_json::Value::Null } else { serde_json::Value::String(owning_stage(category).into()) },
                "test_references": if glm {
                    vec!["vesper-provider-glm::integration_tests", "vesper-provider-glm unit tests"]
                } else if contract {
                    vec!["vesper-provider-glm unit tests", "vesper-testkit conformance tests"]
                } else {
                    vec!["deferred to named future owner"]
                }
            })
        })
        .collect::<Vec<_>>();
    let coverage = serde_json::json!({
        "schema_version": 1,
        "stage": 3,
        "source_commit": SOURCE_COMMIT,
        "fixture_index_sha256": corpus.index_sha256().map_err(|error| error.to_string())?,
        "generated_by": "cargo xtask fixtures coverage --stage 3",
        "summary": {
            "total": scenarios.len(),
            "glm_source_scenarios_implemented": scenarios.iter().filter(|scenario| scenario["glm_adapter_implemented"] == true).count(),
            "other_scenarios_deferred": scenarios.iter().filter(|scenario| scenario["runtime_behavior_deferred"] == true).count(),
            "by_category": corpus.category_counts()
        },
        "scenarios": scenarios
    });
    let output = repository_root().join("fixtures/coverage-stage3.json");
    let bytes = serde_json::to_vec_pretty(&coverage).map_err(|error| error.to_string())?;
    fs::write(output, [bytes, b"\n".to_vec()].concat()).map_err(|error| error.to_string())?;
    validate_stage3_coverage(&corpus)
}

fn validate_stage3_coverage(corpus: &FixtureCorpus) -> Result<(), String> {
    let path = repository_root().join("fixtures/coverage-stage3.json");
    let coverage: serde_json::Value =
        serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let scenarios = coverage["scenarios"]
        .as_array()
        .ok_or("Stage 3 coverage scenarios must be an array")?;
    if scenarios.len() != corpus.scenarios.len() {
        return Err("Stage 3 coverage does not cover the complete corpus".into());
    }
    let implemented = scenarios
        .iter()
        .filter(|scenario| scenario["glm_adapter_implemented"] == true)
        .count();
    if implemented != 21 {
        return Err(format!(
            "expected 21 GLM source scenarios implemented, found {implemented}"
        ));
    }
    if scenarios.iter().any(|scenario| {
        scenario["runtime_behavior_deferred"] == true && scenario["future_owner"].as_str().is_none()
    }) {
        return Err("a deferred Stage 3 scenario lacks a future owner".into());
    }
    println!(
        "validated Stage 3 coverage for {} scenarios ({implemented} GLM)",
        scenarios.len()
    );
    Ok(())
}

fn provider_glm_verify() -> Result<(), String> {
    let corpus = FixtureCorpus::load(fixture_root()).map_err(|error| error.to_string())?;
    validate_stage3_coverage(&corpus)?;
    run("cargo", &["test", "-p", "vesper-provider-glm"])
}

fn runtime_verify() -> Result<(), String> {
    run("cargo", &["test", "-p", "vesper-runtime", "--all-features"])
}

fn acp_verify() -> Result<(), String> {
    run("cargo", &["test", "-p", "vesper-acp", "--all-features"])?;
    run(
        "cargo",
        &[
            "test",
            "-p",
            "agent-vesper-acp",
            "--test",
            "process_transcript",
        ],
    )?;
    run(
        "cargo",
        &[
            "test",
            "-p",
            "agent-vesper-acp",
            "--all-features",
            "--test",
            "process_blockers",
        ],
    )
}

fn sessions_verify() -> Result<(), String> {
    let corpus = FixtureCorpus::load(fixture_root()).map_err(|error| error.to_string())?;
    validate_stage5_coverage(&corpus)?;
    scan_stage5_sources(&repository_root())?;
    run(
        "cargo",
        &["test", "-p", "vesper-sessions", "--all-features"],
    )?;
    run("cargo", &["test", "-p", "vesper-testkit", "--all-features"])
}

fn implemented_contracts(id: &str, category: &str) -> Vec<&'static str> {
    match category {
        "acp" => vec![
            "harness-command-event-representation",
            "identity-and-order-preservation",
        ],
        "provider/glm" => vec![
            "provider-request-stream-error-representation",
            "partial-output-and-terminal-state-contract",
        ],
        "sessions/v1" => vec!["legacy-session-v1-read-write-free-codec"],
        "tools" => vec!["tool-schema-call-result-and-policy-classification"],
        "process" => vec!["bounded-process-observation-contract"],
        "policy" => vec!["pure-policy-precedence-invariant"],
        "security" if id == "security.plugin-signature" => {
            vec!["security-outcome-and-evidence-contract"]
        }
        "security" if id == "security.checkpoint-conflict" => {
            vec!["security-outcome-and-conflict-contract"]
        }
        "security" => vec!["foundational-security-invariant"],
        "contracts" => vec!["synthetic-provider-neutral-contract-vector"],
        _ => Vec::new(),
    }
}

fn deferred_for_existing(id: &str, category: &str) -> Vec<String> {
    if category == "policy"
        || matches!(
            id,
            "security.secret-redaction"
                | "security.promptware-wrapping"
                | "security.canary-sinks"
                | "session.reasoning-enabled"
                | "session.reasoning-disabled"
                | "session.unknown-fields"
        )
    {
        Vec::new()
    } else {
        vec![format!(
            "{} runtime behavior",
            owning_stage(category).trim_end_matches(" stage")
        )]
    }
}

fn test_references(id: &str, category: &str) -> Vec<&'static str> {
    if category == "sessions/v1" {
        vec![
            "vesper-domain::compatibility::tests",
            "vesper-testkit::fixture::tests",
        ]
    } else if category == "provider/glm" || category == "contracts" {
        vec![
            "vesper-provider::stream::tests",
            "vesper-provider::request::tests",
            "vesper-testkit::conformance::tests",
        ]
    } else if category == "acp" {
        vec![
            "vesper-domain::event::tests",
            "vesper-testkit::conformance::tests",
        ]
    } else if category == "policy" {
        vec!["vesper-policy::tests"]
    } else if id.contains("secret") || id.contains("promptware") {
        vec![
            "vesper-security::tests",
            "vesper-testkit::conformance::tests",
        ]
    } else {
        vec!["vesper-testkit::fixture::tests"]
    }
}

fn contracts_verify() -> Result<(), String> {
    let corpus = FixtureCorpus::load(fixture_root()).map_err(|error| error.to_string())?;
    let coverage_path = repository_root().join("fixtures/coverage-stage2.json");
    let coverage: serde_json::Value =
        serde_json::from_slice(&fs::read(&coverage_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let scenarios = coverage["scenarios"]
        .as_array()
        .ok_or("Stage 2 coverage scenarios must be an array")?;
    if scenarios.len() != corpus.scenarios.len() {
        return Err("Stage 2 coverage does not cover the complete corpus".into());
    }
    let expected = corpus
        .scenarios
        .iter()
        .map(|scenario| scenario.manifest.scenario_id.as_str())
        .collect::<BTreeSet<_>>();
    let actual = scenarios
        .iter()
        .filter_map(|scenario| scenario["scenario_id"].as_str())
        .collect::<BTreeSet<_>>();
    if expected != actual {
        return Err("Stage 2 coverage scenario IDs differ from the corpus".into());
    }
    for scenario in scenarios {
        let deferred = scenario["deferred_runtime_behavior"]
            .as_array()
            .ok_or("deferred runtime behavior must be an array")?;
        let owner = scenario["owning_future_stage"].as_str().unwrap_or_default();
        if !deferred.is_empty() && owner.is_empty() {
            return Err(format!(
                "deferred scenario {} lacks a future owner",
                scenario["scenario_id"]
            ));
        }
        if scenario["implemented_contracts"]
            .as_array()
            .is_none_or(Vec::is_empty)
        {
            return Err(format!(
                "scenario {} lacks an implemented Stage 2 contract",
                scenario["scenario_id"]
            ));
        }
    }
    let synthetic = corpus
        .scenarios
        .iter()
        .filter(|scenario| scenario.manifest.category == "contracts")
        .count();
    if synthetic != 11 {
        return Err(format!(
            "expected 11 synthetic contract vectors, found {synthetic}"
        ));
    }
    println!(
        "verified Stage 2 contracts for {} scenarios",
        scenarios.len()
    );
    Ok(())
}

fn architecture() -> Result<(), String> {
    let root = repository_root();
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(&root)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let metadata: Metadata =
        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
    let workspace: BTreeSet<_> = metadata.workspace_members.into_iter().collect();
    let names = metadata
        .packages
        .iter()
        .filter(|package| workspace.contains(&package.id))
        .map(|package| (package.id.clone(), package.name.clone()))
        .collect::<BTreeMap<_, _>>();
    let allowed = allowed_dependencies();
    for package in metadata
        .packages
        .iter()
        .filter(|package| workspace.contains(&package.id))
    {
        swarm_gate::validate(package)?;
        for dependency in &package.dependencies {
            let workspace_target = dependency
                .path
                .as_ref()
                .and_then(|_| names.values().find(|name| *name == &dependency.name));
            // VRO-13 PR-8: `vesper-harness` integration tests compose the
            // real foundational seams (firewall, provider factory, testkit
            // fakes) that production `src/` must NOT link. Dev-kind deps
            // never enter the production build, so they are allowed for
            // the harness's cross-feature fixtures while the allowlist
            // above keeps binding for every normal dependency everywhere.
            let harness_dev = package.name == "vesper-harness"
                && dependency.kind.as_deref() == Some("dev")
                && matches!(
                    dependency.name.as_str(),
                    "vesper-policy" | "vesper-provider" | "vesper-testkit"
                );
            if let Some(target) = workspace_target
                && !harness_dev
                && !allowed
                    .get(package.name.as_str())
                    .is_some_and(|targets| targets.contains(target.as_str()))
            {
                return Err(format!(
                    "workspace dependency {} -> {} violates the architecture",
                    package.name, target
                ));
            }
            if !matches!(package.name.as_str(), "vesper-testkit" | "xtask")
                && dependency.name == "vesper-testkit"
                && dependency.kind.as_deref() != Some("dev")
            {
                return Err(format!(
                    "{} may use vesper-testkit only as a dev dependency",
                    package.name
                ));
            }
            if dependency.name == "agent-client-protocol" && package.name != "vesper-acp" {
                return Err(format!(
                    "ACP SDK dependency escaped vesper-acp into {}",
                    package.name
                ));
            }
            if matches!(
                dependency.name.as_str(),
                "rusqlite" | "sqlx" | "libsqlite3-sys"
            ) && package.name != "vesper-cognition"
            {
                // ADR 0015 (Stage 16): `vesper-cognition` is the only
                // production crate permitted to declare a SQLite dependency.
                // Every other crate remains SQLite-free. (The historical
                // "Stage 5" blanket ban is superseded by this allowlist.)
                return Err(format!(
                    "SQLite dependency {} is prohibited outside vesper-cognition",
                    dependency.name
                ));
            }
            if dependency
                .source
                .as_deref()
                .is_some_and(|value| value.starts_with("git+"))
                && !dependency
                    .source
                    .as_deref()
                    .is_some_and(|value| value.contains('#'))
            {
                return Err(format!(
                    "Git dependency {} in {} is not revision pinned",
                    dependency.name, package.name
                ));
            }
            if dependency.path.is_none() && dependency.requirement == "*" {
                return Err(format!(
                    "dependency {} in {} uses a wildcard requirement",
                    dependency.name, package.name
                ));
            }
        }
    }
    scan_production_sources(&root)?;
    scan_stage4_sources(&root)?;
    scan_stage5_sources(&root)?;
    scan_production_scenario_ids(&root)?;
    println!(
        "architecture boundaries validated for {} packages",
        names.len()
    );
    Ok(())
}

fn scan_production_scenario_ids(root: &Path) -> Result<(), String> {
    let corpus = FixtureCorpus::load(fixture_root()).map_err(|error| error.to_string())?;
    for source_root in [root.join("crates"), root.join("apps")] {
        for entry in fs::read_dir(source_root).map_err(|error| error.to_string())? {
            let package = entry.map_err(|error| error.to_string())?.path();
            if !package.is_dir() {
                continue;
            }
            if package.file_name().and_then(|name| name.to_str()) == Some("vesper-testkit") {
                continue;
            }
            let mut files = Vec::new();
            collect_source_files(&package.join("src"), &mut files)?;
            for file in files {
                if file.file_name().and_then(|name| name.to_str()) == Some("integration_tests.rs") {
                    continue;
                }
                let source = fs::read_to_string(&file).map_err(|error| error.to_string())?;
                for fixture in &corpus.scenarios {
                    if source.contains(&fixture.manifest.scenario_id) {
                        return Err(format!(
                            "fixture scenario ID {} entered production module {}",
                            fixture.manifest.scenario_id,
                            file.display()
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

fn scan_stage5_sources(root: &Path) -> Result<(), String> {
    let sessions = root.join("crates/vesper-sessions/src");
    // Stage 6 introduces a single bounded transactional writer module. Every
    // other module remains strictly read-only.
    let write_modules: BTreeSet<&str> = ["writer.rs"].into_iter().collect();
    // Filesystem mutation APIs: forbidden everywhere except the writer module.
    let forbidden_writes = [
        "fs::write",
        "fs::create_dir",
        "File::create",
        "OpenOptions",
        "remove_file",
        "remove_dir",
        "rename(",
        "create_dir_all",
    ];
    // Forbidden dependencies: forbidden in every session module unconditionally.
    let forbidden_dependencies = [
        "rusqlite",
        "sqlx",
        "libsqlite3_sys",
        "agent_client_protocol",
        "vesper_provider_glm",
        "vesper_runtime",
    ];
    let mut files = Vec::new();
    collect_source_files(&sessions, &mut files)?;
    for file in files {
        let source = fs::read_to_string(&file).map_err(|error| error.to_string())?;
        let is_write_module = file
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| write_modules.contains(name));
        for term in forbidden_dependencies {
            if source.contains(term) {
                return Err(format!(
                    "Stage 5 dependency boundary term {term:?} found in {}",
                    file.display()
                ));
            }
        }
        if !is_write_module {
            for term in forbidden_writes {
                if source.contains(term) {
                    return Err(format!(
                        "Stage 5 read-only boundary term {term:?} found in {}",
                        file.display()
                    ));
                }
            }
        }
    }
    Ok(())
}

fn scan_stage4_sources(root: &Path) -> Result<(), String> {
    let runtime = root.join("crates/vesper-runtime/src");
    let acp = root.join("crates/vesper-acp/src");
    let app = root.join("apps/agent-vesper-acp/src");
    for (path, forbidden) in [
        (
            runtime,
            &[
                "std::fs",
                "tokio::fs",
                "rusqlite",
                "agent_client_protocol",
                "vesper_provider_glm",
                "unbounded_channel",
            ][..],
        ),
        (
            acp,
            &[
                "reqwest",
                "rusqlite",
                "vesper_provider_glm",
                "unbounded_channel",
            ][..],
        ),
    ] {
        let mut files = Vec::new();
        collect_source_files(&path, &mut files)?;
        for file in files {
            let source = fs::read_to_string(&file).map_err(|error| error.to_string())?;
            for term in forbidden {
                if source.contains(term) {
                    return Err(format!(
                        "Stage 4 boundary term {term:?} found in {}",
                        file.display()
                    ));
                }
            }
        }
    }
    let main = fs::read_to_string(app.join("main.rs")).map_err(|error| error.to_string())?;
    if main.lines().any(|line| line.trim().starts_with("println!")) {
        return Err("ACP composition binary may not write normal output to stdout".into());
    }
    let app_manifest = fs::read_to_string(root.join("apps/agent-vesper-acp/Cargo.toml"))
        .map_err(|error| error.to_string())?;
    if !app_manifest.contains("required-features = [\"integration-test-harness\"]")
        || app_manifest.contains("default = [\"integration-test-harness\"]")
    {
        return Err(
            "ACP dispatch-gate test driver must remain unavailable in default release builds"
                .into(),
        );
    }
    Ok(())
}

fn naming_guard(regenerate: bool) -> Result<(), String> {
    // VRO-15 PR-1: the upstream-brand naming embargo, enforced as a
    // fail-closed ratchet. The embargo scope and forbidden patterns are
    // defined below in hex so this source file contains no upstream brand
    // strings of its own. The baseline freezes pre-existing mentions
    // (historical docs, seed skills, host-UX comparisons) as counted file/content
    // SHA digests; line shifts do not change identity. Removing a
    // baseline entry is allowed and does not fail; `--regenerate` rewrites
    // the baseline (maintainer action, then commit the result).
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;

    let root = repository_root();
    let baseline_path = root.join("xtask").join("naming-guard-baseline.json");
    // UTF-8 lowercase words. Only the standalone brand stems are needed:
    // substring compounds (e.g. hyphenated or path forms) all contain one
    // of these stems. Matching is word-bounded so ordinary words that
    // merely contain a stem cannot false-positive.
    const FORBIDDEN_HEX: &[&str] = &[
        // pattern 1 (product name and its owner-handled variants)
        "7275666c6f",
        // pattern 2 (owner name and variants)
        "7275766e6574",
        "727576696e",
        "7275766e6f",
        // pattern 3 (assistant product name; covers hyphenated compounds)
        "636c61756465",
        // pattern 4 (vendor name)
        "616e7468726f706963",
        // pattern 5 (upstream database product)
        "6167656e746462",
        // pattern 6 (upstream memory product)
        "736f6e61",
        // pattern 7 (VRO-16 governance alpha upstream: product name)
        "6167656e6379636c69",
        // pattern 8 (VRO-16 governance beta upstream: long compound name)
        "6175746f7265736561726368636c6177",
        // pattern 9 (VRO-16 governance beta upstream: short stem; the long
        // compound above embeds this stem, but the short form is used alone
        // in the upstream's CLI verbs, config keys and doc filenames)
        "7265736561726368636c6177",
        // pattern 10 (VRO-17 voice oracle upstream: product name stem)
        "6a6172766973",
        // pattern 11 (VRO-17 voice oracle upstream's agent runtime: brand
        // stem; appears in its API surface and config keys)
        "6865726d6573",
    ];
    // The frozen baseline also pins the expected token-count; a guard edit
    // that silently drops a check must fail instead of passing quietly.
    const FORBIDDEN_TOKEN_COUNT: usize = 13;

    let mut pattern_bytes = Vec::with_capacity(FORBIDDEN_HEX.len());
    for hex in FORBIDDEN_HEX {
        let mut bytes = Vec::with_capacity(hex.len() / 2);
        let value = hex.as_bytes();
        for pair in value.chunks_exact(2) {
            let high = (pair[0] as char).to_digit(16).ok_or("bad hex")?;
            let low = (pair[1] as char).to_digit(16).ok_or("bad hex")?;
            bytes.push(((high << 4) | low) as u8);
        }
        pattern_bytes.push(bytes);
    }
    let patterns: Vec<String> = pattern_bytes
        .iter()
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .collect();
    if patterns.len() != FORBIDDEN_TOKEN_COUNT {
        return Err(format!(
            "naming guard token integrity failure: expected {FORBIDDEN_TOKEN_COUNT} forbidden tokens, found {}",
            patterns.len()
        ));
    }

    // Embargo scope (directive): docs/, AGENTS.md, README, crates/**.
    // Exclusions: generated/lock artifacts and the mirror directory. Files
    // are walked deterministically (sorted) so baselines are reproducible.
    let scope_dirs = ["docs", "crates"];
    let root_files = ["AGENTS.md", "README.md"];

    let mut files = Vec::new();
    for dir in scope_dirs {
        collect_text_files(&root.join(dir), &mut files)?;
    }
    for name in root_files {
        let path = root.join(name);
        if path.is_file() {
            files.push(path);
        }
    }
    files.sort();

    let mut current: Vec<naming_baseline::Hit> = Vec::new();
    for path in &files {
        let Ok(source) = fs::read_to_string(path) else {
            continue;
        };
        let relative = path
            .strip_prefix(&root)
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string_lossy().into_owned())
            .replace('\\', "/");
        for (index, line) in source.lines().enumerate() {
            let lower = line.to_lowercase();
            for pattern in &patterns {
                if contains_word_bounded(&lower, pattern) {
                    let mut digest_input = String::with_capacity(relative.len() + line.len() + 8);
                    let _ = write!(digest_input, "{relative}\n{line}");
                    let digest = Sha256::digest(digest_input.as_bytes());
                    current.push(naming_baseline::Hit {
                        path: relative.clone(),
                        line: index + 1,
                        sha256: hex_digest(&digest),
                    });
                    break;
                }
            }
        }
    }

    if regenerate {
        fs::write(&baseline_path, naming_baseline::encode(&current)?)
            .map_err(|error| error.to_string())?;
        println!(
            "naming-guard: regenerated baseline with {} frozen hits",
            current.len()
        );
        return Ok(());
    }

    let baseline = fs::read_to_string(&baseline_path)
        .map_err(|_| format!("missing baseline {}", baseline_path.display()))?;
    let rejected = naming_baseline::violations(&baseline, &current)?;
    for hit in &rejected {
        eprintln!(
            "naming-guard violation: {}:{} {}",
            hit.path, hit.line, hit.sha256
        );
    }
    let violations = rejected.len();
    if violations > 0 {
        return Err(format!(
            "naming embargo violated: {violations} new upstream-brand hit(s) not in {}",
            baseline_path.display()
        ));
    }
    println!(
        "naming-guard: clean ({} hits, all frozen in baseline)",
        current.len()
    );
    Ok(())
}

fn hex_digest(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Word-bounded containment: the pattern must not be flanked by ASCII
/// alphanumerics. Keeps hyphenated compounds and standalone names matching
/// while ordinary words that merely embed a stem stay legal.
fn contains_word_bounded(haystack: &str, needle: &str) -> bool {
    let mut offset = 0;
    while let Some(found) = haystack[offset..].find(needle) {
        let start = offset + found;
        let end = start + needle.len();
        let before_ok = haystack[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_alphanumeric());
        let after_ok = haystack[end..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_ascii_alphanumeric());
        if before_ok && after_ok {
            return true;
        }
        offset = end;
    }
    false
}

fn collect_text_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if matches!(
                name.as_ref(),
                "node_modules" | "target" | ".git" | ".agent-vesper"
            ) {
                continue;
            }
            collect_text_files(&path, out)?;
        } else if matches!(
            name.as_ref(),
            "Cargo.toml" | "Cargo.lock" | "AGENTS.md" | "README.md"
        ) || name.ends_with(".rs")
            || name.ends_with(".md")
            || name.ends_with(".toml")
        {
            out.push(path);
        }
    }
    Ok(())
}

fn allowed_dependencies() -> BTreeMap<&'static str, BTreeSet<&'static str>> {
    BTreeMap::from([
        ("vesper-domain", BTreeSet::new()),
        ("vesper-security", BTreeSet::new()),
        ("vesper-auth", BTreeSet::from(["vesper-security"])),
        (
            "vesper-memory",
            BTreeSet::from(["vesper-domain", "vesper-security"]),
        ),
        (
            // ADR 0015 (Stage 16): cognitive memory engine. Owns SQLite +
            // FTS5 + the trait ports for embeddings/extraction-LLM/entity-NLP.
            // Concrete provider impls live at the composition boundary
            // (apps/agent-vesper-tui/src/main.rs). This is the only production
            // crate permitted to declare `rusqlite` (per-crate exception
            // below in scan_production_sources).
            "vesper-cognition",
            BTreeSet::from(["vesper-domain", "vesper-security"]),
        ),
        (
            "vesper-checkpoints",
            BTreeSet::from(["vesper-domain", "vesper-security"]),
        ),
        (
            "vesper-config",
            BTreeSet::from(["vesper-domain", "vesper-security"]),
        ),
        (
            "vesper-mcp",
            BTreeSet::from(["vesper-domain", "vesper-security"]),
        ),
        ("vesper-provider", BTreeSet::from(["vesper-domain"])),
        (
            // VRO-14 PR-1: the web oracle perception engine (pure DOM
            // transformation pipeline). Zero I/O by construction: parsing,
            // strip, density pruning, and markdown conversion are all pure
            // functions over in-memory HTML strings. Network transports and
            // the headless renderer are later PRs' ports, implemented at the
            // composition boundary, never here.
            "vesper-web",
            BTreeSet::from(["vesper-domain"]),
        ),
        (
            // VRO-17 PR-0: the voice subsystem's pure core. Contracts,
            // ports, descriptors, configuration, and fakes only — no
            // adapters, no I/O, no clock, no device access. Hosts
            // translate runtime/provider events into voice-owned inputs
            // at the composition boundary; this crate never depends on
            // the runtime, agent, harness, or any provider adapter.
            // Speech-egress policy and bounded budgets live here
            // (config validation); real engines arrive in PR-1/PR-2.
            "vesper-voice",
            BTreeSet::from(["vesper-domain", "vesper-security"]),
        ),
        (
            // VRO-17 R3: the Natural Voice pack adapter (composition
            // adapter for the voice core's TTS port). Depends on the
            // pure voice core (which owns the ports) plus the shared
            // domain/security foundations; the inference backend is a
            // lazily loaded, digest-verified pack component — never a
            // build-time link. Default-off `ort`/`mock-synthesis`
            // features; the crate compiles bare.
            "vesper-voice-kokoro",
            BTreeSet::from(["vesper-domain", "vesper-security", "vesper-voice"]),
        ),
        (
            // VRO-14 PR-3: the sandboxed fetch route. This is the ONE
            // production unit allowed to reference the sandbox backend and
            // reqwest-scanning exception: it executes the fetch helper
            // inside an IsolationRequirement::Network sandbox with an
            // explicit network grant. The helper binary performs the only
            // web egress in the workspace, always inside the sandbox.
            "vesper-web-fetch",
            BTreeSet::from(["vesper-sandbox", "vesper-security", "vesper-web"]),
        ),
        (
            "vesper-policy",
            BTreeSet::from(["vesper-domain", "vesper-security"]),
        ),
        (
            // ADR 0022 (VRO-13 PR-3): Linux namespaces sandbox backend. The
            // library is entirely safe code (traits, specs, probing, process
            // orchestration via std::process); the only unsafe in the crate
            // lives in the `sandbox_init` supervisor binary, which performs
            // raw namespace/mount syscalls before exec. Per-crate exception
            // enforced by scan_production_sources' sandbox-safety gate.
            "vesper-sandbox",
            BTreeSet::from(["vesper-security"]),
        ),
        (
            "vesper-testkit",
            BTreeSet::from([
                "vesper-domain",
                "vesper-provider",
                "vesper-security",
                "vesper-policy",
                "vesper-config",
                // VB-PRD-001 Phase 1: the visibly-fake deterministic Bridge
                // driver (FakeApplication) for test-only evidence.
                "vesper-bridge",
            ]),
        ),
        (
            "vesper-provider-glm",
            BTreeSet::from([
                "vesper-auth",
                "vesper-domain",
                "vesper-provider",
                "vesper-config",
                "vesper-security",
                "vesper-testkit",
            ]),
        ),
        (
            "vesper-provider-synthetic",
            BTreeSet::from(["vesper-domain", "vesper-provider"]),
        ),
        (
            "vesper-provider-openai",
            BTreeSet::from([
                "vesper-auth",
                "vesper-domain",
                "vesper-provider",
                "vesper-config",
                "vesper-security",
            ]),
        ),
        (
            // VRO-18: native xAI HTTP adapter. Provider wire/authentication
            // behavior stays in this leaf crate and does not enter shared runtime.
            "vesper-provider-xai",
            BTreeSet::from([
                "vesper-auth",
                "vesper-domain",
                "vesper-provider",
                "vesper-security",
            ]),
        ),
        (
            "vesper-runtime",
            BTreeSet::from([
                "vesper-domain",
                "vesper-provider",
                "vesper-sessions",
                "vesper-testkit",
            ]),
        ),
        (
            "vesper-sessions",
            BTreeSet::from(["vesper-config", "vesper-domain"]),
        ),
        (
            "vesper-acp",
            BTreeSet::from(["vesper-domain", "vesper-runtime"]),
        ),
        (
            // ADR 0010 (Tier C): the agent loop composes the runtime's
            // single-turn provider dispatch. Phase 1/2 stubs need domain +
            // provider types and the runtime registry; Phase 4 adds policy +
            // security when real executors arrive. PR-2/PR-3: the executor
            // consults the firewall (vesper-policy) and the sandbox caps
            // (vesper-security) before spawning.
            "vesper-agent",
            BTreeSet::from([
                "vesper-domain",
                "vesper-provider",
                "vesper-runtime",
                "vesper-testkit",
                "vesper-policy",
                "vesper-security",
            ]),
        ),
        (
            // Shared hosted services keep ACP and TUI on one Python-oracle
            // tool implementation while leaving protocol/provider concerns
            // at their composition boundaries. PR-4 adds the sandbox port
            // adapter: harness owns the concrete backend (vesper-sandbox)
            // and the config reader (vesper-config) behind the agent's
            // SandboxBackendPort, so neither host touches the backend type.
            "vesper-harness",
            BTreeSet::from([
                "vesper-agent",
                "vesper-policy",
                "vesper-checkpoints",
                "vesper-config",
                "vesper-domain",
                "vesper-mcp",
                "vesper-memory",
                // VRO-15 PR-9: optional deps behind the default-off `swarm`
                // feature; the WorkerPort adapter links provider types and
                // the swarm engine only when swarm is explicitly enabled.
                "vesper-provider",
                // dev-only: the deterministic synthetic provider backs the
                // swarm adapter's integration tests (test kind only).
                "vesper-provider-synthetic",
                "vesper-runtime",
                "vesper-swarm",
                "vesper-sandbox",
                "vesper-sessions",
                // VRO-14 PR-5: the WebService hosts the five opt-in web
                // tools over vesper-web's pure pipeline types.
                "vesper-web",
                // VB-PRD-001 Phase 2: the hosted Bridge tool service
                // (optional, default-off `bridge` feature) composes the
                // pure vesper-bridge core. No adapter, transport or I/O
                // here; every decision routes through the core gate.
                "vesper-bridge",
                "vesper-web-fetch",
            ]),
        ),
        (
            "agent-vesper-acp",
            BTreeSet::from([
                "vesper-provider-openai",
                "vesper-provider-xai",
                "vesper-acp",
                "vesper-agent",
                "vesper-auth",
                "vesper-cognition",
                "vesper-config",
                "vesper-domain",
                "vesper-harness",
                "vesper-policy",
                "vesper-provider",
                "vesper-provider-glm",
                "vesper-provider-synthetic",
                "vesper-runtime",
                "vesper-security",
                "vesper-sessions",
            ]),
        ),
        (
            // ADR 0010 (Tier C Phase 6): the TUI binary now composes the
            // multi-turn agent loop on top of the same shared registry that
            // powers the reasoning-override supervisor. The library stays
            // pure; the binary owns the spawn/drain plumbing.
            "agent-vesper-tui",
            BTreeSet::from([
                "vesper-provider-openai",
                "vesper-provider-xai",
                "vesper-agent",
                "vesper-auth",
                "vesper-checkpoints",
                "vesper-cognition",
                "vesper-domain",
                "vesper-harness",
                "vesper-mcp",
                "vesper-memory",
                "vesper-observability",
                "vesper-policy",
                "vesper-provider",
                "vesper-provider-glm",
                "vesper-provider-synthetic",
                "vesper-runtime",
                "vesper-security",
                "vesper-sessions",
                // VRO-17 PR-4: opt-in voice conversation mode behind the
                // default-off `voice-conversation` feature; the pure
                // voice core is agent-free (same class as vesper-web).
                "vesper-voice",
                // VRO-17 R3: the Natural Voice pack adapter behind the
                // default-off `voice-kokoro` feature (composition-only;
                // the heavy inference backend is lazily loaded from the
                // verified pack, never linked into the binary).
                "vesper-voice-kokoro",
            ]),
        ),
        (
            // VRO-15 PR-1: the swarm oracle's coordination paradigms as a
            // pure Rust layer. Structurally limited to domain + security
            // foundations; execution/inference/sandboxing are trait ports
            // fulfilled at the composition boundary, never here.
            "vesper-swarm",
            BTreeSet::from(["vesper-domain", "vesper-security"]),
        ),
        (
            // VB-PRD-001 Phase 1: the pure application-control core
            // (identities, capability manifest, operation/session state
            // machines, fenced leases, journal port, observations, error
            // contract). No I/O, clock or transport; adapters compose in
            // the hosted layer behind these ports.
            "vesper-bridge",
            BTreeSet::from(["vesper-domain", "vesper-security"]),
        ),
        ("xtask", BTreeSet::from(["vesper-testkit"])),
    ])
}

fn scan_production_sources(root: &Path) -> Result<(), String> {
    let shared_forbidden = [
        "agent_client_protocol",
        "agent-client-protocol",
        "ratatui",
        "reqwest",
        "rusqlite",
        "spikes/",
        "vesper-testkit",
        "vesper_provider_glm",
    ];
    for entry in fs::read_dir(root.join("crates")).map_err(|error| error.to_string())? {
        let crate_path = entry.map_err(|error| error.to_string())?.path();
        if !crate_path.is_dir() {
            continue;
        }
        if crate_path.file_name().and_then(|name| name.to_str()) == Some("vesper-testkit") {
            continue;
        }
        let mut files = Vec::new();
        collect_source_files(&crate_path.join("src"), &mut files)?;
        for file in files {
            let source = fs::read_to_string(&file).map_err(|error| error.to_string())?;
            if matches!(
                file.file_name().and_then(|value| value.to_str()),
                Some("lib.rs" | "main.rs")
            ) && !source.contains("#![forbid(unsafe_code)]")
            {
                return Err(format!(
                    "foundational Rust source tree {} does not inherit a crate-level unsafe ban",
                    file.display()
                ));
            }
            // ADR 0022 (VRO-13 PR-3): the sandbox supervisor binary is the
            // one production unit that must perform raw syscalls (unshare,
            // setgroups/setuid/setgid map writes, mount, pivot_root, execvp)
            // before any libc runtime state exists. Enforce its safety
            // discipline instead of the blanket lib-level ban: every unsafe
            // op must sit in an explicit unsafe block, and the file must
            // carry the ADR 0022 safety contract marker.
            if crate_path.file_name().and_then(|name| name.to_str()) == Some("vesper-sandbox")
                && matches!(
                    file.file_name().and_then(|value| value.to_str()),
                    Some("sandbox_init.rs")
                )
            {
                if !source.contains("#![deny(unsafe_op_in_unsafe_fn)]") {
                    return Err(format!(
                        "sandbox supervisor {} must deny unsafe_op_in_unsafe_fn (ADR 0022)",
                        file.display()
                    ));
                }
                if !source.contains("ADR-0022") {
                    return Err(format!(
                        "sandbox supervisor {} must cite ADR-0022 safety contract",
                        file.display()
                    ));
                }
            }
            let crate_name = crate_path.file_name().and_then(|name| name.to_str());
            // VRO-14 PR-3: the sandboxed fetch helper is the one production
            // unit besides the supervisor allowed to name an HTTP client —
            // it runs INSIDE the provisioned sandbox (the harness process
            // never links it into a fetch path; vesper-web itself stays
            // reqwest-free and pure).
            let forbidden: &[&str] = if crate_name == Some("vesper-web-fetch") {
                &[
                    "agent_client_protocol",
                    "agent-client-protocol",
                    "ratatui",
                    "rusqlite",
                    "spikes/",
                    "vesper-testkit",
                    "vesper_provider_glm",
                ]
            } else if matches!(
                crate_name,
                Some("vesper-provider-openai" | "vesper-provider-xai")
            ) {
                &[
                    "agent_client_protocol",
                    "agent-client-protocol",
                    "ratatui",
                    "rusqlite",
                    "spikes/",
                    "vesper-testkit",
                    "vesper_provider_glm",
                    "std::process",
                    "tokio::process",
                ]
            } else if crate_name == Some("vesper-provider-glm") {
                &[
                    "agent_client_protocol",
                    "agent-client-protocol",
                    "ratatui",
                    "rusqlite",
                    "spikes/",
                ]
            } else if crate_name == Some("vesper-acp") {
                &[
                    "ratatui",
                    "reqwest",
                    "rusqlite",
                    "spikes/",
                    "vesper_provider_glm",
                ]
            } else if crate_name == Some("vesper-runtime") {
                &[
                    "agent_client_protocol",
                    "agent-client-protocol",
                    "ratatui",
                    "reqwest",
                    "rusqlite",
                    "spikes/",
                    "vesper_provider_glm",
                ]
            } else if crate_name == Some("vesper-mcp") {
                &[
                    "agent_client_protocol",
                    "agent-client-protocol",
                    "ratatui",
                    "rusqlite",
                    "spikes/",
                    "vesper_provider_glm",
                ]
            } else if crate_name == Some("vesper-cognition") {
                // ADR 0015 (Stage 16): this is the only production crate
                // permitted to depend on SQLite. The `rusqlite` term is
                // intentionally NOT in this list (it's in `shared_forbidden`)
                // — we carve out the exception by allowing rusqlite while
                // forbidding every other foundational escape.
                &[
                    "agent_client_protocol",
                    "agent-client-protocol",
                    "ratatui",
                    "reqwest",
                    "spikes/",
                    "vesper_provider_glm",
                ]
            } else {
                &shared_forbidden
            };
            for term in forbidden {
                if source.contains(term) {
                    return Err(format!(
                        "forbidden foundational reference {term:?} in {}",
                        file.display()
                    ));
                }
            }
            if crate_path.file_name().and_then(|name| name.to_str()) == Some("vesper-domain")
                && file.extension().and_then(|value| value.to_str()) == Some("rs")
                && file.file_name().and_then(|value| value.to_str()) != Some("compatibility.rs")
            {
                for term in [
                    "std::fs",
                    "std::path",
                    "tokio",
                    "http::",
                    "sqlx",
                    "rusqlite",
                ] {
                    if source.contains(term) {
                        return Err(format!(
                            "I/O or runtime type {term:?} entered provider-neutral domain file {}",
                            file.display()
                        ));
                    }
                }
                for provider in ["GlmClient", "OpenAI", "Anthropic", "Gemini"] {
                    if source.contains(provider) {
                        return Err(format!(
                            "concrete provider name {provider:?} entered shared domain file {}",
                            file.display()
                        ));
                    }
                }
            }
            for line in source.lines().map(str::trim) {
                let looks_serializable_secret = line.starts_with("pub ")
                    && line.contains("String")
                    && ["api_key", "password", "access_token", "secret_value"]
                        .iter()
                        .any(|name| line.contains(name));
                if looks_serializable_secret {
                    return Err(format!(
                        "raw secret-shaped serializable field in {}: {line}",
                        file.display()
                    ));
                }
            }
        }
    }
    Ok(())
}

fn collect_source_files(path: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
    if path.is_file() {
        if matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("rs" | "toml")
        ) {
            output.push(path.to_path_buf());
        }
        return Ok(());
    }
    for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
        collect_source_files(&entry.map_err(|error| error.to_string())?.path(), output)?;
    }
    Ok(())
}

fn verify() -> Result<(), String> {
    let commands: &[&[&str]] = &[
        &["fmt", "--all", "--check"],
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ],
        &["test", "--workspace", "--all-features"],
        &["test", "--workspace", "--doc"],
    ];
    for arguments in commands {
        run("cargo", arguments)?;
    }
    fixtures_validate()?;
    fixtures_verify_index()?;
    fixtures_coverage(2)?;
    fixtures_coverage(3)?;
    fixtures_coverage(4)?;
    fixtures_coverage(5)?;
    contracts_verify()?;
    architecture()?;
    naming_guard(false)?;
    provider_glm_verify()?;
    runtime_verify()?;
    acp_verify()?;
    sessions_verify()?;
    acceptance_verify()
}

fn msrv() -> Result<(), String> {
    run(
        "rustup",
        &[
            "run",
            "1.88.0",
            "cargo",
            "test",
            "--workspace",
            "--all-features",
        ],
    )
}

fn run(program: &str, arguments: &[&str]) -> Result<(), String> {
    println!("running: {program} {}", arguments.join(" "));
    let status = Command::new(program)
        .args(arguments)
        .current_dir(repository_root())
        .status()
        .map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} exited with {status}"))
    }
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    dependencies: Vec<Dependency>,
    #[serde(default)]
    features: BTreeMap<String, Vec<String>>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    #[serde(default)]
    optional: bool,
    rename: Option<String>,
    #[serde(rename = "req")]
    requirement: String,
    source: Option<String>,
    path: Option<PathBuf>,
    kind: Option<String>,
}

/// External coding agents and CI run the same runtime/evaluator regression gate.
/// Exact names make a deleted, renamed, ignored, or zero-match case a failure.
fn acceptance_verify() -> Result<(), String> {
    let cases: Vec<(&str, &[&str], &str)> = vec![
        (
            "vesper-harness",
            &["--lib"],
            "host_resources::native_backend::tests::native_observation_cancellation_interrupts_a_stalled_job_probe",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "host_resources::native_backend::tests::windows_job_observations_reject_missing_malformed_and_inconsistent_values",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "host_resources::native_backend::tests::native_capacity_and_owned_process_observation_is_real",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "host_resources::native_backend::tests::native_owned_descendants_are_observed_and_reaped",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "host_resources::native_backend::tests::native_tree_counts_descendants_excludes_siblings_and_refuses_cycles",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "host_resources::tests::windows_job_bounds_preserve_physical_capacity_and_refuse_unsafe_admission",
        ),
        (
            "agent-vesper-tui",
            &["--bin", "agent-vesper-tui"],
            "tests::native_release_controls_bypass_pending_account_discovery",
        ),
        (
            "vesper-testkit",
            &["--lib"],
            "fixture::tests::invocation_fixture_roots_do_not_follow_cached_build_paths",
        ),
        (
            "xtask",
            &["--test", "cached_workspace"],
            "cached_binary_uses_each_invoking_workspace_and_fixture_corpus",
        ),
        (
            "xtask",
            &["--test", "cached_workspace"],
            "cached_binary_outside_workspace_refuses_build_directory_fallback",
        ),
        (
            "agent-vesper-tui",
            &["--bin", "agent-vesper-tui"],
            "tests::xai_controls_reach_generic_turn_configuration",
        ),
        (
            "vesper-agent",
            &["--test", "acceptance_policy"],
            "every_required_host_needs_its_own_evidence",
        ),
        (
            "vesper-agent",
            &["--test", "acceptance_policy"],
            "unit_tests_cannot_substitute_for_native_tests",
        ),
        (
            "vesper-agent",
            &["--test", "acceptance_policy"],
            "changed_source_contract_checks_platform_or_test_invalidates",
        ),
        (
            "vesper-agent",
            &["--test", "acceptance_policy"],
            "duplicate_unknown_and_wrong_command_receipts_refuse",
        ),
        (
            "vesper-agent",
            &["--test", "acceptance_policy"],
            "omitted_source_and_removed_scenario_checks_refuse",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "acceptance::tests::native_agent_cannot_stop_early_then_repairs_and_earns_real_evidence",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "acceptance::tests::automatic_enrollment_remembers_prd_and_requires_real_evidence_in_the_same_turn",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "acceptance::tests::automatic_enrollment_rejects_missing_external_and_forged_scope_without_saving",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "acceptance::tests::automatic_scope_review_refusal_and_cancellation_do_not_remember_a_path",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "acceptance::tests::delegated_finish_and_empty_deleted_or_replaced_plans_cannot_certify_parent",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "acceptance::tests::wrong_platform_and_verification_timeout_fail_closed",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "acceptance::tests::reviewer_unavailable_or_without_source_inspection_cannot_approve",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "acceptance::tests::explicit_scope_revision_and_resume_preserve_history_but_never_import_verification",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "acceptance::tests::independent_reexamination_resolves_spurious_finding_without_forged_override",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "acceptance::tests::cancelled_and_exhausted_turns_publish_incomplete_history",
        ),
        (
            "agent-vesper-acp",
            &["--test", "acceptance_controls"],
            "native_acceptance_controls_persist_only_explicit_activation",
        ),
        (
            "agent-vesper-tui",
            &["--bin", "agent-vesper-tui"],
            "acceptance_host::tests::acceptance_native_settings_show_scope_and_save_cancel",
        ),
        (
            "agent-vesper-tui",
            &["--bin", "agent-vesper-tui"],
            "tests::acceptance_event_cannot_be_overridden_by_completed_plan",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "acceptance::tests::verification_never_bypasses_shell_permission_or_command_firewall",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "acceptance::tests::publication_rechecks_live_source_after_snapshot_review",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "acceptance::tests::project_rules_are_frozen_and_runtime_configuration_is_snapshotted",
        ),
        (
            "vesper-agent",
            &["--lib"],
            "vro::react::tests::acceptance_parent_controls_reach_react_tools",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "acceptance::tests::saved_activation_restores_unverified_scope_and_cannot_disable_an_active_contract",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::directives_are_typed_and_back_off_without_model_step_polling",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::a_long_running_platform_matrix_is_not_active_repair_stagnation",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::missing_runs_cannot_reuse_a_stale_running_matrix_to_avoid_timeout",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::matching_published_release_resumes_closeout_without_new_epoch_or_tag",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::completed_version_does_not_claim_a_different_objective_was_released",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::published_release_keeps_worker_until_post_release_closeout",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::published_closeout_finishes_report_without_replaying_release_mutations",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::repair_evidence_watchdog_ignores_streaming_and_repeated_tool_results",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::repair_evidence_deadline_and_native_verification_are_distinct",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::repair_dispatch_budget_survives_restart_without_a_focused_proof",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::live_progress_uses_durable_milestones_without_losing_case_counts",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::repair_registry_stops_repeated_observations_for_each_provider_context",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_closeout::tests::closeout_registry_uncertain_write_never_replays",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_closeout::tests::closeout_registry_preserves_concurrent_edit_and_rejects_false_write_receipt",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_closeout::tests::closeout_later_main_has_a_distinct_report_without_overwriting_publication",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_closeout::tests::closeout_report_restart_is_idempotent_and_preserves_user_edits",
        ),
        (
            "agent-vesper-tui",
            &["--bin", "agent-vesper-tui"],
            "tests::release_final_receipt_is_not_suppressed_by_last_active_milestone",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::partial_matrix_blocks_retry",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::missing_initial_workflow_runs_remain_an_active_matrix_wait",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::repeated_incomplete_matrix_refreshes_do_not_spend_repair_watchdog_actions",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::worker_keeps_matrix_and_classification_states_controller_owned",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::remote_poll_wait_is_responsive_to_cancellation",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::verified_repair_is_the_only_path_to_one_full_gate_retry",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::verified_repair_batch_records_every_failure_family_before_retry",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::paused_epoch_reopens_with_exact_identity_and_resumes_through_remote_refresh",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::green_release_then_red_closeout_keeps_publication_immutable",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::same_recoverable_epoch_resumes_from_natural_language_without_manual_command",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::obsolete_diagnosing_epoch_is_preserved_and_newer_candidate_is_admitted",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::canonical_objective_supersedes_historical_active_prerelease_without_clarification",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::unrelated_active_release_asks_one_human_clarification_without_mutation",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::different_release_target_with_pushed_candidate_requires_clarification",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::tui_natural_release_retains_registered_controller_through_local_gate_progress",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::production_orchestrator_reaches_publication_only_through_settled_gates",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::release_worker_cancel_restart_process_acceptance",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::resource_governor_defer_keeps_local_epoch_resumable_without_source_failure",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_progress_tests::persisted_hierarchy_uses_typed_gate_and_job_states_without_percentages",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::trusted_acceptance_output_updates_only_the_active_acceptance_subtask",
        ),
        (
            "agent-vesper-tui",
            &["--lib"],
            "commands::tests::release_routes_to_the_controller_instead_of_a_model_workflow",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::background_controller_waits_then_publishes_without_continue_prompts",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::denied_host_permission_prevents_every_release_side_effect",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::an_unsettled_mutation_is_never_replayed_on_restart",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::publication_watch_budget_survives_reload_and_read_recovery",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::publication_read_timeout_continues_same_candidate_without_another_tag",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::publication_read_watchdog_recovery_is_persisted_and_bounded",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::publication_read_recovery_never_retries_cancellation_denials_or_invalid_evidence",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::same_tagged_publication_request_resumes_without_replacing_epoch",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::publication_requires_all_fourteen_nonempty_uploaded_assets",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::persisted_cancellation_reaches_a_worker_in_another_host",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::passing_error_module_tests_cannot_hide_the_real_causal_failure",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::dependency_errors_and_unrecognized_platform_logs_classify_truthfully",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::settled_windows_metadata_cannot_hide_version_assertion",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::settled_python_fixture_exceptions_admit_source_diagnosis",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::python_fixture_classification_preserves_owner_action_and_uncertainty",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::settled_dependency_download_precedes_downstream_missing_binary_panic",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::settled_package_index_timeout_retains_observed_setup_context",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::remote_operation_cancellation_is_not_local_user_cancellation",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::github_account_execution_restriction_requires_owner_action",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::native_worker_stops_owner_action_and_uncertain_causes_before_repair_permission",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::read_only_health_checks_do_not_request_source_repair_permissions",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::github_failed_job_retry_requires_exact_admission_scope",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::release_read_ports_share_worker_cancellation_before_dispatch",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::required_skipped_job_never_opens_tag_admission",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::last_green_missing_or_skipped_matching_job_preserves_unknown_context",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::missing_runner_log_uses_only_owned_failure_annotations",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::published_docs_red_repair_then_other_platform_red_stops_speculation",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::stale_writer_cannot_overwrite_cancelled_checkpoint",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::rejected_repair_event_does_not_partially_mutate_the_record",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::controller_owner_is_exclusive_across_independent_ledger_handles",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::cancelled_checkpoint_can_be_explicitly_resumed_without_erasing_counters",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::old_attempt_cannot_satisfy_a_reserved_infrastructure_retry",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::github_adapter_uses_exact_attempt_and_main_push_provenance",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::published_release_has_a_distinct_main_epoch_and_archived_history",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::quoted_credentials_basic_auth_and_signed_urls_are_redacted",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::arbitrary_successful_shell_commands_are_not_focused_proof",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::focused_test_proof_requires_a_nonzero_executed_pass",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::focused_proof_identifies_rust_panic_with_numeric_thread_id",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::repair_cargo_requires_governor_and_rejects_policy_overrides",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::repair_command_signal_watcher_settles_on_unwind",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::native_firewall_denial_cannot_be_bypassed_by_a_release_admission",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::local_failure_retains_the_causal_error_for_bounded_diagnosis",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::native_repair_patch_keeps_version_seed_and_includes_new_regressions",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::native_version_bump_keeps_all_workspace_dependency_pins_resolvable",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::colored_ci_logs_cannot_hide_credentials_or_causal_errors",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::publication_checksum_content_must_match_both_server_digests",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::github_command_echo_is_not_the_runtime_causal_error",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::repair_preserves_provider_owned_hosted_tool_selections",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::repair_iteration_budget_survives_disabled_host_cap",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::repair_continues_unplanned_segments_for_two_provider_fixtures",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::repair_terminal_diagnostics_identify_the_safety_stop",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "host_resources::tests::expensive_slot_release_unlocks_inherited_file_description",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::version_preparation_rollback_routes_to_repair_and_pins_promoted_patch",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::repair_initial_provider_retry_is_bounded_and_never_replays_tools",
        ),
        (
            "vesper-provider-openai",
            &["--lib"],
            "http_error_tests::transient_http_rejections_admit_only_before_output_retry_in_both_modes",
        ),
        (
            "vesper-provider-openai",
            &["--lib"],
            "http_error::retry_tests::transient_retry_respects_server_delay_and_refuses_malformed_headers",
        ),
        (
            "vesper-agent",
            &["--test", "agent_loop"],
            "role_ceiling_bounds_planned_continuation",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::repair_worker_cannot_create_unadmitted_release_tags",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::repair_factory_executes_real_tools_for_two_provider_fixtures",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::isolated_agent_repair_verifies_and_promotes_one_patch_for_two_provider_fixtures",
        ),
        (
            "agent-vesper-tui",
            &["--lib"],
            "ui::tests::registered_release_task_replaces_ready_and_no_active_tasks",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_recovery::tests::terminal_cancelled_runner_job_uses_owned_failure_annotations",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "release_executor::tests::local_failure_receipt_preserves_late_stdout_cause_and_redacts_secrets",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "host_resources::tests::reported_normal_threshold_includes_swap_and_owned_tree_burst_margins",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "host_resources::tests::swap_growth_window_rejects_subsecond_rate_amplification",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "host_resources::tests::swap_growth_window_preserves_sustained_pressure_and_critical_signals",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "host_resources::tests::target_cache_scan_tolerates_disappearing_entries",
        ),
        (
            "vesper-harness",
            &["--lib"],
            "host_resources::tests::target_cache_scan_preserves_non_missing_errors",
        ),
    ];
    #[cfg(unix)]
    let mut cases = cases;
    #[cfg(target_os = "linux")]
    cases.extend([
        (
            "vesper-harness",
            &["--lib"] as &[&str],
            "host_resources::tests::cgroup_v2_discovery_uses_the_strictest_ancestor_limit_and_its_usage",
        ),
        (
            "vesper-harness",
            &["--lib"] as &[&str],
            "host_resources_constrained_tests::constrained_memory_acceptance_withholds_expensive_cargo_before_spawn",
        ),
        (
            "vesper-harness",
            &["--lib"] as &[&str],
            "host_resources_constrained_tests::constrained_disk_acceptance_preserves_target_cache_and_withholds_gate",
        ),
    ]);
    #[cfg(target_os = "linux")]
    cases.push((
        "agent-vesper-acp",
        &["--test", "release_resource_governor"],
        "acp_completed_release_delivers_final_receipt_without_provider_or_new_release",
    ));
    #[cfg(target_os = "linux")]
    cases.push((
        "agent-vesper-acp",
        &["--test", "release_resource_governor"],
        "acp_conflicting_published_target_returns_clarification_without_observing_old_worker",
    ));
    #[cfg(target_os = "linux")]
    cases.push((
        "agent-vesper-acp",
        &["--test", "release_resource_governor"],
        "acp_process_release_status_uses_governor_for_every_cargo_path",
    ));
    #[cfg(target_os = "linux")]
    cases.push((
        "agent-vesper-acp",
        &["--test", "release_resource_governor"],
        "acp_natural_release_inherits_session_permission_and_governor",
    ));
    #[cfg(target_os = "linux")]
    cases.extend([
        (
            "vesper-harness",
            &["--lib"] as &[&str],
            "release_executor::tests::repair_cargo_uses_managed_cache_and_bounded_environment",
        ),
        (
            "agent-vesper-acp",
            &["--test", "release_resource_governor"] as &[&str],
            "controlled_cargo_metadata_emits_valid_json",
        ),
    ]);
    #[cfg(unix)]
    cases.push((
        "agent-vesper-tui",
        &["--test", "rrc_terminal_pty"],
        "rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty",
    ));
    #[cfg(unix)]
    cases.push((
        "agent-vesper-tui",
        &["--test", "openai_startup_responsiveness"],
        "native_release_admission_does_not_wait_for_account_model_discovery",
    ));
    cases.extend([
        (
            "vesper-harness",
            &["--lib"] as &[&str],
            "release_executor::tests::only_newly_classified_local_gate_errors_continue_to_repair",
        ),
        (
            "vesper-harness",
            &["--lib"] as &[&str],
            "release_executor::tests::local_repair_dispatch_errors_are_retained_as_liveness_failures",
        ),
        (
            "agent-vesper-tui",
            &["--bin", "agent-vesper-tui"] as &[&str],
            "tests::release_repair_uses_selected_model_and_effort_before_account_discovery",
        ),
        (
            "agent-vesper-acp",
            &["--lib"] as &[&str],
            "tests::release_repair_configuration_uses_session_model_and_provider_controls",
        ),
        (
            "agent-vesper-tui",
            &["--lib"] as &[&str],
            "ui::tests::recoverable_release_is_not_rendered_as_running_or_ready",
        ),
    ]);
    let started = std::time::Instant::now();
    for (index, (package, target, name)) in cases.iter().enumerate() {
        let output = Command::new("cargo")
            .current_dir(repository_root())
            .args([
                "test",
                "--locked",
                "--offline",
                "--color",
                "never",
                "--all-features",
                "-p",
                package,
            ])
            .args(*target)
            .args([name, "--", "--exact", "--test-threads=1"])
            .output()
            .map_err(|error| format!("acceptance case {name}: {error}"))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !output.status.success()
            || !stdout
                .lines()
                .any(|line| line.trim() == format!("test {name} ... ok"))
            || !stdout.contains("test result: ok. 1 passed; 0 failed; 0 ignored;")
        {
            return Err(format!(
                "acceptance case did not execute and pass: {name}\n{stdout}\n{}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        println!("acceptance verified: {name}");
        // This controller-owned marker is intentionally machine-readable: the
        // RRC admits it only while the named acceptance gate owns the child.
        // The denominator is the current fixed case list, never a renderer
        // estimate or a count inferred from Cargo output.
        println!(
            "acceptance progress: {}/{} — {name}",
            index + 1,
            cases.len()
        );
    }
    println!(
        "Acceptance regression gate: {} exact cases passed in {} ms. Offline fixture model cost: zero; live-model effectiveness is not measured.",
        cases.len(),
        started.elapsed().as_millis()
    );
    Ok(())
}

fn acceptance_mutations() -> Result<(), String> {
    fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
        fs::create_dir_all(destination).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name();
            if matches!(
                name.to_str(),
                Some("target" | ".git" | "node_modules" | ".agent-vesper")
            ) {
                continue;
            }
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_symlink() {
                return Err("mutation source symlink refused".into());
            }
            if kind.is_dir() {
                copy_tree(&entry.path(), &destination.join(name))?;
            } else if kind.is_file() {
                fs::copy(entry.path(), destination.join(name)).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    let root = repository_root();
    let temp = tempfile::tempdir().map_err(|e| e.to_string())?;
    for directory in ["crates", "apps", "xtask", "fixtures", ".cargo", "skills"] {
        copy_tree(&root.join(directory), &temp.path().join(directory))?;
    }
    for file in ["Cargo.toml", "Cargo.lock"] {
        fs::copy(root.join(file), temp.path().join(file)).map_err(|e| e.to_string())?;
    }
    let source_path = temp.path().join("crates/vesper-agent/src/acceptance.rs");
    let original = fs::read_to_string(&source_path).map_err(|e| e.to_string())?;
    let mutations = [
        (
            "receipt.source_digest != source_digest",
            "false",
            "changed_source_contract_checks_platform_or_test_invalidates",
        ),
        (
            "if state != AcceptanceState::Verified {",
            "if false {",
            "all_nonpositive_states_refuse_completion",
        ),
    ];
    for (needle, replacement, name) in mutations {
        if original.matches(needle).count() != 1 {
            return Err(format!(
                "mutation location changed: {needle}; update the explicit gate"
            ));
        }
        for mutated in [false, true] {
            fs::write(
                &source_path,
                if mutated {
                    original.replacen(needle, replacement, 1)
                } else {
                    original.clone()
                },
            )
            .map_err(|e| e.to_string())?;
            let output = Command::new("cargo")
                .current_dir(temp.path())
                .env("CARGO_TARGET_DIR", root.join("target/acceptance-mutations"))
                .args([
                    "test",
                    "--offline",
                    "--locked",
                    "--color",
                    "never",
                    "-p",
                    "vesper-agent",
                    "--test",
                    "acceptance_policy",
                    name,
                    "--",
                    "--exact",
                ])
                .output()
                .map_err(|e| e.to_string())?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let expected = if mutated { "FAILED" } else { "ok" };
            if output.status.success() == mutated
                || !stdout
                    .lines()
                    .any(|line| line.trim() == format!("test {name} ... {expected}"))
            {
                return Err(format!(
                    "mutation was not killed by the expected assertion (compile errors do not count): {name}, mutated={mutated}\n{stdout}\n{}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        }
        println!("Acceptance mutation killed: {name} ({needle})");
    }
    Ok(())
}
