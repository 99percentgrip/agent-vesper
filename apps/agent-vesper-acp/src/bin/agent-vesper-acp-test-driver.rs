#![forbid(unsafe_code)]
//! Non-default process-conformance composition with a generic pre-dispatch gate.

use std::{
    io::{Read, Write},
    net::TcpStream,
    process::ExitCode,
    sync::Arc,
    time::Duration,
};

use vesper_domain::{
    ErrorCategory, ErrorInfo, ExtensionMap, ProviderId, RedactedDiagnostics, Retryability,
    SafeMessage,
};
use vesper_provider::{
    CancellationSignal, ProviderConfiguration, ProviderError, ProviderFactory, ProviderFuture,
};
use vesper_provider_glm::{GlmFactory, GlmSession};

struct DispatchGateFactory {
    inner: GlmFactory,
    gate_address: String,
}

impl ProviderFactory for DispatchGateFactory {
    type Session = GlmSession;

    fn provider_id(&self) -> &ProviderId {
        self.inner.provider_id()
    }

    fn create_session<'a>(
        &'a self,
        config: &'a ProviderConfiguration,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<Self::Session, ProviderError>> {
        Box::pin(async move {
            let session = self
                .inner
                .create_session(config, Arc::clone(&cancellation))
                .await?;
            let mut gate =
                TcpStream::connect(&self.gate_address).map_err(|_| gate_error("connect"))?;
            gate.set_read_timeout(Some(Duration::from_secs(10)))
                .map_err(|_| gate_error("timeout"))?;
            gate.write_all(b"provider-session-ready\n")
                .map_err(|_| gate_error("signal"))?;
            let mut release = [0_u8; 1];
            gate.read_exact(&mut release)
                .map_err(|_| gate_error("release"))?;
            if release[0] == b'c' {
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                while !cancellation.is_cancelled() {
                    if std::time::Instant::now() >= deadline {
                        return Err(gate_error("cancellation observation"));
                    }
                    std::thread::yield_now();
                }
            }
            Ok(session)
        })
    }
}

fn gate_error(operation: &str) -> ProviderError {
    ProviderError {
        provider_id: vesper_provider_glm::provider_id(),
        provider_code: None,
        http_status: None,
        continuation_possible: false,
        info: ErrorInfo {
            category: ErrorCategory::Transport,
            retryability: Retryability::Never,
            retry_after_ms: None,
            visible_output_emitted: false,
            safe_message: SafeMessage::new(format!("integration dispatch gate {operation} failed"))
                .expect("bounded gate error"),
            diagnostics: RedactedDiagnostics::default(),
            provider_code: None,
            causes: Vec::new(),
        },
        metadata: ExtensionMap::default(),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let Ok(gate_address) = std::env::var("AGENT_VESPER_TEST_DISPATCH_GATE") else {
        eprintln!("integration dispatch gate is required");
        return ExitCode::FAILURE;
    };
    let factory = DispatchGateFactory {
        inner: GlmFactory::default(),
        gate_address,
    };
    match agent_vesper_acp::run_with_factory(factory).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => ExitCode::FAILURE,
    }
}
