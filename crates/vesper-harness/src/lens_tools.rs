//! Shared native browser feedback tools. Hosts own URL presentation; the same
//! validated tool result feeds the existing AgentLoop in both hosts.
use std::sync::Arc;

pub struct LensToolService {
    inner: Arc<dyn vesper_agent::ToolService>,
    lens_review: Option<Arc<dyn vesper_agent::vro::LensReviewPort>>,
    on_url: Arc<dyn Fn(&str) + Send + Sync>,
    max_questions: usize,
    policy: String,
}
impl LensToolService {
    pub fn new(
        inner: Arc<dyn vesper_agent::ToolService>,
        lens: Arc<dyn vesper_agent::vro::LensReviewPort>,
        on_url: Arc<dyn Fn(&str) + Send + Sync>,
        max_questions: usize,
        policy: String,
    ) -> Self {
        Self {
            inner,
            lens_review: Some(lens),
            on_url,
            max_questions: max_questions.clamp(1, 12),
            policy,
        }
    }
}
impl Default for NativeLensPort {
    fn default() -> Self {
        Self::new()
    }
}
#[derive(Debug, Clone)]
pub struct NativeLensPort {
    lens: Arc<vesper_agent::planning::VesperLens>,
}

impl NativeLensPort {
    pub fn new() -> Self {
        Self {
            lens: Arc::new(vesper_agent::planning::VesperLens::new()),
        }
    }
}

impl vesper_agent::vro::LensReviewPort for NativeLensPort {
    fn review<'a>(
        &'a self,
        html: &str,
        on_url: &'a (dyn Fn(&str) + Send + Sync),
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        vesper_agent::planning::LensFeedback,
                        vesper_agent::planning::LensError,
                    >,
                > + Send
                + 'a,
        >,
    > {
        let lens = Arc::clone(&self.lens);
        let html = html.to_string();
        Box::pin(async move {
            // Bridge: LensReviewPort takes &dyn Fn (callable multiple times),
            // VesperLens::review_artifact takes impl FnOnce. A &dyn Fn
            // satisfies FnOnce (calling once is a subset of calling many
            // times), so this closure adapts without issues.
            lens.review_artifact(&html, |url| on_url(url)).await
        })
    }

    fn review_file<'a>(
        &'a self,
        file: &'a std::path::Path,
        workspace_root: &'a std::path::Path,
        on_url: &'a (dyn Fn(&str) + Send + Sync),
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        vesper_agent::planning::LensFeedback,
                        vesper_agent::planning::LensError,
                    >,
                > + Send
                + 'a,
        >,
    > {
        let lens = Arc::clone(&self.lens);
        let file = file.to_path_buf();
        let workspace_root = workspace_root.to_path_buf();
        Box::pin(async move {
            lens.review_file(&file, &workspace_root, |url| on_url(url))
                .await
        })
    }
}

pub fn request_human_review_definition() -> vesper_domain::ToolDefinition {
    vesper_domain::ToolDefinition {
        id: vesper_domain::ToolId::new("request_human_review").expect("bounded tool id"),
        harness_name: vesper_domain::HarnessToolName::new("request_human_review")
            .expect("bounded harness name"),
        provider_name: None,
        description: "Request human review of a workspace-confined HTML artifact via VesperLens. Opens trusted review chrome around a sandboxed page and BLOCKS until the human submits feedback (approve/reject/modify). Use only when the user requested visual review or visual/interaction choices materially need inspection; do not use for ordinary source code or fully specified HTML that deterministic checks can verify.".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the HTML file to review."
                }
            },
            "required": ["file_path"]
        }),
        execution_class: vesper_domain::ToolExecutionClass::ReadOnly,
        extensions: vesper_domain::ExtensionMap::default(),
        defer_loading: false,
    }
}

/// Browser-native structured planning interview. Unlike artifact review,
/// this tool owns the HTML surface and returns stable question/value pairs.
pub fn request_human_input_definition(
    max_questions: usize,
    policy: &str,
) -> vesper_domain::ToolDefinition {
    vesper_domain::ToolDefinition {
        id: vesper_domain::ToolId::new("request_human_input").expect("bounded tool id"),
        harness_name: vesper_domain::HarnessToolName::new("request_human_input")
            .expect("bounded harness name"),
        provider_name: None,
        description: format!(
            "Open a VesperLens browser interview and BLOCK until the human answers planning questions. Use this before finalizing a plan when requirements, choices, preferences, scope, or tradeoffs are unresolved. {policy} Options are optional and produce free text when omitted."
        ),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Short interview title."
                },
                "questions": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": max_questions,
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string", "description": "Stable short answer key." },
                            "prompt": { "type": "string", "description": "Concrete question shown to the human." },
                            "description": { "type": "string", "description": "Optional help text explaining why the decision matters." },
                            "options": {
                                "type": "array",
                                "maxItems": 6,
                                "items": { "type": "string" }
                            },
                            "allow_multiple": { "type": "boolean", "default": false },
                            "required": { "type": "boolean", "default": true },
                            "recommended": { "type": "string", "description": "Optional recommended answer displayed to the human." },
                            "allow_other": { "type": "boolean", "default": false }
                        },
                        "required": ["id", "prompt"]
                    }
                }
            },
            "required": ["questions"]
        }),
        execution_class: vesper_domain::ToolExecutionClass::ReadOnly,
        extensions: vesper_domain::ExtensionMap::default(),
        defer_loading: false,
    }
}

impl vesper_agent::ToolService for LensToolService {
    fn definitions(&self) -> Vec<vesper_domain::ToolDefinition> {
        let mut defs = self.inner.definitions();
        // Advertise browser human-input tools only when their Lens port is
        // configured, so every advertised call has a real executor.
        if self.lens_review.is_some() {
            defs.push(request_human_review_definition());
            defs.push(request_human_input_definition(
                self.max_questions,
                &self.policy,
            ));
        }
        defs
    }

    fn execute<'a>(
        &'a self,
        call: &'a vesper_domain::ToolCall,
        context: &'a vesper_agent::ToolContext,
    ) -> vesper_agent::ToolFuture<'a, Result<vesper_agent::ToolResult, vesper_agent::ToolError>>
    {
        // VRO-11.4 — handle the explicit request_human_review tool locally.
        // All other tools delegate to the inner harness service.
        if call.tool_id.as_str() == "request_human_review" {
            return self.execute_request_human_review(call, context);
        }
        if call.tool_id.as_str() == "request_human_input" {
            return self.execute_request_human_input(call, context);
        }
        self.inner.execute(call, context)
    }
}
impl LensToolService {
    /// Executes the `request_human_review` tool: reads the HTML file, routes
    /// it through VesperLens, and returns the human's feedback as the tool
    /// result. The tool BLOCKS until the human submits (matching the
    /// explicit-invocation model).
    fn execute_request_human_review<'a>(
        &'a self,
        call: &'a vesper_domain::ToolCall,
        context: &'a vesper_agent::ToolContext,
    ) -> vesper_agent::ToolFuture<'a, Result<vesper_agent::ToolResult, vesper_agent::ToolError>>
    {
        let args = call.arguments.clone();
        let lens = match &self.lens_review {
            Some(l) => Arc::clone(l),
            None => {
                return Box::pin(async move {
                    Err(tool_failure(
                        "request_human_review",
                        "no VesperLens review port configured",
                    ))
                });
            }
        };
        let on_url = self.on_url.clone();
        Box::pin(async move {
            let path = args
                .get("file_path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    tool_failure("request_human_review", "missing file_path argument")
                })?;
            let workspace_root = vesper_agent::confinement::primary_root(context)
                .map_err(|error| tool_failure("request_human_review", error))?
                .to_path_buf();
            let confined = vesper_agent::confinement::confine(&workspace_root, path)
                .map_err(|error| tool_failure("request_human_review", error))?;
            // Surface the review URL to the TUI's inline trajectory so the
            // user sees where to open the browser. The URL arrives through
            // the on_url callback once VesperLens binds its listener.
            // Route the content through VesperLens. This BLOCKS until the
            // human submits feedback (or the 30-minute timeout fires).
            let feedback = lens
                .review_file(&confined, &workspace_root, on_url.as_ref())
                .await
                .map_err(|e| tool_failure("request_human_review", e))?;
            // Return the feedback as the tool result. The model sees the
            // verdict (APPROVED/REJECTED/NEEDS MODIFICATION) + notes +
            // annotations and can apply corrections on the next step.
            let msg = vesper_agent::vro::feedback_as_context_message(&feedback);
            vesper_agent::ToolResult::new(msg)
        })
    }

    fn execute_request_human_input<'a>(
        &'a self,
        call: &'a vesper_domain::ToolCall,
        _context: &'a vesper_agent::ToolContext,
    ) -> vesper_agent::ToolFuture<'a, Result<vesper_agent::ToolResult, vesper_agent::ToolError>>
    {
        let args = call.arguments.clone();
        let lens = match &self.lens_review {
            Some(lens) => Arc::clone(lens),
            None => {
                return Box::pin(async move {
                    Err(tool_failure(
                        "request_human_input",
                        "no VesperLens review port configured",
                    ))
                });
            }
        };
        let on_url = self.on_url.clone();
        Box::pin(async move {
            let raw_questions = args
                .get("questions")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| tool_failure("request_human_input", "missing questions array"))?;
            let max_questions = self.max_questions;
            if !(1..=max_questions).contains(&raw_questions.len()) {
                return Err(tool_failure(
                    "request_human_input",
                    format!(
                        "questions must contain between 1 and {max_questions} items under the current `/interview-limit` policy ({})",
                        self.policy
                    ),
                ));
            }
            let mut questions = Vec::with_capacity(raw_questions.len());
            let mut question_ids = std::collections::BTreeSet::new();
            for raw in raw_questions {
                let id = raw
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        tool_failure("request_human_input", "question id must not be empty")
                    })?;
                if !question_ids.insert(id.to_owned()) {
                    return Err(tool_failure(
                        "request_human_input",
                        format!("duplicate question id `{id}`"),
                    ));
                }
                if id.chars().count() > 64 {
                    return Err(tool_failure(
                        "request_human_input",
                        "question id must be at most 64 characters",
                    ));
                }
                let prompt = raw
                    .get("prompt")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        tool_failure("request_human_input", "question prompt must not be empty")
                    })?;
                if prompt.chars().count() > 500 {
                    return Err(tool_failure(
                        "request_human_input",
                        "question prompt must be at most 500 characters",
                    ));
                }
                let options = raw
                    .get("options")
                    .and_then(serde_json::Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(serde_json::Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(str::to_owned)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if options.len() > 6 {
                    return Err(tool_failure(
                        "request_human_input",
                        "each question supports at most 6 options",
                    ));
                }
                if options.iter().any(|option| option.chars().count() > 200) {
                    return Err(tool_failure(
                        "request_human_input",
                        "question options must be at most 200 characters",
                    ));
                }
                let description = raw
                    .get("description")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .unwrap_or_default();
                let recommended = raw
                    .get("recommended")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .unwrap_or_default();
                if description.chars().count() > 1_000 || recommended.chars().count() > 200 {
                    return Err(tool_failure(
                        "request_human_input",
                        "question description or recommendation is too long",
                    ));
                }
                questions.push(vesper_agent::planning::LensQuestion {
                    id: id.to_owned(),
                    prompt: prompt.to_owned(),
                    description: description.to_owned(),
                    options,
                    allow_multiple: raw
                        .get("allow_multiple")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                    required: raw
                        .get("required")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(true),
                    recommended: recommended.to_owned(),
                    allow_other: raw
                        .get("allow_other")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                });
            }
            let title = args
                .get("title")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("Planning interview");
            let html = vesper_agent::planning::render_interview_artifact(title, &questions);
            let feedback = lens
                .review(&html, on_url.as_ref())
                .await
                .map_err(|error| tool_failure("request_human_input", error))?;
            vesper_agent::ToolResult::new(vesper_agent::vro::feedback_as_context_message(&feedback))
        })
    }
}

fn tool_failure(name: &str, error: impl std::fmt::Display) -> vesper_agent::ToolError {
    vesper_agent::ToolError::Failed(format!("{name} failed: {error}"))
}
