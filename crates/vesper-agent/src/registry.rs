//! Tool registry (ADR 0010, Tier C Phase 1).
//!
//! Maps the nine parity-critical harness tool names to their real
//! [`ToolExecutor`] implementations, advertises mode-filtered [`ToolDefinition`]s to the model (mirroring
//! the Python oracle's `agent.py:2843-2872` eligibility), and dispatches a
//! normalized [`ToolCall`] to its executor. The registry owns no I/O — it only
//! routes; the executor owns the side effect.

use std::collections::BTreeMap;
use std::sync::Arc;

use vesper_domain::{SessionOperatingMode, ToolCall, ToolDefinition, ToolExecutionClass};

use crate::executor::{
    HostedTool, ToolError, ToolExecutor, ToolFuture, ToolResult, ToolService, harness_name,
};
use crate::tools::{
    ApplyPatch, EditFile, Grep, ListDirectory, ReadFile, RunCommand, SearchFiles, UpdatePlan,
    WriteFile,
};

/// One registered tool: its definition plus its executor.
#[derive(Clone)]
struct Entry {
    definition: ToolDefinition,
    executor: Arc<dyn ToolExecutor>,
}

/// One gateway executor keyed by a name prefix. The composition boundary
/// (harness) registers ONE gateway per prefix (e.g. `"mcp__"`) so dynamically
/// injected tool schemas whose `harness_name` starts with that prefix can be
/// executed without being pre-registered as full entries.
#[derive(Clone)]
struct GatewayEntry {
    prefix: String,
    executor: Arc<dyn ToolExecutor>,
}

/// The parity-critical tool registry.
#[derive(Clone)]
pub struct ToolRegistry {
    entries: BTreeMap<String, Entry>,
    /// Gateway executors consulted when a call name is not in `entries` but
    /// matches a registered prefix. The longest-prefix match wins so a
    /// narrower gateway (e.g. `"mcp__playwright__"`) can override a broader
    /// one (e.g. `"mcp__"`).
    gateways: Vec<GatewayEntry>,
}

impl ToolRegistry {
    /// Creates a registry with no tools for provider-only advisory calls.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            entries: BTreeMap::new(),
            gateways: Vec::new(),
        }
    }

    /// Creates a registry populated with the nine parity-critical core tools.
    ///
    /// Tools are registered in stable harness-name order so `definitions_for`
    /// advertises them deterministically to the model.
    #[must_use]
    pub fn parity_default() -> Self {
        let core_tools: [(&str, ToolExecutionClass, Arc<dyn ToolExecutor>); 9] = [
            (
                "read_file",
                ToolExecutionClass::ReadOnly,
                Arc::new(ReadFile),
            ),
            (
                "list_directory",
                ToolExecutionClass::ReadOnly,
                Arc::new(ListDirectory),
            ),
            (
                "search_files",
                ToolExecutionClass::ReadOnly,
                Arc::new(SearchFiles),
            ),
            ("grep", ToolExecutionClass::ReadOnly, Arc::new(Grep)),
            (
                "write_file",
                ToolExecutionClass::Mutating,
                Arc::new(WriteFile),
            ),
            (
                "edit_file",
                ToolExecutionClass::Mutating,
                Arc::new(EditFile),
            ),
            (
                "apply_patch",
                ToolExecutionClass::Mutating,
                Arc::new(ApplyPatch),
            ),
            (
                "run_command",
                ToolExecutionClass::Shell,
                Arc::new(RunCommand),
            ),
            (
                "update_plan",
                ToolExecutionClass::ReadOnly,
                Arc::new(UpdatePlan),
            ),
        ];
        let mut entries = BTreeMap::new();
        for (_name, _expected_class, executor) in core_tools {
            let definition = executor.definition();
            let key = harness_name(&definition);
            entries.insert(
                key,
                Entry {
                    definition,
                    executor,
                },
            );
        }
        Self {
            entries,
            gateways: Vec::new(),
        }
    }

    /// Adds all definitions contributed by a composition-boundary service.
    ///
    /// A duplicate name is rejected by retaining the first registration;
    /// callers can inspect the final registry and fail startup if a provider
    /// or plugin attempted to shadow a core capability.
    #[must_use]
    pub fn with_service(mut self, service: Arc<dyn ToolService>) -> Self {
        for definition in service.definitions() {
            let key = harness_name(&definition);
            self.entries.entry(key).or_insert_with(|| Entry {
                definition: definition.clone(),
                executor: Arc::new(HostedTool::new(definition, Arc::clone(&service))),
            });
        }
        self
    }

    /// Registers a gateway executor for a name prefix. When [`execute`]
    /// encounters a tool name not present in `entries` but matching a
    /// registered prefix, it routes the call to the matching gateway. The
    /// composition boundary (harness) uses this to wire dynamically
    /// discovered MCP tools (named `mcp__<server>__<tool>`) to a single
    /// gateway executor that dispatches to the MCP client. An empty prefix
    /// is ignored. If two gateways' prefixes both match, the longest one
    /// wins.
    #[must_use]
    pub fn with_gateway(
        mut self,
        prefix: impl Into<String>,
        executor: Arc<dyn ToolExecutor>,
    ) -> Self {
        let prefix = prefix.into();
        if !prefix.is_empty() {
            self.gateways.push(GatewayEntry { prefix, executor });
        }
        self
    }

    /// Whether a gateway is registered for `prefix`.
    #[must_use]
    pub fn has_gateway(&self, prefix: &str) -> bool {
        self.gateways.iter().any(|gateway| gateway.prefix == prefix)
    }

    /// Number of registered tools.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether any tools are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the definitions advertised to the model under `mode`.
    ///
    /// `Plan` mode exposes only `ReadOnly` tools (read-only reconnaissance +
    /// `update_plan`); `Code` mode exposes every tool. This mirrors the
    /// oracle's mode-based eligibility (`agent.py:2843-2872`).
    ///
    /// In both modes, tools whose [`ToolDefinition::defer_loading`] is `true`
    /// are excluded from the advertised list — they remain registered for
    /// execution if the model (or a host) calls them by name, but they do not
    /// appear in the initial context-window advertisement. This is the
    /// Claude Code-style deferred-loading seam.
    #[must_use]
    pub fn definitions_for(&self, mode: SessionOperatingMode) -> Vec<ToolDefinition> {
        self.entries
            .values()
            .filter(|entry| !entry.definition.defer_loading)
            .filter(|entry| match mode {
                SessionOperatingMode::Code => true,
                SessionOperatingMode::Plan => {
                    matches!(
                        entry.definition.execution_class,
                        ToolExecutionClass::ReadOnly
                    )
                }
            })
            .map(|entry| entry.definition.clone())
            .collect()
    }

    /// Returns the definition for `name`, if registered.
    #[must_use]
    pub fn definition(&self, name: &str) -> Option<&ToolDefinition> {
        self.entries.get(name).map(|entry| &entry.definition)
    }

    /// Whether a tool named `name` is registered. A name that is not in
    /// `entries` but matches a registered gateway prefix is also considered
    /// registered (it can be executed via the gateway).
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
            || self
                .gateways
                .iter()
                .any(|gateway| name.starts_with(&gateway.prefix))
    }

    /// Routes `call` to its executor and runs it under `context`.
    ///
    /// Unknown tool names surface as [`ToolError::UnknownTool`]; the agent loop
    /// feeds that back to the model as the tool result so the turn recovers.
    /// When the call name is not in `entries` but matches a registered
    /// gateway prefix, the call routes to the longest-matching gateway
    /// executor instead.
    pub fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
        context: &'a crate::executor::ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        if let Some(entry) = self.entries.get(call.tool_id.as_str()) {
            let executor = Arc::clone(&entry.executor);
            return Box::pin(async move { executor.execute(call, context).await });
        }
        // Gateway fallback: longest-prefix match wins. A dynamically injected
        // schema (e.g. `mcp__server__tool`) advertises to the model on the
        // next turn and routes through here when the model calls it.
        let tool_name = call.tool_id.as_str();
        if let Some(executor) = self
            .gateways
            .iter()
            .filter(|gateway| tool_name.starts_with(&gateway.prefix))
            .max_by_key(|gateway| gateway.prefix.len())
            .map(|gateway| Arc::clone(&gateway.executor))
        {
            return Box::pin(async move { executor.execute(call, context).await });
        }
        let name = tool_name.to_string();
        Box::pin(async move { Err(ToolError::UnknownTool(name)) })
    }
}

#[cfg(test)]
mod tests {
    //! Registration, mode filtering, and dispatch routing.

    use super::*;
    use crate::executor::{ToolContext, schema_definition, uncancellable_context};
    use serde_json::json;
    use std::sync::Arc;
    use vesper_domain::{BoundedString, WorkspaceRoot};
    use vesper_domain::{
        SessionOperatingMode, SessionPermissionMode, ToolCall, ToolCallId, ToolExecutionClass,
    };

    fn call_for(name: &str) -> ToolCall {
        ToolCall {
            id: ToolCallId::new("call-1").unwrap(),
            tool_id: vesper_domain::ToolId::new(name).unwrap(),
            arguments: json!({"path": "src/lib.rs"}),
            extensions: vesper_domain::ExtensionMap::default(),
        }
    }

    #[test]
    fn parity_default_registers_nine_tools() {
        let registry = ToolRegistry::parity_default();
        assert_eq!(registry.len(), 9);
        for name in [
            "read_file",
            "list_directory",
            "search_files",
            "grep",
            "write_file",
            "edit_file",
            "apply_patch",
            "run_command",
            "update_plan",
        ] {
            assert!(registry.contains(name), "missing parity tool {name}");
        }
    }

    #[test]
    fn plan_mode_advertises_only_readonly_tools() {
        let registry = ToolRegistry::parity_default();
        let plan_defs = registry.definitions_for(SessionOperatingMode::Plan);
        assert!(
            plan_defs.iter().all(|definition| matches!(
                definition.execution_class,
                ToolExecutionClass::ReadOnly
            )),
            "Plan mode must expose only ReadOnly tools"
        );
        // Mutating/Shell tools are absent in Plan mode.
        let plan_names: Vec<&str> = plan_defs
            .iter()
            .map(|definition| definition.harness_name.as_str())
            .collect();
        assert!(plan_names.contains(&"read_file"));
        assert!(plan_names.contains(&"update_plan"));
        assert!(!plan_names.contains(&"write_file"));
        assert!(!plan_names.contains(&"run_command"));
    }

    #[test]
    fn code_mode_advertises_every_tool() {
        let registry = ToolRegistry::parity_default();
        let code_defs = registry.definitions_for(SessionOperatingMode::Code);
        assert_eq!(code_defs.len(), 9);
    }

    #[tokio::test]
    async fn dispatch_routes_to_the_named_executor() {
        // read_file is a real executor now, so the registry test drives it
        // against a temp workspace root with a real file.
        let registry = ToolRegistry::parity_default();
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("note.txt"), "registry dispatch").unwrap();
        let roots = vec![WorkspaceRoot {
            name: BoundedString::new("workspace").unwrap(),
            path: BoundedString::new(root.path().to_string_lossy().to_string()).unwrap(),
            primary: true,
        }];
        let context: ToolContext = uncancellable_context(
            roots,
            SessionOperatingMode::Code,
            SessionPermissionMode::Ask,
        );
        let call = ToolCall {
            id: ToolCallId::new("call-1").unwrap(),
            tool_id: vesper_domain::ToolId::new("read_file").unwrap(),
            arguments: serde_json::json!({"path": "note.txt"}),
            extensions: vesper_domain::ExtensionMap::default(),
        };
        let result = registry.execute(&call, &context).await.unwrap();
        assert_eq!(result.text.as_str(), "registry dispatch");
    }

    #[tokio::test]
    async fn unknown_tool_surfaces_a_classified_error() {
        let registry = ToolRegistry::parity_default();
        let context = uncancellable_context(
            Vec::new(),
            SessionOperatingMode::Code,
            SessionPermissionMode::Ask,
        );
        let call = call_for("nonexistent_tool");
        let error = registry.execute(&call, &context).await.unwrap_err();
        assert!(matches!(error, ToolError::UnknownTool(_)));
    }

    // ---------------------- Deferred loading (Phase 1) ----------------------

    /// Stub service that contributes one tool with `defer_loading = true`.
    /// Used to prove the visibility seam: hidden from advertisement, still
    /// registered for execution.
    struct DeferredToolService;

    impl ToolService for DeferredToolService {
        fn definitions(&self) -> Vec<ToolDefinition> {
            vec![{
                let mut definition = schema_definition(
                    "deferred_research",
                    "A deferred-load tool the model discovers on demand.",
                    ToolExecutionClass::ReadOnly,
                    &[("query", "string", true)],
                );
                definition.defer_loading = true;
                definition
            }]
        }

        fn execute<'a>(
            &'a self,
            call: &'a ToolCall,
            _context: &'a ToolContext,
        ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
            let name = call.tool_id.as_str().to_owned();
            Box::pin(async move { ToolResult::new(format!("deferred tool `{name}` executed")) })
        }
    }

    fn registry_with_deferred() -> ToolRegistry {
        ToolRegistry::parity_default().with_service(Arc::new(DeferredToolService))
    }

    #[test]
    fn deferred_tool_remains_registered_but_is_hidden_from_advertisement() {
        let registry = registry_with_deferred();

        // Still counted as a registered tool.
        assert_eq!(
            registry.len(),
            10,
            "deferred tool must still occupy a registry slot"
        );
        assert!(
            registry.contains("deferred_research"),
            "deferred tool must remain registered for execution"
        );

        // Excluded from Code-mode advertisement.
        let code_defs = registry.definitions_for(SessionOperatingMode::Code);
        let code_names: Vec<&str> = code_defs
            .iter()
            .map(|definition| definition.harness_name.as_str())
            .collect();
        assert!(
            !code_names.contains(&"deferred_research"),
            "deferred tool must NOT be advertised in Code mode"
        );
        assert_eq!(
            code_defs.len(),
            9,
            "Code-mode advertisement keeps only the 9 advertised parity tools"
        );

        // Excluded from Plan-mode advertisement too.
        let plan_defs = registry.definitions_for(SessionOperatingMode::Plan);
        let plan_names: Vec<&str> = plan_defs
            .iter()
            .map(|definition| definition.harness_name.as_str())
            .collect();
        assert!(
            !plan_names.contains(&"deferred_research"),
            "deferred tool must NOT be advertised in Plan mode either"
        );
    }

    #[test]
    fn deferred_tool_definition_remains_visible_via_definition_lookup() {
        let registry = registry_with_deferred();
        let looked_up = registry
            .definition("deferred_research")
            .expect("deferred tool must be discoverable by name for execution routing");
        assert!(
            looked_up.defer_loading,
            "the registered definition must retain the defer_loading flag"
        );
        // Sanity: an advertised tool also remains visible here.
        assert!(registry.definition("read_file").is_some());
    }

    #[tokio::test]
    async fn deferred_tool_remains_executable_when_called_by_name() {
        let registry = registry_with_deferred();
        let context = uncancellable_context(
            Vec::new(),
            SessionOperatingMode::Code,
            SessionPermissionMode::Bypass,
        );
        let call = ToolCall {
            id: ToolCallId::new("call-1").unwrap(),
            tool_id: vesper_domain::ToolId::new("deferred_research").unwrap(),
            arguments: json!({"query": "anything"}),
            extensions: vesper_domain::ExtensionMap::default(),
        };
        let result = registry.execute(&call, &context).await.expect(
            "a deferred tool must still execute successfully when the model calls it by name",
        );
        assert_eq!(
            result.text.as_str(),
            "deferred tool `deferred_research` executed"
        );
    }

    // ------------------------- Gateway routing (Phase 3) -------------------------

    /// Stub gateway executor for the routing tests. Returns a deterministic
    /// result naming the tool it dispatched so the test can distinguish a
    /// gateway hit from an UnknownTool error.
    struct StubGatewayExecutor;

    impl ToolExecutor for StubGatewayExecutor {
        fn definition(&self) -> ToolDefinition {
            schema_definition(
                "stub_gateway",
                "Stub gateway executor for tests.",
                ToolExecutionClass::NestedWorkflow,
                &[],
            )
        }

        fn execute<'a>(
            &'a self,
            call: &'a ToolCall,
            _context: &'a ToolContext,
        ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
            let name = call.tool_id.as_str().to_owned();
            Box::pin(async move { ToolResult::new(format!("gateway executed `{name}`")) })
        }
    }

    #[test]
    fn gateway_prefixes_are_queryable_via_has_gateway() {
        let registry =
            ToolRegistry::parity_default().with_gateway("stub__", Arc::new(StubGatewayExecutor));
        assert!(registry.has_gateway("stub__"));
        assert!(!registry.has_gateway("mcp__"));
        assert!(!registry.has_gateway(""));
    }

    #[test]
    fn gateway_prefix_is_considered_registered_for_contains_lookup() {
        let registry =
            ToolRegistry::parity_default().with_gateway("stub__", Arc::new(StubGatewayExecutor));
        // Names matching a gateway prefix route via the gateway executor.
        assert!(registry.contains("stub__anything_here"));
        // Names with no matching prefix fall through to UnknownTool.
        assert!(!registry.contains("totally_unknown_name"));
        // An empty gateway prefix is a no-op and must not match everything.
        let bad = ToolRegistry::parity_default().with_gateway("", Arc::new(StubGatewayExecutor));
        assert!(!bad.contains("totally_unknown_name"));
    }

    #[tokio::test]
    async fn gateway_routes_unregistered_calls_matching_a_prefix() {
        let registry =
            ToolRegistry::parity_default().with_gateway("stub__", Arc::new(StubGatewayExecutor));
        let context = uncancellable_context(
            Vec::new(),
            SessionOperatingMode::Code,
            SessionPermissionMode::Bypass,
        );
        let call = ToolCall {
            id: ToolCallId::new("call-1").unwrap(),
            tool_id: vesper_domain::ToolId::new("stub__runtime_tool").unwrap(),
            arguments: json!({}),
            extensions: vesper_domain::ExtensionMap::default(),
        };
        let result = registry
            .execute(&call, &context)
            .await
            .expect("a gateway-prefixed name must route to its executor, not UnknownTool");
        assert_eq!(
            result.text.as_str(),
            "gateway executed `stub__runtime_tool`"
        );
    }

    #[tokio::test]
    async fn longest_gateway_prefix_wins_when_two_prefixes_match() {
        struct BroadGateway;
        impl ToolExecutor for BroadGateway {
            fn definition(&self) -> ToolDefinition {
                schema_definition("broad", "broad", ToolExecutionClass::NestedWorkflow, &[])
            }
            fn execute<'a>(
                &'a self,
                _: &'a ToolCall,
                _: &'a ToolContext,
            ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
                Box::pin(async move { ToolResult::new("broad gateway hit") })
            }
        }
        struct NarrowGateway;
        impl ToolExecutor for NarrowGateway {
            fn definition(&self) -> ToolDefinition {
                schema_definition("narrow", "narrow", ToolExecutionClass::NestedWorkflow, &[])
            }
            fn execute<'a>(
                &'a self,
                _: &'a ToolCall,
                _: &'a ToolContext,
            ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
                Box::pin(async move { ToolResult::new("narrow gateway hit") })
            }
        }
        let registry = ToolRegistry::parity_default()
            .with_gateway("stub__", Arc::new(BroadGateway))
            .with_gateway("stub__narrow__", Arc::new(NarrowGateway));
        let context = uncancellable_context(
            Vec::new(),
            SessionOperatingMode::Code,
            SessionPermissionMode::Bypass,
        );
        let call = ToolCall {
            id: ToolCallId::new("call-1").unwrap(),
            tool_id: vesper_domain::ToolId::new("stub__narrow__specific").unwrap(),
            arguments: json!({}),
            extensions: vesper_domain::ExtensionMap::default(),
        };
        let result = registry.execute(&call, &context).await.unwrap();
        assert_eq!(
            result.text.as_str(),
            "narrow gateway hit",
            "longest-prefix gateway must win when two prefixes match"
        );
    }
}
