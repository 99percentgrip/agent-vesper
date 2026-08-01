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
struct Entry {
    definition: ToolDefinition,
    executor: Arc<dyn ToolExecutor>,
}

/// The parity-critical tool registry.
pub struct ToolRegistry {
    entries: BTreeMap<String, Entry>,
}

impl ToolRegistry {
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
        Self { entries }
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
    #[must_use]
    pub fn definitions_for(&self, mode: SessionOperatingMode) -> Vec<ToolDefinition> {
        self.entries
            .values()
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

    /// Whether a tool named `name` is registered.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    /// Routes `call` to its executor and runs it under `context`.
    ///
    /// Unknown tool names surface as [`ToolError::UnknownTool`]; the agent loop
    /// feeds that back to the model as the tool result so the turn recovers.
    pub fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
        context: &'a crate::executor::ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        let executor = match self.entries.get(call.tool_id.as_str()) {
            Some(entry) => Arc::clone(&entry.executor),
            None => {
                let name = call.tool_id.as_str().to_string();
                return Box::pin(async move { Err(ToolError::UnknownTool(name)) });
            }
        };
        Box::pin(async move { executor.execute(call, context).await })
    }
}

#[cfg(test)]
mod tests {
    //! Registration, mode filtering, and dispatch routing.

    use super::*;
    use crate::executor::{ToolContext, uncancellable_context};
    use serde_json::json;
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
}
