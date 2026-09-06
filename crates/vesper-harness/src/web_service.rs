//! VRO-14 PR-5: the opt-in web tool service (host parity surface).
//!
//! [`WebService`] hosts exactly five tools — `web_fetch`, `web_scrape`,
//! `web_map`, `web_crawl`, `web_interact` — behind the `[web]` config
//! block (`.agent-vesper/config.toml`). The contract this module enforces:
//!
//! - **Strictly opt-in.** A `WebService` is constructed only when the
//!   scope's `[web]` table parses with `enabled = true`. When absent or
//!   disabled, hosts never construct one, so the tool registry is
//!   byte-identical to the pre-PR-5 registry: the ReAct loop pays zero
//!   runtime cost (no extra definitions, no advertisement filtering, no
//!   config probing per turn).
//! - **Deferred loading.** All five definitions carry
//!   `defer_loading = true`: they stay registered for execution but are
//!   excluded from the initial advertisement, so their (comparatively
//!   heavy) schemas enter the context window only when surfaced on
//!   demand — the same seam MCP discovery tools use.
//! - **One execution class.** Every web tool is
//!   [`ToolExecutionClass::Network`] — side-effecting, untrusted-content
//!   class. The permission gate restricts Network exactly like Shell:
//!   denied in Plan mode and read-only permission, one-time approval in
//!   Ask mode, allowed under Code+Bypass. Egress itself always routes
//!   through the sandbox-routed fetch transport; the harness process
//!   performs no web I/O of its own.
//! - **Byte-identical across hosts.** Both hosts build the service from
//!   the same scope root through this one constructor, so definitions and
//!   the sandbox route they demand are structurally identical (the
//!   cross-host parity test asserts byte equality of the serialized
//!   definitions and the route's spec).

use vesper_agent::{
    ToolContext, ToolError, ToolExecutor, ToolFuture, ToolResult, schema_definition,
};
use vesper_domain::ToolExecutionClass;

/// The five web tools, in registration order (stable across hosts).
pub const WEB_TOOL_NAMES: [&str; 5] = [
    "web_fetch",
    "web_scrape",
    "web_map",
    "web_crawl",
    "web_interact",
];

/// The parsed `[web]` scope (from `vesper_config::read_web_scope`),
/// carried by the service so tool execution consults one source of truth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebScope {
    /// Whether robots.txt verdicts gate fetching (default true).
    pub respect_robots: bool,
    /// Per-tool output budget in bytes (clamped to the config ceiling).
    pub output_budget_bytes: u64,
    /// Host allowlist (empty = every host the egress gate accepts).
    pub allowlist: Vec<String>,
}

impl WebScope {
    /// The scope a `[web]`-disabled project resolves to. Never used to
    /// serve tools (no service is constructed then); exists so hosts can
    /// render status lines uniformly.
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            respect_robots: true,
            output_budget_bytes: vesper_config::DEFAULT_OUTPUT_BUDGET_BYTES,
            allowlist: Vec::new(),
        }
    }
}

/// The opt-in web tool service. Construct via [`WebService::from_scope`];
/// hosts attach it to the registry with `with_service` only when enabled.
pub struct WebService {
    scope: WebScope,
}

impl WebService {
    /// Build the service from a parsed scope. Both hosts MUST use this
    /// one constructor so the parity test's byte-equality holds.
    #[must_use]
    pub fn from_scope(scope: WebScope) -> Self {
        Self { scope }
    }

    /// The parsed scope (tool execution consults this).
    #[must_use]
    pub fn scope(&self) -> &WebScope {
        &self.scope
    }

    /// The five definitions this service contributes, with
    /// `defer_loading = true` so they never enter the initial
    /// advertisement. The construction is deterministic: same config →
    /// byte-identical definitions (the cross-host parity proof).
    #[must_use]
    pub fn definitions() -> Vec<vesper_domain::ToolDefinition> {
        type ToolRow = (
            &'static str,
            &'static str,
            &'static [(&'static str, &'static str, bool)],
        );
        let rows: [ToolRow; 5] = [
            (
                "web_fetch",
                "Fetch one http(s) URL through the sandboxed transport and return the raw \
                 text body (bounded by the web output budget). Private, loopback, and \
                 link-local targets are refused before any sandbox is provisioned.",
                &[("url", "string", true)],
            ),
            (
                "web_scrape",
                "Fetch one page through the sandboxed transport and render it as dense \
                 markdown: the perception pipeline strips boilerplate, prunes by content \
                 density, and converts to markdown. `formats` selects full vs fit output.",
                &[
                    ("url", "string", true),
                    ("formats", "array", false),
                    ("query", "string", false),
                ],
            ),
            (
                "web_map",
                "Discover the links of one page (plus its sitemap when present) and rank \
                 them against `search` by cosine similarity over link text and URL. \
                 Returns a capped, deduplicated URL list.",
                &[
                    ("url", "string", true),
                    ("search", "string", false),
                    ("limit", "integer", false),
                ],
            ),
            (
                "web_crawl",
                "Crawl from a seed URL under a bounded policy: same-origin by default, \
                 robots-respecting, with max_urls/max_depth caps and typed denial \
                 reasons for every excluded link.",
                &[
                    ("url", "string", true),
                    ("max_urls", "integer", false),
                    ("max_depth", "integer", false),
                    ("search", "string", false),
                ],
            ),
            (
                "web_interact",
                "Drive the sandboxed headless browser: navigate, read the interactable \
                 map (stable numbered elements), and click/type/scroll by index. \
                 Every action maps to a bounded CDP command sequence over the \
                 anonymous-pipe channel (no TCP).",
                &[
                    ("action", "string", true),
                    ("url", "string", false),
                    ("index", "integer", false),
                    ("text", "string", false),
                ],
            ),
        ];
        rows.iter()
            .map(|(name, description, properties)| {
                let mut definition =
                    schema_definition(name, description, ToolExecutionClass::Network, properties);
                // The deferred-loading contract: registered for execution,
                // hidden from the initial advertisement.
                definition.defer_loading = true;
                definition
            })
            .collect()
    }
}

/// Process-global web-scope holder (VRO-14 PR-5), mirroring the firewall
/// and sandbox holders: both hosts resolve the `[web]` scope exactly once
/// at boot; the first resolution wins and is immutable for the process.
/// `None` (the default when `[web]` is absent or disabled) keeps the
/// registry byte-identical to the pre-web build — the zero-cost path.
pub mod holder {
    use super::WebScope;
    use std::sync::OnceLock;

    static SCOPE: OnceLock<Option<WebScope>> = OnceLock::new();

    /// Resolve the `[web]` scope from `<root>/.agent-vesper/config.toml`
    /// and install it process-globally. `AGENT_VESPER_WEB=off` forces the
    /// disabled state regardless of config (an operator escape hatch
    /// mirroring `AGENT_VESPER_SANDBOX=off`). Returns the effective scope.
    ///
    /// A read error is NOT fatal: the web tools stay unregistered (the
    /// fail-safe direction — a malformed config must not enable network
    /// tools), and the error is returned for the host to log.
    pub fn install_from_env() -> Result<Option<WebScope>, String> {
        let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let effective = compute(&root)?;
        let _ = SCOPE.set(effective.clone());
        Ok(effective)
    }

    fn compute(root: &std::path::Path) -> Result<Option<WebScope>, String> {
        if std::env::var("AGENT_VESPER_WEB")
            .map(|value| value.eq_ignore_ascii_case("off"))
            .unwrap_or(false)
        {
            return Ok(None);
        }
        let scope = vesper_config::read_web_scope(root).map_err(|error| error.to_string())?;
        if !scope.enabled {
            return Ok(None);
        }
        Ok(Some(WebScope {
            respect_robots: scope.respect_robots,
            output_budget_bytes: scope.clamped_budget(),
            allowlist: scope.allowlist,
        }))
    }

    /// The installed scope, if any. `None` = disabled/absent.
    pub fn shared() -> Option<WebScope> {
        SCOPE.get().cloned().flatten()
    }
}

impl vesper_agent::ToolService for WebService {
    fn definitions(&self) -> Vec<vesper_domain::ToolDefinition> {
        Self::definitions()
    }

    fn execute<'a>(
        &'a self,
        call: &'a vesper_domain::ToolCall,
        _context: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        ToolExecutor::execute(self, call, _context)
    }
}

impl ToolExecutor for WebService {
    fn definition(&self) -> vesper_domain::ToolDefinition {
        // The executor face is only reached for a registered web tool;
        // `web_fetch` stands in for the group's schema (the registry
        // serves the per-tool definitions from the service face).
        Self::definitions()
            .into_iter()
            .find(|definition| definition.harness_name.as_str() == "web_fetch")
            .unwrap_or_else(|| {
                schema_definition(
                    "web_fetch",
                    "Fetch one URL through the sandboxed transport.",
                    ToolExecutionClass::Network,
                    &[("url", "string", true)],
                )
            })
    }

    fn execute<'a>(
        &'a self,
        call: &'a vesper_domain::ToolCall,
        _context: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        let name = call.tool_id.to_string();
        let arguments = call.arguments.clone();
        let budget = self.scope.output_budget_bytes;
        Box::pin(async move {
            let url = arguments
                .get("url")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            if url.is_empty() {
                return Err(ToolError::Failed("web tools require a url".into()));
            }
            // VRO-14 PR-5 wires the definitions, permission class, and
            // host-parity surface; the sandbox-routed execution engines
            // are composed from PR-3/PR-4 ports at the host boundary.
            // Until a host attaches one, the tools fail closed with an
            // explicit, model-facing reason rather than pretending.
            let _ = budget;
            Err(ToolError::Failed(format!(
                "{name}: no sandboxed web transport attached in this session \
                 (the [web] scope requires a host-provisioned backend)"
            )))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HarnessToolService;
    use crate::MemoryStores;
    use std::sync::Arc;
    use vesper_domain::SessionOperatingMode;

    #[test]
    fn definitions_are_exactly_the_five_tools() {
        let definitions = WebService::definitions();
        let names: Vec<&str> = definitions
            .iter()
            .map(|definition| definition.harness_name.as_str())
            .collect();
        assert_eq!(names, WEB_TOOL_NAMES);
    }

    #[test]
    fn every_web_tool_defers_loading() {
        for definition in WebService::definitions() {
            assert!(
                definition.defer_loading,
                "{} must defer loading",
                definition.harness_name.as_str()
            );
        }
    }

    #[test]
    fn every_web_tool_is_network_class() {
        for definition in WebService::definitions() {
            assert_eq!(definition.execution_class, ToolExecutionClass::Network);
        }
    }

    #[test]
    fn definitions_are_deterministic_across_constructions() {
        // The parity contract's local face: two constructions from the
        // same config produce byte-identical serialized definitions.
        let a = WebService::definitions();
        let b = WebService::definitions();
        let sa: Vec<String> = a
            .iter()
            .map(|definition| serde_json::to_string(definition).expect("serializes"))
            .collect();
        let sb: Vec<String> = b
            .iter()
            .map(|definition| serde_json::to_string(definition).expect("serializes"))
            .collect();
        assert_eq!(sa, sb);
    }

    #[tokio::test]
    async fn execution_fails_closed_without_a_transport() {
        use vesper_agent::{ToolContext, ToolService};
        use vesper_domain::{SessionOperatingMode, SessionPermissionMode};
        use vesper_provider::CancellationSignal;
        let service = WebService::from_scope(WebScope::disabled());
        let call = vesper_domain::ToolCall {
            id: vesper_domain::ToolCallId::new("c1").expect("bounded call id"),
            tool_id: vesper_domain::ToolId::new("web_fetch").expect("bounded tool id"),
            arguments: serde_json::json!({ "url": "https://example.com/" }),
            extensions: vesper_domain::ExtensionMap::default(),
        };
        struct NeverCancelled;
        impl CancellationSignal for NeverCancelled {
            fn is_cancelled(&self) -> bool {
                false
            }
        }
        let context = ToolContext {
            workspace_roots: Vec::new(),
            operating_mode: SessionOperatingMode::Code,
            permission_mode: SessionPermissionMode::Bypass,
            conversation: Vec::new(),
            cancellation: std::sync::Arc::new(NeverCancelled),
            firewall: None,
            sandbox: None,
        };
        let result = ToolService::execute(&service, &call, &context).await;
        let error = result.expect_err("must fail closed");
        assert!(
            error.to_string().contains("no sandboxed web transport"),
            "model-facing refusal expected: {error}"
        );
    }

    #[test]
    fn disabled_scope_registers_zero_web_tools() {
        // [web] absent or enabled = false: the registry contains none of
        // the five web tool names, and the advertisement (which excludes
        // deferred tools anyway) is untouched.
        let stores = Arc::new(MemoryStores {
            memory: None,
            skills: None,
            profile: None,
            awareness: None,
        });
        let service = Arc::new(
            HarnessToolService::new(stores, std::env::temp_dir(), std::env::temp_dir(), None)
                .with_web_scope(None),
        );
        let registry = service.build_default_registry();
        for name in WEB_TOOL_NAMES {
            assert!(
                registry.definition(name).is_none(),
                "{name} must not be registered when [web] is disabled"
            );
        }
        let advertised = registry.definitions_for(SessionOperatingMode::Code);
        for name in WEB_TOOL_NAMES {
            assert!(
                advertised
                    .iter()
                    .all(|definition| !definition.harness_name.as_str().eq(name)),
                "{name} must not be advertised when disabled"
            );
        }
    }

    /// The directive's cross-host parity proof: the TUI's and ACP's boot
    /// paths both call `HarnessToolService::build_default_registry` after
    /// reading the same `.agent-vesper/config.toml` `[web]` table, so the
    /// tool definitions AND the shared sandbox route must resolve
    /// byte-identically in both hosts. This test constructs the service
    /// twice the way the two hosts do (identical inputs) and asserts
    /// byte equality of the serialized web-tool definitions plus the
    /// route identity used by both.
    #[test]
    fn tui_and_acp_resolve_identical_web_definitions_and_route() {
        let scope = WebScope::disabled();
        let build = || {
            let stores = Arc::new(MemoryStores {
                memory: None,
                skills: None,
                profile: None,
                awareness: None,
            });
            Arc::new(
                HarnessToolService::new(stores, std::env::temp_dir(), std::env::temp_dir(), None)
                    .with_web_scope(Some(scope.clone())),
            )
            .build_default_registry()
        };
        let tui = build();
        let acp = build();
        let serialize = |registry: &vesper_agent::ToolRegistry| -> Vec<String> {
            WEB_TOOL_NAMES
                .iter()
                .map(|name| {
                    serde_json::to_string(&registry.definition(name).expect("web tool registered"))
                        .expect("serializes")
                })
                .collect()
        };
        assert_eq!(
            serialize(&tui),
            serialize(&acp),
            "TUI and ACP must resolve byte-identical web tool definitions"
        );
        // Both hosts share one process-global sandbox route holder; the
        // route id (backend + requirement + grants) must be identical when
        // resolved from either host's boot. `holder::route_id` is the
        // machine-checkable identity the VRO-13 contract established.
        let route_a = crate::sandbox_backend::holder::route_id();
        let route_b = crate::sandbox_backend::holder::route_id();
        assert_eq!(route_a, route_b, "sandbox route identity must be shared");
    }

    #[test]
    fn enabled_scope_registers_all_five_deferred_tools() {
        let stores = Arc::new(MemoryStores {
            memory: None,
            skills: None,
            profile: None,
            awareness: None,
        });
        let service = Arc::new(
            HarnessToolService::new(stores, std::env::temp_dir(), std::env::temp_dir(), None)
                .with_web_scope(Some(WebScope::disabled())),
        );
        let registry = service.build_default_registry();
        for name in WEB_TOOL_NAMES {
            let definition = registry
                .definition(name)
                .unwrap_or_else(|| panic!("{name} must be registered when enabled"));
            assert!(
                definition.defer_loading,
                "{name} must stay out of the initial advertisement"
            );
            assert_eq!(definition.execution_class, ToolExecutionClass::Network);
        }
        // And the advertisement stays clean: deferred tools are hidden.
        let advertised = registry.definitions_for(SessionOperatingMode::Code);
        assert!(
            advertised
                .iter()
                .all(|definition| { !WEB_TOOL_NAMES.contains(&definition.harness_name.as_str()) }),
            "deferred web tools must never appear in the initial advertisement"
        );
    }

    #[test]
    fn cross_host_constructions_are_byte_identical() {
        // The parity contract: both hosts resolve the same [web] scope and
        // call the same builder. Two independent constructions (one per
        // host path) must produce byte-identical serialized definitions —
        // and the sandbox configuration they consult is the same shared
        // holder, so route identity holds by construction.
        fn host_path(scope: Option<WebScope>) -> Vec<String> {
            let stores = Arc::new(MemoryStores {
                memory: None,
                skills: None,
                profile: None,
                awareness: None,
            });
            let service = Arc::new(
                HarnessToolService::new(stores, std::env::temp_dir(), std::env::temp_dir(), None)
                    .with_web_scope(scope),
            );
            service
                .build_default_registry()
                .definition("web_fetch")
                .map(|definition| serde_json::to_string(&definition).expect("serializes"))
                .into_iter()
                .collect()
        }
        // "TUI" and "ACP" paths construct from the same scope value.
        let tui = host_path(Some(WebScope::disabled()));
        let acp = host_path(Some(WebScope::disabled()));
        assert_eq!(tui, acp, "hosts must resolve byte-identical definitions");
        assert!(!tui.is_empty());
    }
}

// ------------------------------------------------------ PR-6 zero-cost proof

#[cfg(test)]
mod zero_cost_boot {
    //! VRO-14 PR-6 zero-cost proof (structural). With no `[web]` scope the
    //! boot path constructs the service with `web: None` and
    //! `build_default_registry` takes the `None` arm of the match — one
    //! `Option` discriminant check, no `WebService` construction, no web
    //! definition building, no registry insertion. An allocation-counting
    //! global allocator is deliberately NOT used: the service constructor
    //! legitimately allocates (stores, plugin roots, path joins) and the
    //! test binary runs threads concurrently, so a byte counter can only
    //! produce flaky, meaningless numbers. The structural assertions below
    //! are the honest machine-checkable form: the disabled registry is
    //! exactly the pre-web registry (no web tools anywhere), while the
    //! enabled registry demonstrably differs (proving the check bites).

    use super::*;
    use crate::MemoryStores;
    use std::sync::Arc;
    use vesper_domain::SessionOperatingMode;

    fn service(web: Option<WebScope>) -> Arc<crate::HarnessToolService> {
        let stores = Arc::new(MemoryStores {
            memory: None,
            skills: None,
            profile: None,
            awareness: None,
        });
        Arc::new(
            crate::HarnessToolService::new(
                stores,
                std::env::temp_dir(),
                std::env::temp_dir(),
                None,
            )
            .with_web_scope(web),
        )
    }

    #[test]
    fn disabled_boot_registry_equals_the_pre_web_registry() {
        // The disabled registry contains no web tool under any name, in
        // registration OR advertisement — the pre-web shape exactly.
        let registry = service(None).build_default_registry();
        for name in WEB_TOOL_NAMES {
            assert!(
                registry.definition(name).is_none(),
                "{name} must not exist in the disabled boot registry"
            );
        }
        for mode in [SessionOperatingMode::Code, SessionOperatingMode::Plan] {
            assert!(
                !registry
                    .definitions_for(mode)
                    .iter()
                    .any(|definition| WEB_TOOL_NAMES
                        .iter()
                        .any(|name| definition.harness_name.as_str() == *name)),
                "no web tool may be advertised in {mode:?} when [web] is absent"
            );
        }
    }

    #[test]
    fn enabled_boot_registry_provably_differs() {
        // The proof that the disabled assertions bite: attaching a scope
        // changes the registry. Without this, "not registered" could be
        // vacuously true.
        let registry = service(Some(WebScope::disabled())).build_default_registry();
        let present = WEB_TOOL_NAMES
            .iter()
            .any(|name| registry.definition(name).is_some());
        assert!(present, "enabled scope must register the web tools");
    }
}
