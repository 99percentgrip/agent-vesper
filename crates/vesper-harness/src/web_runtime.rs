//! Lazy shared web runtime. No containers, probes, temp files or I/O at boot.
use crate::web_service::WebScope;
use std::sync::{Arc, Mutex};
use vesper_web::action::{ActionResult, BrowserAction};
use vesper_web::driver::DriverError;
use vesper_web::transport::{BoxFuture, FetchError, FetchRequest, FetchResponse, FetchTransport};
#[cfg(feature = "docker")]
use vesper_web_fetch::WebSandboxRouteConfig;
use vesper_web_fetch::{WebSandboxPort, browser::BrowserSession};

pub(crate) struct WebRuntime {
    scope: WebScope,
    state: Mutex<Option<State>>,
    permits: Arc<tokio::sync::Semaphore>,
}

struct State {
    browser: Option<BrowserSession>,
    route: Arc<WebSandboxPort>,
    _root: tempfile::TempDir,
}

impl WebRuntime {
    fn check_url(&self, url: &str) -> Result<(), FetchError> {
        let policy = vesper_web::egress::EgressPolicy {
            allow_plain_http: true,
            allowed_hosts: self.scope.allowlist.clone(),
            respect_robots: self.scope.respect_robots,
        };
        match vesper_web::egress::evaluate(url, &policy, None) {
            vesper_web::egress::EgressVerdict::Allow => Ok(()),
            vesper_web::egress::EgressVerdict::Deny(reason) => {
                Err(FetchError::Egress(reason.name().into()))
            }
        }
    }
    pub(crate) fn new(scope: WebScope) -> Self {
        Self {
            scope,
            state: Mutex::new(None),
            permits: Arc::new(tokio::sync::Semaphore::new(4)),
        }
    }

    #[cfg(not(feature = "docker"))]
    fn initialize(&self) -> Result<State, FetchError> {
        Err(FetchError::Sandbox(
            "web driver requires a binary built with the docker feature".into(),
        ))
    }

    #[cfg(feature = "docker")]
    fn initialize(&self) -> Result<State, FetchError> {
        if std::env::var("AGENT_VESPER_SANDBOX")
            .is_ok_and(|value| value.eq_ignore_ascii_case("off"))
        {
            return Err(FetchError::Sandbox(
                "sandbox disabled by AGENT_VESPER_SANDBOX=off".into(),
            ));
        }
        let root = tempfile::tempdir().map_err(|e| FetchError::Sandbox(e.to_string()))?;
        let image = self
            .scope
            .driver_image
            .clone()
            .or_else(|| std::env::var("VESPER_DOCKER_IMAGE").ok())
            .or_else(|| crate::web_settings::bundled_image_id().ok())
            .ok_or_else(|| {
                FetchError::Sandbox(
                    "select Set up / repair driver in Settings > Web tools (ACP: /web setup), save, and restart the host".into(),
                )
            })?;
        if !vesper_config::is_digest_pinned_image(&image) {
            return Err(FetchError::Sandbox(
                "web driver image must be digest-pinned".into(),
            ));
        }
        let backend: Arc<dyn vesper_sandbox::SandboxBackend> = Arc::new(
            vesper_sandbox::DockerBackend::new(vesper_sandbox::DockerSandboxConfig {
                network: true,
                docker_bin: Some(crate::web_settings::container_cli()),
                image: Some(image),
                ..Default::default()
            }),
        );
        {
            let mut config = WebSandboxRouteConfig::new(
                "/usr/local/bin/vesper-web-fetch".into(),
                root.path().into(),
            );
            config.egress = vesper_web::egress::EgressPolicy {
                allow_plain_http: true,
                allowed_hosts: self.scope.allowlist.clone(),
                respect_robots: self.scope.respect_robots,
            };
            config.user_agent = self.scope.user_agent.clone();
            Ok(State {
                browser: None,
                route: Arc::new(WebSandboxPort::new(backend, config)),
                _root: root,
            })
        }
    }

    pub(crate) async fn interact(
        self: &Arc<Self>,
        action: BrowserAction,
        submit: bool,
    ) -> Result<ActionResult, DriverError> {
        if let BrowserAction::Navigate { url } = &action {
            self.check_url(url)
                .map_err(|e| DriverError::Invalid(e.to_string()))?;
        }
        let runtime = self.clone();
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| DriverError::Sandbox("web runtime closed".into()))?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut slot = runtime
                .state
                .lock()
                .map_err(|_| DriverError::Pipe("web runtime poisoned".into()))?;
            if matches!(action, BrowserAction::Close) {
                if let Some(state) = slot.as_mut() {
                    state.browser.take();
                }
                return Ok(ActionResult::ok(&action, true, "session closed".into()));
            }
            if slot.is_none() {
                *slot = Some(
                    runtime
                        .initialize()
                        .map_err(|e| DriverError::Sandbox(e.to_string()))?,
                );
            }
            let state = slot.as_mut().expect("initialized");
            let handle = tokio::runtime::Handle::current();
            if state.browser.is_none() {
                if !matches!(action, BrowserAction::Navigate { .. }) {
                    return Err(DriverError::Invalid("no session; navigate to open".into()));
                }
                state.browser = Some(handle.block_on(BrowserSession::open(
                    state.route.clone(),
                    runtime.scope.output_budget_bytes as usize,
                ))?);
            }
            let result = handle.block_on(
                state
                    .browser
                    .as_mut()
                    .expect("opened")
                    .execute(&action, submit),
            );
            if matches!(result, Err(DriverError::Pipe(_) | DriverError::Cdp { .. })) {
                state.browser.take();
            }
            result
        })
        .await
        .map_err(|_| DriverError::Pipe("browser worker failed; reopen session".into()))?
    }

    pub(crate) async fn render(
        self: &Arc<Self>,
        url: String,
        seconds: u64,
    ) -> Result<FetchResponse, FetchError> {
        self.check_url(&url)?;
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(seconds.clamp(1, 45));
        let runtime = self.clone();
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| FetchError::Sandbox("web runtime closed".into()))?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut slot = runtime
                .state
                .lock()
                .map_err(|_| FetchError::Sandbox("web runtime poisoned".into()))?;
            if slot.is_none() {
                *slot = Some(runtime.initialize()?);
            }
            let route = slot.as_ref().expect("initialized").route.clone();
            drop(slot);
            let handle = tokio::runtime::Handle::current();
            // Render uses an ephemeral page; it must not mutate the user's
            // interactive session or its index map.
            let mut browser = handle
                .block_on(BrowserSession::open_until(
                    route,
                    runtime.scope.output_budget_bytes as usize,
                    deadline,
                ))
                .map_err(|e| FetchError::Fetch(format!("render-escalation-failed: {e}")))?;
            handle
                .block_on(browser.execute_until(
                    &BrowserAction::Navigate { url: url.clone() },
                    false,
                    deadline,
                ))
                .map_err(|e| FetchError::Fetch(format!("render-escalation-failed: {e}")))?;
            let body = browser
                .html()
                .map_err(|e| FetchError::Fetch(format!("render-escalation-failed: {e}")))?;
            let (url, status) = browser
                .page_identity()
                .map_err(|e| FetchError::Fetch(e.to_string()))?;
            Ok(FetchResponse {
                status,
                url,
                body,
                content_type: "text/html".into(),
                truncated: false,
            })
        })
        .await
        .map_err(|_| FetchError::Sandbox("render worker failed".into()))?
    }
}

/// Arc wrapper preserves the exact runtime/route identity for fetch and render.
pub(crate) struct RuntimeFetch(pub Arc<WebRuntime>);
pub(crate) struct RuntimeBrowser(pub Arc<WebRuntime>);

impl vesper_web::driver::BrowserDriverPort for RuntimeBrowser {
    fn execute(&self, action: &BrowserAction) -> BoxFuture<'_, Result<ActionResult, DriverError>> {
        self.execute_with_submit(action, false)
    }
    fn execute_with_submit(
        &self,
        action: &BrowserAction,
        submit: bool,
    ) -> BoxFuture<'_, Result<ActionResult, DriverError>> {
        let action = action.clone();
        Box::pin(async move { self.0.interact(action, submit).await })
    }
    fn close(&self) -> BoxFuture<'_, Result<(), DriverError>> {
        Box::pin(async move {
            self.0
                .interact(BrowserAction::Close, false)
                .await
                .map(|_| ())
        })
    }
}

impl vesper_web::transport::RenderTransport for RuntimeFetch {
    fn render(&self, request: &FetchRequest) -> BoxFuture<'_, Result<FetchResponse, FetchError>> {
        let url = request.url.clone();
        let seconds = request.timeout_seconds;
        Box::pin(async move { self.0.render(url, seconds).await })
    }
}
impl FetchTransport for RuntimeFetch {
    fn fetch(&self, request: &FetchRequest) -> BoxFuture<'_, Result<FetchResponse, FetchError>> {
        let runtime = self.0.clone();
        let request = request.clone();
        Box::pin(async move {
            let permit = runtime
                .permits
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| FetchError::Sandbox("web runtime closed".into()))?;
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                let route = {
                    let mut slot = runtime
                        .state
                        .lock()
                        .map_err(|_| FetchError::Sandbox("web runtime poisoned".into()))?;
                    if slot.is_none() {
                        *slot = Some(runtime.initialize()?);
                    }
                    slot.as_ref().expect("initialized").route.clone()
                };
                tokio::runtime::Handle::current().block_on(route.fetch(&request))
            })
            .await
            .map_err(|_| FetchError::Sandbox("web worker failed".into()))?
        })
    }
}
