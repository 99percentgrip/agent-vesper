#![forbid(unsafe_code)]

use std::process::ExitCode;

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    // Meta flags (`--version`/`--help`) are handled BEFORE the ACP server
    // boots so the installer's `agent-vesper-acp --version` check works and
    // never tries to start the stdio protocol. Mirrors the original Python
    // `native-glm-acp --version` UX.
    if let Some(code) = handle_meta_flags() {
        return code;
    }
    if let Some(code) = handle_auth_flags() {
        return code;
    }
    configure_stderr_tracing();
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
    eprintln!("        --provider <glm|zai|lmstudio>     Initial provider (default: AGENT_VESPER_PROVIDER or glm)");
    eprintln!("                                          All adapters stay registered; switch via the footer Provider picker");
    eprintln!("    -V, --version                        Print version and exit");
    eprintln!("    -h, --help                           Print this help and exit");
    eprintln!();
    eprintln!("ENVIRONMENT:");
    eprintln!("    ZAI_API_KEY                          Z.ai API key (required for the GLM provider)");
    eprintln!("    LMSTUDIO_API_KEY                     LM Studio API key (optional; local servers usually need none)");
    eprintln!("    AGENT_VESPER_PROVIDER                Default provider (glm|zai|lmstudio)");
    eprintln!("    AGENT_VESPER_LOG                     Tracing filter (default: warn, stderr only)");
    eprintln!("    --setup                               Store a Z.ai API key without printing it");
    eprintln!("    --check-auth                          Check configured Z.ai credentials");
}

/// Handles the explicit terminal authentication setup path. Credentials are
/// accepted from the environment or one stdin line and are written only
/// through the provider's atomic, user-private credential store.
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
