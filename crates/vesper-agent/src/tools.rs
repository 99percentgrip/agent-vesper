//! Real tool executors (ADR 0010, Tier C Phase 3/4).
//!
//! Each parity-critical tool performs real filesystem/shell I/O behind strict
//! path confinement (see [`crate::confinement`]). `apply_patch` ships a
//! minimal single-file unified-diff applier; the other eight are full
//! implementations matching the Python oracle (`glm_acp/tools.py:205-404`).

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use vesper_domain::{SessionOperatingMode, SessionPermissionMode, ToolCall, ToolExecutionClass};
use vesper_provider::CancellationSignal;

use crate::confinement::{
    confine, io_failure, optional_string_arg, optional_u64_arg, primary_root, string_arg,
};
use crate::executor::{
    ToolContext, ToolError, ToolExecutor, ToolFuture, ToolResult, schema_definition,
};

/// Maximum bytes of tool output retained (prevents unbounded model context).
const MAX_OUTPUT_BYTES: usize = 65_536;

// ----------------------------- read-only -----------------------------

pub struct ReadFile;
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
        ctx: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        let args = call.arguments.clone();
        Box::pin(async move {
            let root = primary_root(ctx)?;
            let path = confine(root, &string_arg(&args, "path")?)?;
            let content = fs::read_to_string(&path).map_err(|e| io_failure("read_file", e))?;
            let start = optional_u64_arg(&args, "start_line").map(|v| v as usize);
            let end = optional_u64_arg(&args, "end_line").map(|v| v as usize);
            let selected = select_lines(&content, start, end);
            ToolResult::new(bounded(&selected))
        })
    }
}

pub struct ListDirectory;
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
        ctx: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        let args = call.arguments.clone();
        Box::pin(async move {
            let root = primary_root(ctx)?;
            let target = match optional_string_arg(&args, "path") {
                Some(p) => confine(root, &p)?,
                None => root.to_path_buf(),
            };
            let mut entries: Vec<String> = fs::read_dir(&target)
                .map_err(|e| io_failure("list_directory", e))?
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect();
            entries.sort();
            let joined = entries.join("\n");
            ToolResult::new(bounded(&joined))
        })
    }
}

pub struct SearchFiles;
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
        ctx: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        let args = call.arguments.clone();
        Box::pin(async move {
            let root = primary_root(ctx)?;
            let base = match optional_string_arg(&args, "path") {
                Some(p) => confine(root, &p)?,
                None => root.to_path_buf(),
            };
            let pattern = string_arg(&args, "pattern")?;
            let matches = glob_search(&base, &pattern)?;
            ToolResult::new(bounded(&matches.join("\n")))
        })
    }
}

pub struct Grep;
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
        ctx: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        let args = call.arguments.clone();
        Box::pin(async move {
            let root = primary_root(ctx)?;
            let base = match optional_string_arg(&args, "path") {
                Some(p) => confine(root, &p)?,
                None => root.to_path_buf(),
            };
            let pattern = string_arg(&args, "pattern")?;
            let include = optional_string_arg(&args, "include");
            let re = regex::Regex::new(&pattern).map_err(|e| ToolError::InvalidArguments {
                tool: "grep".into(),
                reason: e.to_string(),
            })?;
            let mut hits = Vec::new();
            grep_walk(&base, &base, &re, include.as_deref(), &mut hits)?;
            ToolResult::new(bounded(&hits.join("\n")))
        })
    }
}

// ----------------------------- mutating ------------------------------

pub struct WriteFile;
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
        ctx: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        let args = call.arguments.clone();
        Box::pin(async move {
            let root = primary_root(ctx)?;
            let path = confine(root, &string_arg(&args, "path")?)?;
            let content = string_arg(&args, "content")?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| io_failure("write_file", e))?;
            }
            fs::write(&path, content.as_bytes()).map_err(|e| io_failure("write_file", e))?;
            ToolResult::new(format!(
                "wrote {} bytes to {}",
                content.len(),
                path.display()
            ))
        })
    }
}

pub struct EditFile;
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
        ctx: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        let args = call.arguments.clone();
        Box::pin(async move {
            let root = primary_root(ctx)?;
            let path = confine(root, &string_arg(&args, "path")?)?;
            let old_text = string_arg(&args, "old_text")?;
            let new_text = string_arg(&args, "new_text")?;
            let content = fs::read_to_string(&path).map_err(|e| io_failure("edit_file", e))?;
            let count = content.matches(&old_text).count();
            if count == 0 {
                return Err(ToolError::InvalidArguments {
                    tool: "edit_file".into(),
                    reason: "old_text was not found in the file".into(),
                });
            }
            if count > 1 {
                return Err(ToolError::InvalidArguments {
                    tool: "edit_file".into(),
                    reason: format!(
                        "old_text matched {count} times; provide more context for a unique match"
                    ),
                });
            }
            let updated = content.replacen(&old_text, &new_text, 1);
            fs::write(&path, updated.as_bytes()).map_err(|e| io_failure("edit_file", e))?;
            ToolResult::new(format!("edited {}", path.display()))
        })
    }
}

pub struct ApplyPatch;
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
        ctx: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        let args = call.arguments.clone();
        Box::pin(async move {
            let root = primary_root(ctx)?;
            let path = confine(root, &string_arg(&args, "path")?)?;
            let patch = string_arg(&args, "patch")?;
            let original = fs::read_to_string(&path).map_err(|e| io_failure("apply_patch", e))?;
            let updated = apply_unified_diff(&original, &patch)?;
            // Atomic write: write alongside then rename.
            let staging = path.with_extension("vesper-patch-tmp");
            fs::write(&staging, updated.as_bytes()).map_err(|e| io_failure("apply_patch", e))?;
            fs::rename(&staging, &path).map_err(|e| io_failure("apply_patch", e))?;
            ToolResult::new(format!("patched {}", path.display()))
        })
    }
}

// ------------------------------ shell --------------------------------

pub struct RunCommand;
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
        ctx: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        let args = call.arguments.clone();
        let cwd = primary_root(ctx).map(|p| p.to_path_buf());
        let cancelled = ctx.cancellation.clone();
        Box::pin(async move {
            let cwd = cwd?;
            let command = string_arg(&args, "command")?;
            let timeout = optional_u64_arg(&args, "timeout").unwrap_or(120);
            let command_for_task = command.clone();
            let cwd_for_task = cwd.clone();
            let cancelled_for_task = cancelled.clone();
            let output = tokio::task::spawn_blocking(move || {
                run_bounded(
                    &command_for_task,
                    &cwd_for_task,
                    timeout,
                    &cancelled_for_task,
                )
            })
            .await
            .map_err(|e| ToolError::Failed(format!("command task failed: {e}")))??;
            ToolResult::new(bounded(&output))
        })
    }
}

// ------------------------------ planner ------------------------------

pub struct UpdatePlan;
impl ToolExecutor for UpdatePlan {
    fn definition(&self) -> vesper_domain::ToolDefinition {
        // `update_plan` mutates only `.agent/plan.md`; classified ReadOnly for
        // the FS authority envelope (Phase 5 wires its result into the TUI
        // REVIEW transition). The nested `tasks` schema matches the oracle.
        let mut definition = schema_definition(
            "update_plan",
            "Update the task plan shown to the user. Call at the start of multi-step tasks.",
            ToolExecutionClass::ReadOnly,
            &[("tasks", "array", true)],
        );
        definition.input_schema = serde_json::json!({
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
        ctx: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        let args = call.arguments.clone();
        Box::pin(async move {
            let root = primary_root(ctx)?;
            let markdown = render_plan_markdown(&args);
            // Write the plan artifact inside the confined `.agent/` directory,
            // matching the oracle's `.agent/plan.md`.
            let plan_rel = std::path::Path::new(".agent").join("plan.md");
            let plan_path = confine(root, &plan_rel.to_string_lossy())?;
            if let Some(parent) = plan_path.parent() {
                fs::create_dir_all(parent).map_err(|e| io_failure("update_plan", e))?;
            }
            let mut file =
                fs::File::create(&plan_path).map_err(|e| io_failure("update_plan", e))?;
            file.write_all(markdown.as_bytes())
                .map_err(|e| io_failure("update_plan", e))?;
            // Return the rendered plan so the agent loop can surface it to the
            // TUI REVIEW transition (Phase 5).
            ToolResult::new(bounded(&markdown))
        })
    }
}

/// Builds an uncancellable [`ToolContext`] (tests/stubs without a runtime-owned
/// cancellation). Production contexts come from the agent loop.
#[must_use]
pub fn stub_context(
    roots: Vec<vesper_domain::WorkspaceRoot>,
    operating_mode: SessionOperatingMode,
    permission_mode: SessionPermissionMode,
) -> ToolContext {
    struct NeverCancelled;
    impl CancellationSignal for NeverCancelled {
        fn is_cancelled(&self) -> bool {
            false
        }
    }
    ToolContext {
        workspace_roots: roots,
        operating_mode,
        permission_mode,
        conversation: Vec::new(),
        cancellation: std::sync::Arc::new(NeverCancelled),
    }
}

// ----------------------------- helpers -------------------------------

/// Bounds an output string to `MAX_OUTPUT_BYTES` on a UTF-8 boundary.
fn bounded(value: &str) -> String {
    if value.len() <= MAX_OUTPUT_BYTES {
        return value.to_string();
    }
    let mut end = MAX_OUTPUT_BYTES;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [truncated]", &value[..end])
}

/// Selects an inclusive 1-based line range from a file's contents.
fn select_lines(content: &str, start: Option<usize>, end: Option<usize>) -> String {
    let (Some(start), Some(end)) = (start, end) else {
        return content.to_string();
    };
    if start == 0 || end < start {
        return content.to_string();
    }
    content
        .lines()
        .skip(start - 1)
        .take(end.saturating_sub(start - 1))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Recursive glob search confined to `base`. Handles `**/*.<ext>` and
/// `*.<ext>` patterns via the `glob` crate (symlinks not followed).
fn glob_search(base: &Path, pattern: &str) -> Result<Vec<String>, ToolError> {
    let full = base.join(pattern);
    let pattern_str = full.to_string_lossy().to_string();
    let options = glob::MatchOptions {
        case_sensitive: true,
        require_literal_separator: true,
        require_literal_leading_dot: false,
    };
    let mut matches = Vec::new();
    for entry in glob::glob_with(&pattern_str, options)
        .map_err(|e| ToolError::InvalidArguments {
            tool: "search_files".into(),
            reason: e.to_string(),
        })?
        .flatten()
    {
        if let Ok(rel) = entry.strip_prefix(base) {
            matches.push(rel.to_string_lossy().into_owned());
        } else {
            matches.push(entry.to_string_lossy().into_owned());
        }
        if matches.len() >= 500 {
            break;
        }
    }
    Ok(matches)
}

/// Recursive content search writing `path:line: text` hits.
fn grep_walk(
    base: &Path,
    current: &Path,
    re: &regex::Regex,
    include: Option<&str>,
    hits: &mut Vec<String>,
) -> Result<(), ToolError> {
    if hits.len() >= 500 {
        return Ok(());
    }
    let entries = match fs::read_dir(current) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        if hits.len() >= 500 {
            return Ok(());
        }
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if file_type.is_dir() {
            // Skip VCS noise; recursion stays confined under `base`.
            if path.file_name().map(|n| n == ".git").unwrap_or(false) {
                continue;
            }
            grep_walk(base, &path, re, include, hits)?;
        } else if file_type.is_file() {
            if let Some(glob) = include {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if !glob_match_name(glob, &name) {
                    continue;
                }
            }
            if let Ok(content) = fs::read_to_string(&path) {
                let rel = path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned();
                for (index, line) in content.lines().enumerate() {
                    if re.is_match(line) {
                        hits.push(format!("{}:{}: {}", rel, index + 1, line));
                        if hits.len() >= 500 {
                            return Ok(());
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Simple single-segment glob for the `include` filter (`*.rs`).
fn glob_match_name(pattern: &str, name: &str) -> bool {
    if pattern == name {
        return true;
    }
    let (prefix, suffix) = match pattern.split_once('*') {
        Some((pre, suf)) => (pre, suf),
        None => return pattern == name,
    };
    name.starts_with(prefix) && name.ends_with(suffix) && name.len() >= prefix.len() + suffix.len()
}

/// Renders the plan tasks array as the `.agent/plan.md` markdown body.
fn render_plan_markdown(args: &serde_json::Value) -> String {
    let mut buffer = String::from("# Plan\n\n");
    let Some(tasks) = args.get("tasks").and_then(|v| v.as_array()) else {
        return buffer + "_(no tasks)_\n";
    };
    for (index, task) in tasks.iter().enumerate() {
        let content = task
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("(no content)");
        let status = task
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("pending");
        let priority = task
            .get("priority")
            .and_then(|v| v.as_str())
            .unwrap_or("medium");
        let marker = match status {
            "completed" => "[x]",
            "in_progress" => "[~]",
            _ => "[ ]",
        };
        buffer.push_str(&format!(
            "{marker} #{} ({}/{}) {content}\n",
            index + 1,
            status,
            priority
        ));
    }
    buffer
}

/// Applies a minimal single-file unified diff by reconstructing the `before`
/// (context + removed) and `after` (context + added) blocks and doing one
/// exact replace in the original. Errors when the context is absent or
/// ambiguous so callers can retry with more context.
pub fn apply_unified_diff(original: &str, patch: &str) -> Result<String, ToolError> {
    let mut before = String::new();
    let mut after = String::new();
    let mut saw_hunk = false;
    for line in patch.lines() {
        if line.starts_with("@@") || line.starts_with("---") || line.starts_with("+++") {
            saw_hunk = true;
            continue;
        }
        if line.is_empty() {
            continue;
        }
        saw_hunk = true;
        if let Some(content) = line.strip_prefix(' ') {
            // Context line: present in both the before and after states.
            before.push_str(content);
            before.push('\n');
            after.push_str(content);
            after.push('\n');
        } else if let Some(added) = line.strip_prefix('+') {
            after.push_str(added);
            after.push('\n');
        } else if let Some(removed) = line.strip_prefix('-') {
            before.push_str(removed);
            before.push('\n');
        } else {
            return Err(ToolError::InvalidArguments {
                tool: "apply_patch".into(),
                reason: format!("unsupported diff line: {line:?}"),
            });
        }
    }
    if !saw_hunk {
        return Ok(original.to_string());
    }
    if before.is_empty() {
        // Pure insertion: append the new block to the original.
        let mut result = original.to_string();
        result.push_str(&after);
        return Ok(result);
    }
    let matches = original.matches(&before).count();
    if matches == 0 {
        return Err(ToolError::InvalidArguments {
            tool: "apply_patch".into(),
            reason: "patch context was not found in the file".into(),
        });
    }
    if matches > 1 {
        return Err(ToolError::InvalidArguments {
            tool: "apply_patch".into(),
            reason: "patch context matched multiple times; add more context".into(),
        });
    }
    Ok(original.replacen(&before, &after, 1))
}

/// Runs `command` via the platform shell in `cwd`, bounded by `timeout_secs`.
/// Observes `cancellation` between polls. Runs on a blocking thread.
///
/// On timeout the shell leader is killed and reaped, but the pipes are *not*
/// read (a killed `sh -c "…"` may leave a grandchild holding the pipe open;
/// reading would block until it exits). Grandchildren may briefly outlive the
/// kill — the tradeoff of `#![forbid(unsafe_code)]` (no safe `killpg`).
fn run_bounded(
    command: &str,
    cwd: &Path,
    timeout_secs: u64,
    cancellation: &std::sync::Arc<dyn CancellationSignal>,
) -> Result<String, ToolError> {
    let (program, flag) = if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };
    let mut child = Command::new(program)
        .arg(flag)
        .arg(command)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ToolError::Failed(format!("spawn failed: {e}")))?;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if cancellation.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ToolError::Failed("command cancelled".into()));
        }
        match child.try_wait() {
            Ok(Some(_)) => break, // shell exited; pipes are closed and safe to read
            Ok(None) => {
                if Instant::now() >= deadline {
                    // Kill + reap the leader without reading the pipes so a
                    // lingering grandchild cannot block us.
                    let _ = child.kill();
                    let _ = child.wait();
                    return Ok(String::from("[command timed out and was killed]"));
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ToolError::Failed(format!("wait failed: {error}")));
            }
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|e| ToolError::Failed(format!("read output failed: {e}")))?;
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.stderr.is_empty() {
        combined.push_str("\n[stderr]\n");
        combined.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    Ok(bounded(&combined))
}
