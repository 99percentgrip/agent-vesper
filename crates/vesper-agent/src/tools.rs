//! Parity-critical stub executors (ADR 0010, Tier C Phase 1).
//!
//! One stub per oracle tool in the parity subset (`glm_acp/tools.py:205-404`).
//! Each advertises its [`ToolDefinition`] (name, JSON schema, and
//! [`ToolExecutionClass`]) and returns a canned [`ToolResult`] — no real
//! filesystem, shell, or network I/O. Real implementations replace the bodies
//! in Phase 4 behind the same [`ToolExecutor`] contract, with path confinement
//! via `vesper-security`.

use vesper_domain::{SessionOperatingMode, ToolCall, ToolExecutionClass};
use vesper_provider::CancellationSignal;
use vesper_runtime::RuntimeCancellation;

use crate::executor::{
    ToolContext, ToolError, ToolExecutor, ToolFuture, ToolResult, schema_definition,
};

/// Reads a text file (optionally between line ranges).
pub struct ReadFile;
/// Lists files and directories at a path.
pub struct ListDirectory;
/// Glob file search.
pub struct SearchFiles;
/// Regex content search.
pub struct Grep;
/// Creates or overwrites a file.
pub struct WriteFile;
/// Exact-block text replacement.
pub struct EditFile;
/// Atomic unified-diff application.
pub struct ApplyPatch;
/// Confined shell command execution.
pub struct RunCommand;
/// Plan-task list update (the `.agent/plan.md` planner).
pub struct UpdatePlan;

impl ToolExecutor for ReadFile {
    fn definition(&self) -> vesper_domain::ToolDefinition {
        schema_definition(
            "read_file",
            "Read the contents of a text file. Use absolute or relative paths.",
            ToolExecutionClass::ReadOnly,
            &[
                ("path", "string", true),
                ("start_line", "integer", false),
                ("end_line", "integer", false),
            ],
        )
    }
    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
        _context: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        let args = summarize_args(call);
        Box::pin(async move { ToolResult::new(format!("[stub read_file] would read {args}")) })
    }
}

impl ToolExecutor for ListDirectory {
    fn definition(&self) -> vesper_domain::ToolDefinition {
        schema_definition(
            "list_directory",
            "List files and directories at the given path.",
            ToolExecutionClass::ReadOnly,
            &[("path", "string", false)],
        )
    }
    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
        _context: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        let args = summarize_args(call);
        Box::pin(async move { ToolResult::new(format!("[stub list_directory] would list {args}")) })
    }
}

impl ToolExecutor for SearchFiles {
    fn definition(&self) -> vesper_domain::ToolDefinition {
        schema_definition(
            "search_files",
            "Search for files by glob pattern (e.g. **/*.rs). Returns matching paths.",
            ToolExecutionClass::ReadOnly,
            &[("pattern", "string", true), ("path", "string", false)],
        )
    }
    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
        _context: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        let args = summarize_args(call);
        Box::pin(async move { ToolResult::new(format!("[stub search_files] would match {args}")) })
    }
}

impl ToolExecutor for Grep {
    fn definition(&self) -> vesper_domain::ToolDefinition {
        schema_definition(
            "grep",
            "Search file contents using a regular expression.",
            ToolExecutionClass::ReadOnly,
            &[
                ("pattern", "string", true),
                ("path", "string", false),
                ("include", "string", false),
            ],
        )
    }
    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
        _context: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        let args = summarize_args(call);
        Box::pin(async move { ToolResult::new(format!("[stub grep] would search {args}")) })
    }
}

impl ToolExecutor for WriteFile {
    fn definition(&self) -> vesper_domain::ToolDefinition {
        schema_definition(
            "write_file",
            "Write content to a file. Creates the file if it does not exist, overwrites if it does.",
            ToolExecutionClass::Mutating,
            &[("path", "string", true), ("content", "string", true)],
        )
    }
    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
        _context: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        let args = summarize_args(call);
        Box::pin(async move { ToolResult::new(format!("[stub write_file] would write {args}")) })
    }
}

impl ToolExecutor for EditFile {
    fn definition(&self) -> vesper_domain::ToolDefinition {
        schema_definition(
            "edit_file",
            "Replace a specific block of text in a file. Both old_text and new_text must be exact.",
            ToolExecutionClass::Mutating,
            &[
                ("path", "string", true),
                ("old_text", "string", true),
                ("new_text", "string", true),
            ],
        )
    }
    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
        _context: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        let args = summarize_args(call);
        Box::pin(async move { ToolResult::new(format!("[stub edit_file] would edit {args}")) })
    }
}

impl ToolExecutor for ApplyPatch {
    fn definition(&self) -> vesper_domain::ToolDefinition {
        schema_definition(
            "apply_patch",
            "Apply a validated unified diff to one text file atomically.",
            ToolExecutionClass::Mutating,
            &[("path", "string", true), ("patch", "string", true)],
        )
    }
    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
        _context: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        let args = summarize_args(call);
        Box::pin(async move { ToolResult::new(format!("[stub apply_patch] would apply {args}")) })
    }
}

impl ToolExecutor for RunCommand {
    fn definition(&self) -> vesper_domain::ToolDefinition {
        schema_definition(
            "run_command",
            "Execute a shell command in the working directory.",
            ToolExecutionClass::Shell,
            &[("command", "string", true), ("timeout", "integer", false)],
        )
    }
    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
        _context: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        let args = summarize_args(call);
        Box::pin(async move { ToolResult::new(format!("[stub run_command] would run {args}")) })
    }
}

impl ToolExecutor for UpdatePlan {
    fn definition(&self) -> vesper_domain::ToolDefinition {
        use serde_json::json;
        // `update_plan` carries a nested `tasks` array; build the schema inline.
        let mut definition = schema_definition(
            "update_plan",
            "Update the task plan shown to the user. Call at the start of multi-step tasks.",
            // The plan tracker mutates only `.agent/plan.md`; classified ReadOnly
            // for the FS authority envelope, with a single sanctioned write path
            // enforced by the real executor in Phase 4.
            ToolExecutionClass::ReadOnly,
            &[("tasks", "array", true)],
        );
        definition.input_schema = json!({
            "type": "object",
            "properties": {
                "tasks": {
                    "type": "array",
                    "description": "The complete list of tasks. Replaces the previous plan.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": {"type": "string"},
                            "status": {"type": "string", "enum": ["pending", "in_progress", "completed"]},
                            "priority": {"type": "string", "enum": ["high", "medium", "low"]}
                        },
                        "required": ["content", "status"]
                    }
                }
            },
            "required": ["tasks"]
        });
        definition
    }
    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
        _context: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        let args = summarize_args(call);
        Box::pin(async move { ToolResult::new(format!("[stub update_plan] would record {args}")) })
    }
}

/// Renders a short, safe summary of a call's arguments for stub output.
///
/// Real executors parse `call.arguments` per their schema; stubs only echo a
/// bounded digest so the loop can prove routing without filesystem I/O.
fn summarize_args(call: &ToolCall) -> String {
    let serialized = call.arguments.to_string();
    // Limit by char count (MSRV-safe; never splits a code point) and keep the
    // digest bounded. Stub output never carries credentials — executors
    // receive only model-authored arguments.
    if serialized.chars().count() > 80 {
        let digest: String = serialized.chars().take(80).collect();
        format!("{digest}…")
    } else {
        serialized
    }
}

/// Builds an uncancellable [`ToolContext`] for stub execution and tests.
///
/// Phase 4 replaces this with a context that carries real workspace roots and
/// a runtime-owned cancellation derived from the agent loop.
#[must_use]
pub fn stub_context(
    operating_mode: SessionOperatingMode,
    permission_mode: vesper_domain::SessionPermissionMode,
) -> ToolContext {
    struct NeverCancelled;
    impl CancellationSignal for NeverCancelled {
        fn is_cancelled(&self) -> bool {
            false
        }
    }
    let _ = RuntimeCancellation::new(); // compile-time link to the runtime cancellation surface
    ToolContext {
        workspace_roots: Vec::new(),
        operating_mode,
        permission_mode,
        cancellation: std::sync::Arc::new(NeverCancelled),
    }
}
