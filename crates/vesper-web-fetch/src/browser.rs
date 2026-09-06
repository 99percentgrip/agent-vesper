//! Stateful pipe-CDP execution over the same sandbox route as plain fetch.
//! Callers offload these blocking protocol operations from their async/UI loop.
use crate::WebSandboxPort;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use vesper_sandbox::{Argv, SandboxHandle, SandboxPipe, SandboxSpec};
use vesper_security::IsolationRequirement;
use vesper_web::action::{ActionResult, BrowserAction};
use vesper_web::driver::DriverError;
use vesper_web::selector_map::{SelectorMapCache, serialize_interactable_map};
use vesper_web::snapshot::{CaptureSnapshotResult, MaterializedNode, REQUIRED_COMPUTED_STYLES};

/// One tab and ephemeral container. Dropping the session tears both down.
pub struct BrowserSession {
    pipe: SandboxPipe,
    handle: Arc<SandboxHandle>,
    route: Arc<WebSandboxPort>,
    session: String,
    next_id: u64,
    cache: SelectorMapCache,
    nodes: HashMap<usize, MaterializedNode>,
    deadline: Instant,
    budget: usize,
    main_frame: String,
    page_status: u16,
}

fn pipe_error(error: impl std::fmt::Display) -> DriverError {
    DriverError::Pipe(error.to_string())
}

impl BrowserSession {
    /// Probe containment, launch the fixed image entry script and perform a
    /// real blank-page smoke navigation before reporting a usable session.
    pub async fn open(route: Arc<WebSandboxPort>, budget: usize) -> Result<Self, DriverError> {
        Self::open_until(route, budget, Instant::now() + Duration::from_secs(30)).await
    }

    /// Open under the caller's absolute operation deadline.
    pub async fn open_until(
        route: Arc<WebSandboxPort>,
        budget: usize,
        deadline: Instant,
    ) -> Result<Self, DriverError> {
        if !route
            .backend
            .capabilities()
            .satisfies(IsolationRequirement::Network)
        {
            return Err(DriverError::Sandbox("Network isolation unavailable".into()));
        }
        let mut spec = SandboxSpec::new(route.config.writable_root.clone())
            .with_network_grant()
            .with_cpu_limit(2.0)
            .with_memory_limit_bytes(1024 * 1024 * 1024);
        // An abandoned host cannot leave an unbounded detached browser.
        spec.timeout_seconds = 900;
        let handle = Arc::new(route.backend.provision(&spec).await.map_err(pipe_error)?);
        let pipe = route
            .backend
            .open_pipe(
                handle.clone(),
                &Argv {
                    argv: vec!["/usr/local/bin/vesper-browser-pipe".into()],
                    cwd: "/".into(),
                },
            )
            .map_err(pipe_error)?;
        let mut this = Self {
            pipe,
            handle,
            route,
            session: String::new(),
            next_id: 1,
            cache: SelectorMapCache::new("pending"),
            nodes: HashMap::new(),
            deadline,
            budget: budget.clamp(1, 512 * 1024),
            main_frame: String::new(),
            page_status: 0,
        };
        this.command("Browser.getVersion", json!({}))?;
        let target = this.command("Target.createTarget", json!({"url":"about:blank"}))?;
        let attached = this.command(
            "Target.attachToTarget",
            json!({"targetId":target["targetId"],"flatten":true}),
        )?;
        this.session = attached["sessionId"]
            .as_str()
            .ok_or_else(|| pipe_error("missing CDP session"))?
            .into();
        this.cache = SelectorMapCache::new(&this.session);
        this.command("Page.enable", json!({}))?;
        this.command("Network.enable", json!({}))?;
        let frames = this.command("Page.getFrameTree", json!({}))?;
        this.main_frame = frames["frameTree"]["frame"]["id"]
            .as_str()
            .unwrap_or_default()
            .into();
        this.command("Accessibility.enable", json!({}))?;
        this.command(
            "Network.setUserAgentOverride",
            json!({"userAgent":this.route.config.user_agent}),
        )?;
        this.command("Browser.setDownloadBehavior", json!({"behavior":"deny"}))?;
        this.command("Page.navigate", json!({"url":"about:blank"}))?;
        this.snapshot()?;
        this.command(
            "Fetch.enable",
            json!({"patterns":[{"resourceType":"Document","requestStage":"Request"}]}),
        )?;
        Ok(this)
    }

    /// Sandbox backend identity, identical to this route's fetch identity.
    pub fn route_id(&self) -> usize {
        self.route.instance_id()
    }

    fn command(&mut self, method: &str, params: Value) -> Result<Value, DriverError> {
        let id = self.next_id;
        self.next_id += 1;
        let mut command = json!({"id":id,"method":method,"params":params});
        if !self.session.is_empty()
            && !method.starts_with("Browser.")
            && !method.starts_with("Target.")
        {
            command["sessionId"] = json!(self.session);
        }
        self.pipe
            .send(
                serde_json::to_vec(&command).map_err(pipe_error)?,
                self.deadline,
            )
            .map_err(pipe_error)?;
        loop {
            let bytes = self.pipe.receive(self.deadline).map_err(pipe_error)?;
            let message: Value =
                serde_json::from_slice(&bytes).map_err(|_| pipe_error("invalid CDP frame"))?;
            if message["method"] == "Network.responseReceived"
                && message["params"]["type"] == "Document"
                && message["params"]["frameId"].as_str() == Some(&self.main_frame)
            {
                self.page_status = message["params"]["response"]["status"]
                    .as_f64()
                    .unwrap_or(0.0) as u16;
            }
            if message["method"] == "Fetch.requestPaused" {
                let request = &message["params"];
                let url = request["request"]["url"].as_str().unwrap_or_default();
                let admitted = block_on(self.admit(url));
                let (resume, params) = if admitted.is_ok() {
                    (
                        "Fetch.continueRequest",
                        json!({"requestId":request["requestId"]}),
                    )
                } else {
                    (
                        "Fetch.failRequest",
                        json!({"requestId":request["requestId"],"errorReason":"BlockedByClient"}),
                    )
                };
                let resume_id = self.next_id;
                self.next_id += 1;
                self.pipe.send(serde_json::to_vec(&json!({"id":resume_id,"method":resume,"params":params,"sessionId":self.session})).map_err(pipe_error)?, self.deadline).map_err(pipe_error)?;
                admitted?;
                continue;
            }
            if message["id"].as_u64() != Some(id) {
                continue;
            }
            if message.get("error").is_some() {
                // Protocol errors can echo typed text. Never copy them out.
                return Err(DriverError::Cdp {
                    method: method.into(),
                    message: "command rejected".into(),
                });
            }
            return Ok(message["result"].clone());
        }
    }

    async fn admit(&self, url: &str) -> Result<(), DriverError> {
        self.route.egress_check(url).map_err(pipe_error)?;
        let policy = &self.route.config.egress;
        let output = self.route.backend.run(&self.handle, &Argv {
            argv: vec!["timeout".into(), self.deadline.saturating_duration_since(Instant::now()).as_secs().max(1).to_string(),
                self.route.config.helper_path.to_string_lossy().into_owned(), url.into(), "1".into(),
                json!({"allow_plain_http":policy.allow_plain_http,"allowed_hosts":policy.allowed_hosts,
                    "respect_robots":policy.respect_robots,"user_agent":self.route.config.user_agent}).to_string(), "--check".into()], cwd: "/".into(),
        }).await.map_err(pipe_error)?;
        crate::route::parse_output(output, url, 1).map_err(pipe_error)?;
        Ok(())
    }

    fn object(&mut self, index: usize) -> Result<String, DriverError> {
        let id = self
            .nodes
            .get(&index)
            .and_then(|n| n.backend_node_id)
            .ok_or(DriverError::UnknownIndex(index))?;
        let result = self
            .command(
                "DOM.resolveNode",
                json!({"backendNodeId":id,"objectGroup":"vesper-action"}),
            )
            .map_err(|_| DriverError::UnknownIndex(index))?;
        let object = result["object"]["objectId"]
            .as_str()
            .ok_or(DriverError::UnknownIndex(index))?
            .to_owned();
        let live = self.call_object(&object, "function(){return this.isConnected}", json!([]))?;
        if live.as_bool() != Some(true) {
            return Err(DriverError::UnknownIndex(index));
        }
        Ok(object)
    }

    fn call_object(
        &mut self,
        object: &str,
        function: &str,
        arguments: Value,
    ) -> Result<Value, DriverError> {
        let result = self.command(
            "Runtime.callFunctionOn",
            json!({"objectId":object,"functionDeclaration":function,
            "arguments":arguments,"returnByValue":true,"awaitPromise":true}),
        )?;
        if result.get("exceptionDetails").is_some() {
            return Err(pipe_error("page operation rejected"));
        }
        Ok(result["result"]["value"].clone())
    }

    fn evaluate(&mut self, expression: &str) -> Result<Value, DriverError> {
        let result = self.command(
            "Runtime.evaluate",
            json!({"expression":expression,"returnByValue":true,"awaitPromise":true}),
        )?;
        if result.get("exceptionDetails").is_some() {
            return Err(pipe_error("page observation failed"));
        }
        Ok(result["result"]["value"].clone())
    }

    fn settle(&mut self) -> Result<(), DriverError> {
        // Readiness wait is bounded by the same absolute action deadline;
        // no network-idle heuristic that can hang on analytics/WebSockets.
        self.evaluate("new Promise(resolve=>{if(document.readyState==='complete')resolve(true);else window.addEventListener('load',()=>resolve(true),{once:true})})")?;
        Ok(())
    }

    /// Execute once, with no replay of possibly side-effecting actions.
    /// The owner must discard this session after a pipe/protocol failure.
    pub async fn execute(
        &mut self,
        action: &BrowserAction,
        submit: bool,
    ) -> Result<ActionResult, DriverError> {
        self.execute_until(action, submit, Instant::now() + Duration::from_secs(45))
            .await
    }

    /// Execute without extending a fetch/render or crawl deadline.
    pub async fn execute_until(
        &mut self,
        action: &BrowserAction,
        submit: bool,
        deadline: Instant,
    ) -> Result<ActionResult, DriverError> {
        self.deadline = deadline;
        let mut screenshot = String::new();
        match action {
            BrowserAction::Navigate { url } => {
                self.admit(url).await?;
                let result = self.command("Page.navigate", json!({"url":url}))?;
                if result.get("errorText").is_some() {
                    return Err(pipe_error("navigation failed"));
                }
                self.settle()?;
            }
            BrowserAction::Click { index } => {
                let object = self.object(*index)?;
                let bounds = self.call_object(&object, "function(){this.scrollIntoView({block:'center',inline:'center'});const r=this.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2,href:this.closest('a[href]')?.href}}", json!([]))?;
                if let Some(url) = bounds["href"].as_str() {
                    self.admit(url).await?;
                }
                for kind in ["mousePressed", "mouseReleased"] {
                    self.command("Input.dispatchMouseEvent", json!({"type":kind,"x":bounds["x"],"y":bounds["y"],"button":"left","clickCount":1}))?;
                }
            }
            BrowserAction::Type {
                index,
                text,
                clear_first,
            } => {
                let object = self.object(*index)?;
                self.call_object(&object, "function(clear){this.focus();if(clear){if(this.isContentEditable){const r=document.createRange();r.selectNodeContents(this);const s=getSelection();s.removeAllRanges();s.addRange(r)}else this.select()}}", json!([{"value":clear_first}]))?;
                self.command("Input.insertText", json!({"text":text}))?;
                if submit {
                    for kind in ["keyDown", "keyUp"] {
                        self.command("Input.dispatchKeyEvent", json!({"type":kind,"key":"Enter","code":"Enter","windowsVirtualKeyCode":13}))?;
                    }
                }
            }
            BrowserAction::SelectOption { index, value } => {
                let object = self.object(*index)?;
                let selected = self.call_object(&object, "function(value){if(this.tagName!=='SELECT'||!Array.from(this.options).some(o=>o.value===value))return false;this.value=value;this.dispatchEvent(new Event('input',{bubbles:true}));this.dispatchEvent(new Event('change',{bubbles:true}));return true}", json!([{"value":value}]))?;
                if selected != true {
                    return Err(DriverError::Invalid("option does not exist".into()));
                }
            }
            BrowserAction::Scroll { dy, index } => {
                if let Some(index) = index {
                    let object = self.object(*index)?;
                    self.call_object(
                        &object,
                        "function(dy){this.scrollBy(0,dy)}",
                        json!([{"value":dy}]),
                    )?;
                } else {
                    self.evaluate(&format!("window.scrollBy(0,{dy})"))?;
                }
            }
            BrowserAction::Back | BrowserAction::Forward => {
                let history = self.command("Page.getNavigationHistory", json!({}))?;
                let current = history["currentIndex"].as_i64().unwrap_or(0);
                let next = current
                    + if matches!(action, BrowserAction::Back) {
                        -1
                    } else {
                        1
                    };
                let entry = usize::try_from(next)
                    .ok()
                    .and_then(|i| history["entries"].get(i))
                    .ok_or_else(|| DriverError::Invalid("history boundary".into()))?;
                let url = entry["url"]
                    .as_str()
                    .ok_or_else(|| pipe_error("invalid history"))?;
                if url != "about:blank" {
                    self.admit(url).await?;
                }
                self.command(
                    "Page.navigateToHistoryEntry",
                    json!({"entryId":entry["id"]}),
                )?;
                self.settle()?;
            }
            BrowserAction::Reload => {
                let url = self.evaluate("location.href")?;
                if url != "about:blank" {
                    self.admit(url.as_str().unwrap_or_default()).await?;
                }
                self.command("Page.reload", json!({}))?;
                self.settle()?;
            }
            BrowserAction::Screenshot => {
                let result = self.command(
                    "Page.captureScreenshot",
                    json!({"format":"png","captureBeyondViewport":false}),
                )?;
                screenshot = result["data"].as_str().unwrap_or_default().into();
                if screenshot.len() > self.budget {
                    return Err(DriverError::Invalid("budget_exceeded: screenshot".into()));
                }
            }
            BrowserAction::Close => {
                self.command("Browser.close", json!({}))?;
                return Ok(ActionResult::ok(action, true, String::new()));
            }
        }
        self.command(
            "Runtime.releaseObjectGroup",
            json!({"objectGroup":"vesper-action"}),
        )?;
        let map = self.snapshot()?;
        let mut result = ActionResult::ok(action, true, map);
        result.screenshot_base64 = screenshot;
        Ok(result)
    }

    /// Rendered HTML enters the same pure perception pipeline as fetch.
    pub fn html(&mut self) -> Result<String, DriverError> {
        let html = self.evaluate("document.documentElement.outerHTML")?;
        let mut html = html.as_str().unwrap_or_default().to_owned();
        if html.len() > self.budget {
            return Err(DriverError::Invalid(
                "budget_exceeded: rendered HTML".into(),
            ));
        }
        // Keep ownership explicit: no DOM handles cross the host boundary.
        html.shrink_to_fit();
        Ok(html)
    }

    /// Final URL and actual main-document HTTP status, never an invented 200.
    pub fn page_identity(&mut self) -> Result<(String, u16), DriverError> {
        let url = self.evaluate("location.href")?;
        Ok((url.as_str().unwrap_or_default().into(), self.page_status))
    }

    fn snapshot(&mut self) -> Result<String, DriverError> {
        let result = self.command(
            "DOMSnapshot.captureSnapshot",
            json!({"computedStyles":REQUIRED_COMPUTED_STYLES,
            "includePaintOrder":true,"includeDOMRects":true}),
        )?;
        let capture: CaptureSnapshotResult =
            serde_json::from_value(result).map_err(|_| pipe_error("invalid snapshot"))?;
        let ax = self.command("Accessibility.getFullAXTree", json!({}))?;
        let mut accessible = HashMap::new();
        if let Some(nodes) = ax["nodes"].as_array() {
            for node in nodes {
                if let Some(id) = node["backendDOMNodeId"].as_i64() {
                    accessible.insert(
                        id,
                        (
                            node["role"]["value"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned(),
                            node["name"]["value"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned(),
                        ),
                    );
                }
            }
        }
        let mut current = HashMap::new();
        let mut map = String::new();
        let viewport = self.evaluate("({top:scrollY,height:innerHeight})")?;
        for doc_id in 0..capture.documents.len() {
            let mut doc = capture.materialize(doc_id);
            let mut visible = vec![true; doc.nodes.len()];
            let mut context = String::new();
            for (i, node) in doc.nodes.iter().enumerate() {
                visible[i] = node
                    .parent_index
                    .is_none_or(|p| visible.get(p).copied().unwrap_or(false))
                    && node.style("display") != Some("none")
                    && node.style("visibility") != Some("hidden")
                    && node.attr("aria-hidden") != Some("true")
                    && !matches!(
                        node.tag_name.as_str(),
                        "script" | "style" | "noscript" | "template"
                    )
                    && !vesper_web::interactable::is_sensitive_value(
                        node.attr("type"),
                        node.attr("autocomplete"),
                    );
                if visible[i] && node.node_type == 3 && context.len() < self.budget / 2 {
                    let text = node
                        .node_value
                        .as_deref()
                        .unwrap_or_default()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ");
                    if !text.is_empty() {
                        context.push_str(&text.chars().take(240).collect::<String>());
                        context.push('\n');
                    }
                }
            }
            if !context.is_empty() {
                map.push_str("[page context]\n");
                map.push_str(&context);
            }
            for node in &mut doc.nodes {
                if let Some((role, name)) = node.backend_node_id.and_then(|id| accessible.get(&id))
                {
                    if node.attr("role").is_none() {
                        node.attributes.push(("role".into(), role.clone()));
                    }
                    if node.attr("aria-label").is_none() {
                        node.attributes.push(("aria-label".into(), name.clone()));
                    }
                }
            }
            let live_indices: std::collections::HashSet<_> =
                vesper_web::selector_map::interactable_lines(&doc, &mut self.cache)
                    .into_iter()
                    .map(|line| line.index)
                    .collect();
            map.push_str(&serialize_interactable_map(&doc, &mut self.cache));
            for node in doc.nodes {
                if let Some(index) = node
                    .backend_node_id
                    .and_then(|id| self.cache.peek(id))
                    .filter(|index| live_indices.contains(index))
                {
                    if let Some(bounds) = node.bounds
                        && bounds[1]
                            > viewport["top"].as_f64().unwrap_or(0.0)
                                + viewport["height"].as_f64().unwrap_or(800.0)
                    {
                        map.push_str(&format!("[below fold: index {index}; scroll to reach]\n"));
                    }
                    current.insert(index, node);
                }
            }
        }
        for node in self.nodes.values() {
            if let Some(id) = node.backend_node_id
                && self
                    .cache
                    .peek(id)
                    .is_some_and(|index| !current.contains_key(&index))
            {
                self.cache.retire(id);
            }
        }
        self.nodes = current;
        let original = map.len();
        let mut end = original.min(self.budget);
        while !map.is_char_boundary(end) {
            end -= 1;
        }
        map.truncate(end);
        map.push_str(&format!(
            "[density: original_bytes={original} returned_bytes={end} truncated={}]",
            end < original
        ));
        Ok(map)
    }
}

// Sandbox CLI futures are synchronous today; this bridge also supports a
// backend waking asynchronously without polling or requiring a host runtime.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    struct WakeThread(std::thread::Thread);
    impl std::task::Wake for WakeThread {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Arc::new(WakeThread(std::thread::current())).into();
    let mut context = std::task::Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(output) => return output,
            std::task::Poll::Pending => std::thread::park(),
        }
    }
}

#[cfg(all(test, feature = "docker"))]
mod live_tests {
    use super::*;
    use vesper_sandbox::{DockerBackend, DockerSandboxConfig};

    #[tokio::test]
    #[ignore = "requires a real image/runtime and explicitly permits public website access"]
    async fn real_navigation_and_chunked_fetch() {
        use vesper_web::transport::{FetchRequest, FetchTransport};
        let image = std::env::var("VESPER_WEB_TEST_IMAGE").expect("set VESPER_WEB_TEST_IMAGE");
        let root = tempfile::tempdir().unwrap();
        let backend = Arc::new(DockerBackend::new(DockerSandboxConfig {
            image: Some(image),
            network: true,
            ..Default::default()
        }));
        let mut config = crate::WebSandboxRouteConfig::new(
            "/usr/local/bin/vesper-web-fetch".into(),
            root.path().into(),
        );
        config.egress.respect_robots = false;
        let route = Arc::new(WebSandboxPort::new(backend, config));
        let mut request = FetchRequest::new("https://www.rfc-editor.org/rfc/rfc9110.html");
        request.max_body_bytes = 128 * 1024;
        let page = route
            .fetch(&request)
            .await
            .expect("real chunked helper fetch");
        assert_eq!(page.status, 200);
        assert!(page.body.len() > 64 * 1024);
        assert!(page.body.len() <= 128 * 1024 && page.truncated);
        let mut browser = BrowserSession::open(route, 512 * 1024).await.unwrap();
        browser
            .execute(
                &BrowserAction::Navigate {
                    url: "https://example.com/".into(),
                },
                false,
            )
            .await
            .expect("real admitted browser navigation");
        let (url, status) = browser.page_identity().unwrap();
        assert!(url.starts_with("https://example.com"));
        assert_eq!(status, 200);
        assert!(browser.html().unwrap().contains("Example Domain"));
    }

    #[tokio::test]
    #[ignore = "requires VESPER_WEB_TEST_IMAGE and a real container runtime"]
    async fn real_pipe_browser_actions_redaction_and_stale_indices() {
        let image = std::env::var("VESPER_WEB_TEST_IMAGE")
            .expect("set VESPER_WEB_TEST_IMAGE; this acceptance test never silently skips");
        let root = tempfile::tempdir().unwrap();
        let backend = Arc::new(DockerBackend::new(DockerSandboxConfig {
            image: Some(image),
            network: true,
            ..Default::default()
        }));
        let mut config = crate::WebSandboxRouteConfig::new(
            "/usr/local/bin/vesper-web-fetch".into(),
            root.path().into(),
        );
        config.egress.allow_plain_http = true;
        config.egress.respect_robots = false;
        let route = Arc::new(WebSandboxPort::new(backend, config));
        let mut browser = BrowserSession::open(route.clone(), 512 * 1024)
            .await
            .expect("real pipe smoke");
        assert_ne!(browser.route_id(), 0);
        assert_eq!(browser.route_id(), route.instance_id());
        let frame = browser.command("Page.getFrameTree", json!({})).unwrap();
        browser.command("Page.setDocumentContent", json!({"frameId":frame["frameTree"]["frame"]["id"],"html":
            "<html><body><h1>Acceptance</h1><button id='go' onclick='this.textContent=\"Clicked\"'>Go</button><input id='secret' type='password' aria-label='Secret' value='canary-before'><select id='choice' aria-label='Choice'><option value='a'>A</option><option value='b'>B</option></select><div id='listener'>Listener</div><script>document.querySelector('#listener').addEventListener('click',()=>{})</script></body></html>"})).unwrap();
        let map = browser.snapshot().unwrap();
        assert!(map.contains("Go"), "{map}");
        assert!(!map.contains("canary-before"));
        let find = |browser: &BrowserSession, name: &str| {
            *browser
                .nodes
                .iter()
                .find(|(_, node)| node.attr("id") == Some(name))
                .unwrap()
                .0
        };
        let button = find(&browser, "go");
        let secret = find(&browser, "secret");
        let choice = find(&browser, "choice");
        let _listener = find(&browser, "listener");
        let clicked = browser
            .execute(&BrowserAction::Click { index: button }, false)
            .await
            .unwrap();
        assert!(clicked.map.unwrap().contains("Clicked"));
        assert_eq!(find(&browser, "go"), button);
        let typed = browser
            .execute(
                &BrowserAction::Type {
                    index: secret,
                    text: "canary-after".into(),
                    clear_first: true,
                },
                false,
            )
            .await
            .unwrap();
        assert!(!format!("{typed:?}").contains("canary-after"));
        assert_eq!(
            browser
                .evaluate("document.querySelector('#secret').value")
                .unwrap(),
            "canary-after"
        );
        browser
            .execute(
                &BrowserAction::SelectOption {
                    index: choice,
                    value: "b".into(),
                },
                false,
            )
            .await
            .unwrap();
        assert_eq!(
            browser
                .evaluate("document.querySelector('#choice').value")
                .unwrap(),
            "b"
        );
        browser
            .evaluate("document.body.style.height='4000px'")
            .unwrap();
        browser
            .execute(
                &BrowserAction::Scroll {
                    dy: 500,
                    index: None,
                },
                false,
            )
            .await
            .unwrap();
        assert!(
            browser
                .evaluate("window.scrollY")
                .unwrap()
                .as_f64()
                .unwrap()
                > 0.0
        );
        let screenshot = browser
            .execute(&BrowserAction::Screenshot, false)
            .await
            .unwrap();
        assert!(screenshot.screenshot_base64.starts_with("iVBOR"));
        assert!(screenshot.screenshot_base64.len() <= 512 * 1024);
        browser
            .evaluate("document.querySelector('#go').remove()")
            .unwrap();
        assert!(matches!(
            browser
                .execute(&BrowserAction::Click { index: button }, false)
                .await,
            Err(DriverError::UnknownIndex(_))
        ));
        assert!(
            browser
                .execute(
                    &BrowserAction::Navigate {
                        url: "https://127.0.0.1/".into()
                    },
                    false
                )
                .await
                .is_err()
        );
        browser
            .execute(&BrowserAction::Reload, false)
            .await
            .unwrap();
        assert!(matches!(
            browser.execute(&BrowserAction::Back, false).await,
            Err(DriverError::Invalid(_))
        ));
        // Deadline expiry is a real pipe failure, not a successful fake action.
        browser.deadline = Instant::now() - Duration::from_millis(1);
        assert!(matches!(
            browser.command("Runtime.evaluate", json!({"expression":"1"})),
            Err(DriverError::Pipe(_))
        ));
        drop(browser);
    }
}
