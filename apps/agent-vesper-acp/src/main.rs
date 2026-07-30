#![forbid(unsafe_code)]

use std::process::ExitCode;

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
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
