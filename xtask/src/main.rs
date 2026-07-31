#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use clap::{Parser, Subcommand};
use serde::Deserialize;
use vesper_testkit::{FixtureCorpus, fixture_root};

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
    let result = match Cli::parse().command {
        Task::Verify => verify(),
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
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must be under repository root")
        .to_path_buf()
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
                "acp.slash-command" => Some("only real Stage 4 slash commands are accepted; the full catalog is deferred"),
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
        for dependency in &package.dependencies {
            let workspace_target = dependency
                .path
                .as_ref()
                .and_then(|_| names.values().find(|name| *name == &dependency.name));
            if let Some(target) = workspace_target
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
            ) {
                return Err(format!(
                    "SQLite dependency {} is prohibited during Stage 5",
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

fn allowed_dependencies() -> BTreeMap<&'static str, BTreeSet<&'static str>> {
    BTreeMap::from([
        ("vesper-domain", BTreeSet::new()),
        ("vesper-security", BTreeSet::new()),
        (
            "vesper-config",
            BTreeSet::from(["vesper-domain", "vesper-security"]),
        ),
        ("vesper-provider", BTreeSet::from(["vesper-domain"])),
        (
            "vesper-policy",
            BTreeSet::from(["vesper-domain", "vesper-security"]),
        ),
        (
            "vesper-testkit",
            BTreeSet::from([
                "vesper-domain",
                "vesper-provider",
                "vesper-security",
                "vesper-policy",
                "vesper-config",
            ]),
        ),
        (
            "vesper-provider-glm",
            BTreeSet::from([
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
            "agent-vesper-acp",
            BTreeSet::from([
                "vesper-acp",
                "vesper-config",
                "vesper-domain",
                "vesper-provider",
                "vesper-provider-glm",
                "vesper-provider-synthetic",
                "vesper-runtime",
                "vesper-sessions",
            ]),
        ),
        (
            "agent-vesper-tui",
            BTreeSet::from([
                "vesper-domain",
                "vesper-provider",
                "vesper-provider-glm",
                "vesper-provider-synthetic",
                "vesper-runtime",
            ]),
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
            let crate_name = crate_path.file_name().and_then(|name| name.to_str());
            let forbidden: &[&str] = if crate_name == Some("vesper-provider-glm") {
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
    provider_glm_verify()?;
    runtime_verify()?;
    acp_verify()?;
    sessions_verify()
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
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    #[serde(rename = "req")]
    requirement: String,
    source: Option<String>,
    path: Option<PathBuf>,
    kind: Option<String>,
}
