//! Explicit, non-shipped routing diagnostic. Dry-run is the default.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    sync::Arc,
};
use vesper_memory::routing_quality::{RoutingMode, RoutingOptions};
use vesper_memory::{SkillRoutingQuery, SkillStore};
#[derive(Deserialize)]
struct Corpus {
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    id: String,
    context: Context,
    prompt: String,
}
#[derive(Deserialize)]
struct Context {
    unavailable_tools: Vec<String>,
    denied_permissions: Vec<String>,
}
fn manifest(
    directory: &std::path::Path,
    output: &mut BTreeMap<std::path::PathBuf, Vec<u8>>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            manifest(&entry.path(), output)?;
        } else if entry.file_type()?.is_file() {
            output.insert(
                entry.path(),
                Sha256::digest(std::fs::read(entry.path())?).to_vec(),
            );
        }
    }
    Ok(())
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let output = match args.as_slice() {
        [] => None,
        [flag, output] if flag == "--execute-live" => Some(output),
        _ => return Err("Usage: skill_routing_eval [--execute-live NEW-OUTPUT.jsonl]".into()),
    };
    let bytes = include_bytes!("../../../docs/foundation/skill-routing-independent-corpus.json");
    let hash = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if hash != "79ef89a4934a0e2c61aa08d4e2de6a3efd0272c18f1ee3e9507271d5c6111275" {
        return Err("Frozen corpus digest changed".into());
    }
    let corpus: Corpus = serde_json::from_slice(bytes)?;
    if corpus.cases.len() != 205 {
        return Err("Expected 205 frozen cases".into());
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../skills")
        .canonicalize()?;
    let mut before = BTreeMap::new();
    manifest(&root, &mut before)?;
    let store = SkillStore::open(&root)?;
    let mut options = RoutingOptions::default();
    options.preferences.mode = RoutingMode::Enhanced;
    options.preferences.model_assistance = true;
    let tools: BTreeSet<String> = [
        "read_file",
        "write_file",
        "run_command",
        "delegate_task",
        "read_skill",
        "web_search",
        "web_fetch",
        "web_reader",
        "browser_ui",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let outcomes = BTreeMap::new();
    let mut file = output
        .map(|path| {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
        })
        .transpose()?;
    let factory = if file.is_some() {
        let registry = Arc::new(vesper_runtime::ProviderRegistry::new());
        registry
            .register(vesper_provider_glm::GlmFactory::default())
            .await?;
        let configuration = vesper_provider_glm::GlmFactory::default_configuration();
        let model =
            vesper_provider_glm::GlmCatalog::find("glm-5.3").ok_or("GLM-5.3 unavailable")?;
        let capacity = vesper_provider_glm::GlmCatalog::entries()
            .iter()
            .find(|entry| entry.id() == "glm-5.3")
            .ok_or("Missing capacity")?
            .context_tokens();
        Some(vesper_harness::WorkerFactory::new(
            registry,
            vesper_agent::AgentLoopConfig {
                provider_id: configuration.provider_id.clone(),
                provider_configuration: configuration,
                model: model.model,
                context_window_tokens: capacity,
                native_compaction: vesper_agent::NativeCompactionPolicy::Disabled,
                hosted_tools: Vec::new(),
                system_instructions: vec![],
                workspace_roots: vec![],
                max_tool_iterations: 1,
                firewall: None,
                sandbox: None,
            },
        ))
    } else {
        None
    };
    if let Some(file) = file.as_mut() {
        writeln!(
            file,
            "{}",
            serde_json::json!({"corpus_sha256":hash,"provider":"zai","model":"glm-5.3","plan":"coding","reasoning":"enabled","cases":205,"kind":"inspected-corpus-regression","library_files":before.len()})
        )?;
    }
    let mut prepared_count = 0;
    for case in corpus.cases {
        let mut available = tools.clone();
        let unsupported: Vec<_> = case
            .context
            .unavailable_tools
            .iter()
            .filter(|tool| !available.contains(*tool))
            .cloned()
            .collect();
        for tool in &case.context.unavailable_tools {
            available.remove(tool);
        }
        let query = SkillRoutingQuery {
            prompt: &case.prompt,
            explicit_skill: None,
            available_tools: &available,
            platform: std::env::consts::OS,
            outcome_adjustments: &outcomes,
        };
        let mut report = store.orchestrate_with_options(&query, &options);
        let mut receipt = None;
        let offered = report
            .prepared_selection
            .as_ref()
            .map(|prepared| {
                prepared
                    .offers()
                    .iter()
                    .map(|offer| offer.id.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if let Some(prepared) = report.prepared_selection.take() {
            prepared_count += 1;
            if let Some(factory) = &factory {
                let measured = vesper_harness::skill_model_selector::select(
                    factory,
                    &prepared,
                    &case.prompt,
                    Arc::new(vesper_runtime::RuntimeCancellation::new()),
                )
                .await;
                match &measured.decision {
                    Ok(decision) => {
                        report =
                            store.complete_model_selection(&query, &options, &prepared, decision)
                    }
                    Err(reason) => {
                        let mut lexical = options.clone();
                        lexical.preferences.model_assistance = false;
                        report = store.orchestrate_with_options(&query, &lexical);
                        report.routing_trace.reason = format!("{reason}; lexical routing used");
                    }
                }
                receipt = Some(
                    serde_json::json!({"decision": measured.decision, "elapsed_ms":measured.elapsed_ms,"usage":measured.usage}),
                );
            }
        }
        if let Some(file) = file.as_mut() {
            writeln!(
                file,
                "{}",
                serde_json::json!({"id":case.id,"offered":offered,"selected":report.selected_names(),"reason":report.routing_trace.reason,"selector":receipt,"unsupported_tools":unsupported,"unsupported_permissions":case.context.denied_permissions})
            )?;
            file.flush()?;
        }
    }
    let mut after = BTreeMap::new();
    manifest(&root, &mut after)?;
    if before != after {
        return Err("Skill library changed during evaluation".into());
    }
    println!(
        "{}: 205 cases, {prepared_count} prepared, {} source files unchanged",
        if output.is_some() {
            "LIVE regression finished"
        } else {
            "DRY RUN; zero provider calls"
        },
        before.len()
    );
    Ok(())
}
