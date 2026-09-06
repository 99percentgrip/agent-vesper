//! [`WebSandboxPort`] — the production `FetchTransport` (VRO-14 PR-3).
//!
//! Routing contract (PRD Feature 4), enforced in this order:
//!
//! 1. **Pure egress gate** — `vesper_web::egress::evaluate` classifies the
//!    URL (scheme, host allowlist, literal-IP class, robots verdict)
//!    *before* anything is provisioned. A denied URL returns
//!    `FetchError::Egress` with the typed denial name; no process spawns.
//! 2. **Capability gate** — the backend must honestly satisfy
//!    `IsolationRequirement::Network`. Anything less yields
//!    `FetchError::Sandbox` (fail-closed; never a silent unsandboxed run).
//! 3. **Execution** — the `vesper-web-fetch` helper runs inside the
//!    provisioned sandbox with `allow_network = true` on the spec (the
//!    grant is explicit per fetch, never a stored default). The helper's
//!    `VWMETA:` stderr line carries status/content-type/charset/final
//!    URL/redirects/truncation; stdout carries the body.

use std::sync::Arc;

use vesper_sandbox::{Argv, SandboxBackend, SandboxError, SandboxSpec};
use vesper_security::IsolationRequirement;
use vesper_web::transport::{
    BoxFuture, EgressPolicy, FetchError, FetchRequest, FetchResponse, FetchTransport,
};

use crate::parse::{parse_helper_meta, response_from_parts};

/// Configuration for the sandboxed fetch route.
#[derive(Debug, Clone)]
pub struct WebSandboxRouteConfig {
    /// Absolute path of the `vesper-web-fetch` helper binary (copied into
    /// the sandbox's executable path by the host at install time).
    pub helper_path: std::path::PathBuf,
    /// Directory the sandbox mounts read-write. Fetches need no writable
    /// workspace; hosts pass a temp dir.
    pub writable_root: std::path::PathBuf,
    /// Pre-flight policy.
    pub egress: EgressPolicy,
    /// Timeout (seconds) placed on the sandbox spec.
    pub timeout_seconds: u64,
    /// Shared HTTP/browser identification string.
    pub user_agent: String,
}

impl WebSandboxRouteConfig {
    /// Defaults: egress defaults, 45 s timeout.
    #[must_use]
    pub fn new(helper_path: std::path::PathBuf, writable_root: std::path::PathBuf) -> Self {
        Self {
            helper_path,
            writable_root,
            egress: EgressPolicy::default(),
            timeout_seconds: crate::DEFAULT_FETCH_TIMEOUT_SECONDS,
            user_agent: "agent-vesper".into(),
        }
    }
}

/// The sandbox-backed transport. Holds an `Arc<dyn SandboxBackend>` so it
/// is `Clone + Send + Sync` for the host.
#[derive(Clone)]
pub struct WebSandboxPort {
    pub(crate) backend: Arc<dyn SandboxBackend>,
    pub(crate) config: WebSandboxRouteConfig,
}

impl WebSandboxPort {
    /// Wire the port to a backend.
    #[must_use]
    pub fn new(backend: Arc<dyn SandboxBackend>, config: WebSandboxRouteConfig) -> Self {
        Self { backend, config }
    }

    /// Nonzero identity shared by fetch and browser composition paths.
    pub fn instance_id(&self) -> usize {
        Arc::as_ptr(&self.backend) as *const () as usize
    }

    /// The pre-flight gate, exposed for tests and host-side pre-checks.
    pub fn egress_check(&self, url: &str) -> Result<(), FetchError> {
        match vesper_web::egress::evaluate(url, &self.config.egress, None) {
            vesper_web::egress::EgressVerdict::Allow => Ok(()),
            vesper_web::egress::EgressVerdict::Deny(denial) => {
                Err(FetchError::Egress(denial.name().to_string()))
            }
        }
    }
}

fn sandbox_denial(error: SandboxError) -> FetchError {
    FetchError::Sandbox(match error {
        SandboxError::CapabilityUnavailable {
            requirement,
            capabilities,
        } => format!(
            "backend cannot satisfy {requirement:?}; available: \
             process_tree={:?} filesystem={:?} network={:?} — the fetch \
             needs an isolated network path and the operation was refused \
             rather than run unsandboxed",
            capabilities.process_tree, capabilities.filesystem, capabilities.network
        ),
        other => other.to_string(),
    })
}

impl FetchTransport for WebSandboxPort {
    fn fetch(&self, request: &FetchRequest) -> BoxFuture<'_, Result<FetchResponse, FetchError>> {
        let url = request.url.clone();
        let timeout = request
            .timeout_seconds
            .min(self.config.timeout_seconds)
            .clamp(1, crate::DEFAULT_FETCH_TIMEOUT_SECONDS);
        let max_body = request.max_body_bytes.clamp(1, 512 * 1024);
        let backend = Arc::clone(&self.backend);
        let helper = self.config.helper_path.clone();
        let writable_root = self.config.writable_root.clone();
        let policy = self.config.egress.clone();
        let user_agent = self.config.user_agent.clone();

        Box::pin(async move {
            // 1. Pure pre-flight gate. DNS and robots are not performed on
            //    the host: the helper enforces both inside the sandbox before
            //    fetching a page, using the serialized policy below.
            if let vesper_web::egress::EgressVerdict::Deny(denial) =
                vesper_web::egress::evaluate(&url, &policy, None)
            {
                return Err(FetchError::Egress(denial.name().to_string()));
            }

            // 2. Fail-closed capability gate.
            let capabilities = backend.capabilities();
            if !capabilities.satisfies(IsolationRequirement::Network) {
                return Err(FetchError::Sandbox(format!(
                    "backend cannot satisfy Network isolation; available: \
                     process_tree={:?} filesystem={:?} network={:?} — the fetch \
                     needs an isolated network path and the operation was refused \
                     rather than run unsandboxed",
                    capabilities.process_tree, capabilities.filesystem, capabilities.network
                )));
            }

            // 3. Provision + execute with the explicit per-fetch network
            //    grant. The spec's default is NO network; this grant is
            //    the only path to egress.
            let mut spec = SandboxSpec::new(writable_root);
            spec.timeout_seconds = timeout;
            spec.allow_network = true;
            spec.cpu_limit = Some(2.0);
            spec.memory_limit_bytes = Some(512 * 1024 * 1024);

            let handle = backend.provision(&spec).await.map_err(sandbox_denial)?;
            if max_body > vesper_sandbox::OUTPUT_CAP_BYTES {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
                let pipe = backend
                    .open_pipe(
                        Arc::new(handle),
                        &Argv {
                            argv: vec![helper.to_string_lossy().into_owned(), "--pipe".into()],
                            cwd: "/".into(),
                        },
                    )
                    .map_err(sandbox_denial)?;
                pipe.send(serde_json::to_vec(&serde_json::json!({"url":url,"cap":max_body,"policy":{
                    "allow_plain_http":policy.allow_plain_http,"allowed_hosts":policy.allowed_hosts,"respect_robots":policy.respect_robots,"user_agent":user_agent,
                }})).map_err(|_| FetchError::Fetch("invalid request".into()))?, deadline).map_err(sandbox_denial)?;
                let mut body = String::new();
                loop {
                    let bytes = pipe.receive(deadline).map_err(sandbox_denial)?;
                    if bytes.len() > vesper_sandbox::OUTPUT_CAP_BYTES {
                        return Err(FetchError::TooLarge("helper chunk".into()));
                    }
                    let frame: serde_json::Value = serde_json::from_slice(&bytes)
                        .map_err(|_| FetchError::Fetch("invalid helper frame".into()))?;
                    if let Some(reason) = frame["error"].as_str() {
                        return Err(match frame["kind"].as_str() {
                            Some("egress") => FetchError::Egress(reason.into()),
                            Some("budget") => FetchError::TooLarge(reason.into()),
                            _ => FetchError::Fetch(reason.into()),
                        });
                    }
                    if let Some(chunk) = frame["body"].as_str() {
                        if body.len() + chunk.len() > max_body {
                            return Err(FetchError::TooLarge("helper body".into()));
                        }
                        body.push_str(chunk);
                    }
                    if frame["done"] == true {
                        let meta = frame["meta"]
                            .as_str()
                            .and_then(parse_helper_meta)
                            .ok_or_else(|| FetchError::Fetch("missing helper metadata".into()))?;
                        return response_from_parts(meta, &url, body, false)
                            .map_err(FetchError::Fetch);
                    }
                }
            }
            let argv = Argv {
                argv: vec![
                    helper.to_string_lossy().into_owned(),
                    url.clone(),
                    max_body.to_string(),
                    serde_json::json!({
                        "allow_plain_http": policy.allow_plain_http,
                        "allowed_hosts": policy.allowed_hosts,
                        "respect_robots": policy.respect_robots,
                        "user_agent": user_agent,
                    })
                    .to_string(),
                ],
                cwd: std::path::PathBuf::from("/"),
            };
            let output = backend.run(&handle, &argv).await;
            let teardown = backend.teardown(handle).await;
            let output = output.map_err(sandbox_denial)?;
            teardown.map_err(sandbox_denial)?;
            parse_output(output, &url, max_body)
        })
    }
}

/// Assemble the actual sandbox output, also used by offline protocol tests.
pub fn parse_output(
    output: vesper_sandbox::ExecOutput,
    url: &str,
    max_body: usize,
) -> Result<FetchResponse, FetchError> {
    if output.timed_out {
        return Err(FetchError::Fetch("timed_out".into()));
    }
    if output.exit_code != Some(0) {
        let first = output
            .stderr
            .lines()
            .find(|line| !line.starts_with("VWMETA:"))
            .unwrap_or("unknown fetch failure");
        return Err(match output.exit_code {
            Some(1) => FetchError::Egress(first.to_string()),
            Some(3) => FetchError::TooLarge(first.to_string()),
            _ => FetchError::Fetch(first.to_string()),
        });
    }
    let vmeta = parse_helper_meta(&output.stderr)
        .ok_or_else(|| FetchError::Fetch("helper produced no VWMETA line".into()))?;
    let mut body = output.stdout;
    let cap = max_body.clamp(1, vesper_sandbox::OUTPUT_CAP_BYTES);
    let truncated = body.len() > cap;
    let mut end = cap.min(body.len());
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    body.truncate(end);
    response_from_parts(vmeta, url, body, truncated).map_err(FetchError::Fetch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vesper_sandbox::{Argv, SandboxError, SandboxHandle, SandboxSpec};
    use vesper_security::{
        CapabilityStatus, IsolationRequirement, SandboxCapabilities, SecurityStrength,
    };

    struct UnavailableBackend;

    impl SandboxBackend for UnavailableBackend {
        fn capabilities(&self) -> SandboxCapabilities {
            SandboxCapabilities {
                backend: "unavailable-stub".into(),
                process_tree: CapabilityStatus::Unavailable,
                filesystem: CapabilityStatus::Unavailable,
                network: CapabilityStatus::Unavailable,
                strength: SecurityStrength::None,
            }
        }
        fn provision<'a>(
            &'a self,
            _spec: &'a SandboxSpec,
        ) -> vesper_sandbox::SandboxFuture<'a, Result<vesper_sandbox::SandboxHandle, SandboxError>>
        {
            Box::pin(async {
                Err(SandboxError::CapabilityUnavailable {
                    requirement: IsolationRequirement::Network,
                    capabilities: self.capabilities(),
                })
            })
        }
        fn run<'a>(
            &'a self,
            _handle: &'a vesper_sandbox::SandboxHandle,
            _argv: &'a Argv,
        ) -> vesper_sandbox::SandboxFuture<'a, Result<vesper_sandbox::ExecOutput, SandboxError>>
        {
            Box::pin(async { Err(SandboxError::Provision("never provisioned".into())) })
        }
        fn teardown<'a>(
            &'a self,
            _handle: SandboxHandle,
        ) -> vesper_sandbox::SandboxFuture<'a, Result<(), SandboxError>> {
            Box::pin(async { Ok(()) })
        }
    }

    fn port() -> WebSandboxPort {
        WebSandboxPort::new(
            Arc::new(UnavailableBackend),
            WebSandboxRouteConfig::new(
                "/nonexistent/vesper-web-fetch".into(),
                std::env::temp_dir(),
            ),
        )
    }

    #[tokio::test]
    async fn loopback_url_is_denied_before_provisioning() {
        let port = port();
        let err = port
            .fetch(&FetchRequest::new("https://127.0.0.1:8080/admin"))
            .await
            .expect_err("must deny loopback");
        assert!(
            matches!(err, FetchError::Egress(ref reason) if reason == "loopback_address"),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn localhost_hostname_is_denied_before_provisioning() {
        let port = port();
        let err = port
            .fetch(&FetchRequest::new("https://localhost/x"))
            .await
            .expect_err("must deny localhost");
        assert!(
            matches!(err, FetchError::Egress(ref reason) if reason == "loopback_address"),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn private_ranges_are_denied_before_provisioning() {
        for url in [
            "http://10.1.2.3/x",
            "http://192.168.1.1/router",
            "http://172.16.0.9/",
            "http://172.31.255.255/",
            "http://169.254.169.254/latest/meta-data",
            "http://[::1]/",
            "http://[fe80::1]/",
            "http://[fd12:3456:789a::1]/",
        ] {
            let err = port()
                .fetch(&FetchRequest::new(url))
                .await
                .expect_err(&format!("must deny {url}"));
            assert!(
                matches!(err, FetchError::Egress(_)),
                "{url} must be an egress denial, got {err:?}"
            );
        }
    }

    #[tokio::test]
    async fn capability_shortfall_fails_closed() {
        // A public URL that passes egress, against a backend that cannot
        // provide network isolation: must refuse, not run unsandboxed.
        let port = port();
        let err = port
            .fetch(&FetchRequest::new("https://example.com/"))
            .await
            .expect_err("must refuse");
        assert!(matches!(err, FetchError::Sandbox(_)), "got {err:?}");
        let message = err.to_string();
        assert!(
            message.contains("refused rather than run unsandboxed"),
            "refusal must be explicit: {message}"
        );
    }

    #[tokio::test]
    async fn http_scheme_is_denied_by_default() {
        let err = port()
            .fetch(&FetchRequest::new("http://example.com/"))
            .await
            .expect_err("plain http is denied by default");
        assert!(
            matches!(err, FetchError::Egress(ref reason) if reason == "plain_http_disallowed"),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn non_web_scheme_is_denied() {
        for url in ["file:///etc/passwd", "ftp://example.com/x", "gopher://x"] {
            let err = port()
                .fetch(&FetchRequest::new(url))
                .await
                .expect_err(&format!("must deny {url}"));
            assert!(
                matches!(err, FetchError::Egress(ref reason) if reason == "non_web_scheme"),
                "{url}: got {err:?}"
            );
        }
    }
}
