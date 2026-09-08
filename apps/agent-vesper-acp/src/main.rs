#![forbid(unsafe_code)]

use std::process::ExitCode;

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    if let Some(success) = vesper_harness::web_settings::handle_setup_flag().await {
        return if success {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    // Meta flags (`--version`/`--help`) are handled BEFORE the ACP server
    // boots so the installer's `agent-vesper-acp --version` check works and
    // never tries to start the stdio protocol. Mirrors the original Python
    // `native-glm-acp --version` UX.
    if let Some(code) = handle_meta_flags() {
        return code;
    }
    if let Some(code) = handle_openai_auth_flags().await {
        return code;
    }
    if let Some(code) = handle_auth_flags() {
        return code;
    }
    configure_stderr_tracing();
    // VRO-13 PR-2: resolve the process-global firewall exactly once, before
    // any session or engine can observe it. Both hosts must call this at
    // boot; the first resolution wins and is immutable for the process.
    let _ = vesper_policy::firewall::holder::install_from_env();
    // VRO-13 PR-4: resolve the process-global sandbox route once, before
    // any session or engine can observe it. Same first-resolution-wins
    // contract as the firewall: `AGENT_VESPER_SANDBOX=docker|namespaces|off`
    // plus the project scope's `[sandbox]` demand from
    // `.agent-vesper/config.toml`. No demand → holder stays `None` and the
    // executor keeps the byte-identical legacy path.
    let _ = vesper_harness::sandbox_backend::holder::install_from_env();
    // VRO-14 PR-5: resolve the opt-in [web] tool scope once at boot —
    // byte-identical derivation to the TUI's boot (same reader, same
    // defaults, same enabled gate), so the cross-host parity contract
    // holds by construction rather than by convention.
    let _ = vesper_harness::web_service::holder::install_from_env();
    // VRO-13 PR-5: resolve the WorkspaceScope once at boot (identity, layers,
    // per-scope skills, firewall composition) — identical derivation in both
    // hosts, so the TUI and ACP resolve identical ScopeIds for one directory.
    // Scope resolution is strictly a host-boot concern; the loop layer never
    // sees scopes. Resolution failure degrades gracefully (un-scoped
    // defaults), never blocking stdio protocol startup.
    let scope_stamp_policy =
        if std::env::var("AGENT_VESPER_ENABLE_SCOPE_STAMP").is_ok_and(|value| value == "1") {
            vesper_agent::vro::scope::StampPolicy::Write
        } else {
            vesper_agent::vro::scope::StampPolicy::ReadOnly
        };
    if let Err(error) =
        vesper_harness::scope_holder::holder::install_from_env_with_stamp_policy(scope_stamp_policy)
    {
        eprintln!("vesper: workspace scope resolution failed ({error}); using unscoped defaults");
    }
    let outcome = match provider_from_argv() {
        Some(provider) => agent_vesper_acp::boot(&provider).await,
        None => agent_vesper_acp::run().await,
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => {
            eprintln!("agent-vesper-acp terminated with a safe startup or protocol error");
            ExitCode::FAILURE
        }
    }
}

/// Handles `--version` / `-V` and `--help` / `-h`. Returns `Some(exit_code)`
/// when a meta flag was handled (the program should exit with that code); the
/// ACP server must not start in that case.
///
/// `--version` is required by the installers in `scripts/`. The output is a
/// single stable line; stdout purity is unaffected because no ACP session is
/// running when these flags are used.
fn handle_meta_flags() -> Option<ExitCode> {
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--version" | "-V" => {
                // Stdout is reserved for ACP JSON-RPC (composition contract);
                // route meta output to stderr so the server's stdout stays pure.
                eprintln!("agent-vesper-acp {}", env!("CARGO_PKG_VERSION"));
                return Some(ExitCode::SUCCESS);
            }
            "--help" | "-h" => {
                print_help();
                return Some(ExitCode::SUCCESS);
            }
            _ => {}
        }
    }
    None
}

fn print_help() {
    eprintln!("agent-vesper-acp {}", env!("CARGO_PKG_VERSION"));
    eprintln!("ACP-protocol-v1 stdio server for Z.ai GLM models.");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("    agent-vesper-acp [OPTIONS]");
    eprintln!();
    eprintln!("OPTIONS:");
    eprintln!(
        "        --provider <glm|zai|lmstudio|openai> Initial provider (default: AGENT_VESPER_PROVIDER or glm)"
    );
    eprintln!(
        "                                          All adapters stay registered; switch via the footer Provider picker"
    );
    eprintln!("    -V, --version                        Print version and exit");
    eprintln!("    -h, --help                           Print this help and exit");
    eprintln!();
    eprintln!("ENVIRONMENT:");
    eprintln!(
        "    ZAI_API_KEY                          Z.ai API key (required for the GLM provider)"
    );
    eprintln!(
        "    LMSTUDIO_API_KEY                     LM Studio API key (optional; local servers usually need none)"
    );
    eprintln!(
        "    AGENT_VESPER_PROVIDER                Default provider (glm|zai|lmstudio|openai)"
    );
    eprintln!(
        "    AGENT_VESPER_LOG                     Tracing filter (default: warn, stderr only)"
    );
    eprintln!(
        "    AGENT_VESPER_ENABLE_CHECKPOINTS      Opt in to /checkpoint, /rollback, /undo, /sessions, /lineage (default: off)"
    );
    eprintln!("    AGENT_VESPER_VRO_ENABLED=1           Opt in to reasoning orchestration (VRO)");
    eprintln!(
        "    --setup                               Store the selected provider API key without printing it"
    );
    eprintln!(
        "    --provider openai --login              Native ChatGPT subscription device sign-in"
    );
    eprintln!(
        "    --provider openai --logout             Sign out locally; disable environment-key fallback"
    );
    eprintln!(
        "    OPENAI_API_KEY                        Optional OpenAI API-key override in API mode"
    );
    eprintln!(
        "    --setup-web-driver                    Verify/import the bundled web driver (no provider calls)"
    );
    eprintln!("    --check-auth                          Check configured Z.ai credentials");
}

/// Handles the explicit terminal authentication setup path. Credentials are
/// accepted from the environment or one stdin line and are written only
/// through the provider's atomic, user-private credential store.
async fn handle_openai_auth_flags() -> Option<ExitCode> {
    use std::sync::Arc;
    use vesper_provider::ProviderCredentialPort;
    let provider = provider_from_argv().or_else(|| std::env::var("AGENT_VESPER_PROVIDER").ok());
    if provider.as_deref() != Some("openai") {
        return None;
    }
    let args: Vec<String> = std::env::args().collect();
    let factory = vesper_provider_openai::OpenAiFactory::default();
    let result = if args.iter().any(|s| s == "--login") {
        eprintln!("OpenAI ChatGPT subscription sign-in. No Codex installation is required.");
        let cancel = Arc::new(vesper_runtime::RuntimeCancellation::new());
        let login=factory.device_login(cancel.clone(),Arc::new(|url,code|eprintln!("Open {url}\nEnter one-time code: {code}\nOnly continue if you started this login. Ctrl+C cancels.")));
        tokio::pin!(login);
        tokio::select! {result=&mut login=>result,_=tokio::signal::ctrl_c()=>{cancel.cancel();login.await}}
    } else if args.iter().any(|s| s == "--logout") {
        tokio::task::spawn_blocking(move || factory.logout())
            .await
            .unwrap_or(Err(vesper_provider::CredentialError::Failed))
    } else if args.iter().any(|s| s == "--check-auth") {
        let present = tokio::task::spawn_blocking(move || factory.credential_present()).await;
        return Some(if matches!(present, Ok(Ok(true))) {
            eprintln!("OpenAI credentials are configured.");
            ExitCode::SUCCESS
        } else {
            eprintln!("OpenAI credentials are not configured.");
            ExitCode::FAILURE
        });
    } else if args.iter().any(|s| s == "--setup") {
        match std::env::var("OPENAI_API_KEY") {
            Ok(key) => tokio::task::spawn_blocking(move || factory.store_credential(&key))
                .await
                .unwrap_or(Err(vesper_provider::CredentialError::Failed)),
            Err(_) => {
                eprintln!(
                    "Use TUI Settings → Providers → OpenAI for masked API-key entry, or supply OPENAI_API_KEY for this setup command."
                );
                Err(vesper_provider::CredentialError::Absent)
            }
        }
    } else {
        return None;
    };
    Some(match result {
        Ok(()) => {
            eprintln!("OpenAI authentication updated.");
            ExitCode::SUCCESS
        }
        Err(_) => {
            eprintln!(
                "OpenAI authentication failed or was cancelled. No API billing fallback was attempted."
            );
            ExitCode::FAILURE
        }
    })
}

fn handle_auth_flags() -> Option<ExitCode> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--check-auth") {
        let source = vesper_provider_glm::EnvironmentCredentialSource;
        if vesper_provider_glm::resolve_credential(&source).is_ok() {
            eprintln!("Z.ai credentials are configured.");
            Some(ExitCode::SUCCESS)
        } else {
            eprintln!("Z.ai credentials are not configured.");
            Some(ExitCode::from(1))
        }
    } else if args.iter().any(|arg| arg == "--setup") {
        let key = std::env::var("ZAI_API_KEY")
            .or_else(|_| std::env::var("Z_AI_API_KEY"))
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                eprintln!("Z.ai API key (input is not echoed by this protocol process):");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input).ok()?;
                Some(input)
            });
        let Some(key) = key else {
            eprintln!("Setup failed: no API key supplied.");
            return Some(ExitCode::from(1));
        };
        match vesper_provider_glm::store_api_key(&key) {
            Ok(_) => {
                eprintln!("Credentials saved. The key was not printed.");
                Some(ExitCode::SUCCESS)
            }
            Err(error) => {
                eprintln!("Setup failed: {error}");
                Some(ExitCode::from(1))
            }
        }
    } else {
        None
    }
}

/// Parses a `--provider <value>` or `--provider=<value>` CLI flag.
///
/// Returns the selected provider token so the composition boundary can resolve
/// it through the same `boot` dispatch the environment-driven path uses. When
/// absent the caller falls back to `AGENT_VESPER_PROVIDER` (default `glm`).
/// This stays provider-agnostic; unknown values are rejected by `boot`.
fn provider_from_argv() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--provider=") {
            return Some(value.to_owned());
        }
        if arg == "--provider" {
            return args.next();
        }
    }
    None
}

fn configure_stderr_tracing() {
    let filter =
        EnvFilter::try_from_env("AGENT_VESPER_LOG").unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init();
}
