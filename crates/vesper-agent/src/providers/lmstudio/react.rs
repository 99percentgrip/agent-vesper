//! LM Studio [`ReactAgent`] adapter (VRO-5.2, PRD §13.1 + §11.6).
//!
//! [`LmStudioReactAgent`] implements the VRO-5.1 ReAct loop's model seam
//! against the configured LM Studio server. It builds the ReAct prompting
//! contract (a system instruction teaching the action/observation format, the
//! user prompt, and the running trajectory as `assistant`/`user` message
//! pairs), sends a `/chat/completions` request via the
//! [`LmStudioTransport`] port, and parses the response into a
//! [`ReactDecision`] (CallTool or Finish).
//!
//! ## Why this lives in `vesper-agent`
//!
//! Same reasoning as [`LmStudioCandidateGenerator`](super::generator::LmStudioCandidateGenerator):
//! [`ReactAgent`](crate::vro::react::ReactAgent) is a `vesper-agent` trait
//! (the orchestrator's ReAct model seam). A provider crate implementing it
//! would invert the dependency direction (provider → agent), which the crate
//! boundary rules forbid.
//!
//! ## Decision parser (directive 3)
//!
//! The parser tries the following precedence, and is infallible (the
//! [`ReactAgent`](crate::vro::react::ReactAgent) trait returns
//! [`ReactDecision`] directly, never `Result`):
//!
//! 1. JSON object with an `action` field containing a non-empty `tool` and
//!    optional `arguments` → [`ReactDecision::CallTool`].
//! 2. JSON object with an `answer` / `final_answer` / `final` field →
//!    [`ReactDecision::Finish`].
//! 3. No JSON object anywhere in the text → [`ReactDecision::Finish`] with the
//!    raw text as the answer (the model wrote prose without using the format;
//!    we terminate the loop with its actual output rather than spin on
//!    unparseable JSON).
//! 4. JSON present but missing required fields, empty tool name, or an
//!    unrecognised shape → a synthesized [`ReactDecision::CallTool`] with the
//!    sentinel name [`MALFORMED_TOOL_NAME`]. The [`ToolInvoker`] returns
//!    `UnknownTool` for it, producing a structured failure observation the
//!    loop feeds back to the model so it can self-correct on the next turn.
//!
//! [`ToolInvoker`]: crate::vro::react::ToolInvoker

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use vesper_domain::ModelCapabilities;

use crate::vro::react::{ReactAgent, ReactDecision, TrajectoryEntry};

use super::client::{ChatMessage, LmStudioTransport, build_chat_request, parse_chat_response};
use super::config::LmStudioConfig;

/// Sentinel tool name the parser emits when the model's output is malformed.
///
/// The [`ToolInvoker`] returns `UnknownTool` for it, producing a structured
/// failure observation the loop feeds back to the model. A real harness tool
/// would never begin with an underscore (the harness tool namespace is
/// snake_case identifiers such as `read_file`), so this name is unambiguous.
///
/// [`ToolInvoker`]: crate::vro::react::ToolInvoker
pub const MALFORMED_TOOL_NAME: &str = "_malformed_action";

/// System instruction that teaches the model the ReAct action/observation
/// format. The model is told to emit EXACTLY ONE JSON object per turn — either
/// a tool call or a final answer — with no surrounding prose.
pub const REACT_SYSTEM_PROMPT: &str = "\
You are a ReAct agent. You answer the user's request by interleaving \
THOUGHT, ACTION, and OBSERVATION steps.

On each turn you MUST emit EXACTLY ONE of these JSON objects, and nothing else:

- To call a tool:
  {\"action\": {\"tool\": \"<tool_name>\", \"arguments\": {<...>}}}
- To finish with a final answer:
  {\"answer\": \"<your final answer>\"}

Rules:
1. Output ONLY the JSON object. No prose, no markdown fences, no commentary \
   before or after.
2. Use a registered tool name. Look at the previous observations to see what \
   tools exist.
3. Read-only tools (read_file, grep, list_directory, search_files) gather \
   evidence. Mutating tools (write_file, edit_file, run_command) change state. \
   If a Read-Before-Write policy is in effect, you MUST gather at least one \
   successful read-only observation before any mutation.
4. When you have enough evidence to answer the user, emit the final answer \
   JSON. Do NOT call more tools after you have the answer.
5. When asked to generate code, UI, or artifacts, you MUST execute the \
   write_file tool within the same turn. Call request_human_review only for \
   workspace-confined HTML when the user requested visual review or unresolved \
   visual/interaction choices materially require human inspection. Never use \
   it for ordinary source code. \
   Do NOT output your plan and yield to the user. Execute the tools \
   immediately. Printing the artifact's content as your final answer without \
   calling write_file is a FAILED turn. The only exception is Plan mode, \
   where update_plan replaces file mutation.
6. When planning depends on unresolved user choices, call \
   request_human_input with only the concrete questions needed and never more \
   than the current tool schema permits. Continue from \
   the returned browser answers; never invent missing requirements.";

/// LM Studio-backed [`ReactAgent`].
///
/// Mirrors [`LmStudioCandidateGenerator`](super::generator::LmStudioCandidateGenerator)
/// in shape: `config` + `model` + probed `capabilities` + a shared
/// `Arc<dyn LmStudioTransport>`. The transport is the composition-boundary
/// seam — no HTTP client crate is imported here.
#[derive(Clone)]
pub struct LmStudioReactAgent {
    config: LmStudioConfig,
    model: String,
    capabilities: ModelCapabilities,
    transport: Arc<dyn LmStudioTransport>,
}

impl LmStudioReactAgent {
    /// Creates a ReAct agent pinned to `model` with probed `capabilities`,
    /// using `transport` to reach the server.
    #[must_use]
    pub fn new(
        config: LmStudioConfig,
        model: impl Into<String>,
        capabilities: ModelCapabilities,
        transport: Arc<dyn LmStudioTransport>,
    ) -> Self {
        Self {
            config,
            model: model.into(),
            capabilities,
            transport,
        }
    }

    /// The model id this agent targets.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// The observed capabilities.
    #[must_use]
    pub fn capabilities(&self) -> ModelCapabilities {
        self.capabilities
    }

    /// Builds the chat message list for a `(prompt, trajectory)` pair.
    ///
    /// Layout (directive 1):
    /// - A `system` message with the ReAct format contract (when the model
    ///   supports system prompts; otherwise the contract is prepended to the
    ///   user prompt as a fallback).
    /// - A `user` message with the original user prompt.
    /// - For each trajectory entry, in order:
    ///   - [`TrajectoryEntry::Action`] → an `assistant` message replaying the
    ///     chosen action in the canonical JSON form the model is supposed to
    ///     emit, so the model sees its own past decisions in the format we
    ///     expect.
    ///   - [`TrajectoryEntry::Observation`] → a `user` message labelled
    ///     `Observation:` (success) or `Error:` (failure), so the model can
    ///     distinguish tool results from tool errors.
    ///
    /// Pure and unit-testable independently of the transport.
    #[must_use]
    pub fn build_messages(
        prompt: &str,
        trajectory: &[TrajectoryEntry],
        supports_system_prompts: bool,
    ) -> Vec<ChatMessage> {
        let mut messages = Vec::with_capacity(2 + trajectory.len() * 2);

        // The contract message. Some local servers do not support the system
        // role; in that case we prepend the contract to the user prompt so
        // the model still sees it.
        let (contract_role, user_prefix) = if supports_system_prompts {
            (
                Some(ChatMessage {
                    role: "system".into(),
                    content: REACT_SYSTEM_PROMPT.into(),
                }),
                String::new(),
            )
        } else {
            (
                None,
                format!("{REACT_SYSTEM_PROMPT}\n\n---\n\nUser request:\n"),
            )
        };
        if let Some(system) = contract_role {
            messages.push(system);
        }

        messages.push(ChatMessage {
            role: "user".into(),
            content: format!("{user_prefix}{prompt}"),
        });

        // Replay the trajectory. Each Action becomes an `assistant` turn
        // echoing the canonical JSON action shape; each Observation becomes a
        // `user` turn labelled Observation/Error. This reconstructs the
        // visible chat history the model expects after a real action/observe
        // round-trip.
        for entry in trajectory {
            match entry {
                TrajectoryEntry::Action { name, arguments } => {
                    let canonical = serde_json::json!({
                        "action": {
                            "tool": name,
                            "arguments": arguments,
                        }
                    });
                    messages.push(ChatMessage {
                        role: "assistant".into(),
                        content: canonical.to_string(),
                    });
                }
                TrajectoryEntry::Observation { text, success } => {
                    let label = if *success { "Observation" } else { "Error" };
                    messages.push(ChatMessage {
                        role: "user".into(),
                        content: format!("{label}: {text}"),
                    });
                }
            }
        }

        messages
    }
}

impl ReactAgent for LmStudioReactAgent {
    fn next_action<'a>(
        &'a self,
        prompt: &'a str,
        trajectory: &'a [TrajectoryEntry],
    ) -> Pin<Box<dyn Future<Output = ReactDecision> + Send + 'a>> {
        let config = &self.config;
        let model = &self.model;
        let transport = &self.transport;
        let supports_system_prompts = self.capabilities.supports_system_prompts;
        Box::pin(async move {
            let messages = Self::build_messages(prompt, trajectory, supports_system_prompts);
            let req = build_chat_request(config, model, &messages);
            // Transport or parse failure → synthesize a malformed-action
            // decision. The loop's ToolInvoker returns UnknownTool for the
            // sentinel name, producing a structured failure observation the
            // loop feeds back to the model so it can retry on the next turn
            // (rather than crashing the loop).
            let raw = match transport.send(&req).await {
                Ok(response) => match parse_chat_response(&response) {
                    Ok((content, _tokens)) => content,
                    Err(err) => return malformed_decision(&format!("parse error: {err}")),
                },
                Err(err) => return malformed_decision(&format!("transport error: {err}")),
            };
            parse_react_decision(&raw)
        })
    }
}

// ---------------------------------------------------------------------------
// Decision parser (directive 3)
// ---------------------------------------------------------------------------

/// Parses the model's raw text output into a [`ReactDecision`].
///
/// Precedence:
/// 1. JSON object with a non-empty `action.tool` (and optional
///    `action.arguments`) → [`ReactDecision::CallTool`].
/// 2. JSON object with an `answer` / `final_answer` / `final` field →
///    [`ReactDecision::Finish`] (string value taken verbatim; non-string
///    values are stringified).
/// 3. No JSON object anywhere in the text → [`ReactDecision::Finish`] with the
///    trimmed raw text as the answer (the model wrote prose without using the
///    format; we terminate the loop with its actual output).
/// 4. JSON present but missing required fields, empty tool name, or an
///    unrecognised shape → [`ReactDecision::CallTool`] with the sentinel
///    [`MALFORMED_TOOL_NAME`], which the [`ToolInvoker`] rejects as
///    `UnknownTool`.
///
/// The function is infallible (never panics) and deterministic — given the
/// same input it produces the same decision.
///
/// [`ToolInvoker`]: crate::vro::react::ToolInvoker
#[must_use]
pub fn parse_react_decision(raw: &str) -> ReactDecision {
    let trimmed = raw.trim();

    // Decide whether the text "looks like JSON" — i.e. its first non-fence
    // character is `{`. We strip a leading ```json / ``` fence before checking
    // so a fenced-and-truncated object is still detected as JSON-like.
    let looks_like_json = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .map(str::trim_start)
        .unwrap_or(trimmed)
        .starts_with('{');

    let Some(json_str) = extract_json_object(trimmed) else {
        if looks_like_json {
            // The text starts with `{` (so it is NOT prose) but no balanced
            // object was found — it's truncated or otherwise broken JSON.
            // Synthesize a malformed-action decision so the loop feeds the
            // parse failure back to the model rather than silently terminating.
            return malformed_decision(&format!("unterminated JSON: {trimmed}"));
        }
        // Genuinely no JSON object anywhere and the text doesn't look like
        // JSON. Treat the prose as the final answer — this is the most
        // graceful exit: the model effectively answered without using the
        // format, so we honor its text rather than spin the loop on
        // unparseable output.
        return ReactDecision::Finish {
            output: serde_json::Value::String(trimmed.to_string()),
        };
    };

    let parsed: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(_) => {
            // Looked like JSON but did not parse. Synthesize a malformed
            // decision so the loop feeds the parse failure back to the model.
            return malformed_decision(&format!("malformed JSON: {trimmed}"));
        }
    };

    // 1. Tool-call schema: {"action": {"tool": "...", "arguments": {...}}}
    if let Some(action) = parsed.get("action").and_then(serde_json::Value::as_object) {
        let name = action.get("tool").and_then(serde_json::Value::as_str);
        let args = action
            .get("arguments")
            .cloned()
            .filter(serde_json::Value::is_object)
            .unwrap_or_else(|| serde_json::Value::Object(Default::default()));
        return match name {
            Some(name) if !name.is_empty() => ReactDecision::CallTool {
                name: name.to_string(),
                arguments: args,
            },
            Some(_) => malformed_decision(&format!("empty tool name: {trimmed}")),
            None => malformed_decision(&format!("action without tool name: {trimmed}")),
        };
    }

    // 2. Final-answer schema. Try `answer`, then `final_answer`, then `final`.
    for key in ["answer", "final_answer", "final"] {
        if let Some(val) = parsed.get(key) {
            let answer = match val {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            return ReactDecision::Finish {
                output: serde_json::Value::String(answer),
            };
        }
    }

    // 3. JSON parsed but no recognized shape. Malformed.
    malformed_decision(&format!("unrecognized JSON shape: {trimmed}"))
}

/// Extracts the first balanced JSON object from a possibly-noisy string.
///
/// Handles common model-output quirks:
/// - Markdown fenced blocks (` ```json ... ``` ` or bare ` ``` ... ``` `).
/// - Leading/trailing commentary around the JSON.
/// - Strings containing `{` or `}` (proper escape handling so brace depth
///   inside a string is not miscounted).
///
/// Returns `None` if no balanced `{...}` object is found.
fn extract_json_object(s: &str) -> Option<String> {
    let mut s = s.trim();

    // Strip fenced code-block wrapper if present. We strip both the opening
    // fence (optionally with a language tag like `json`) and the closing fence.
    if let Some(rest) = s.strip_prefix("```json").or_else(|| s.strip_prefix("```")) {
        s = rest.trim_start();
        if let Some(inner) = s.strip_suffix("```") {
            s = inner.trim_end();
        }
    }

    // Find the first `{` and scan for its matching `}`, accounting for
    // strings and escapes so braces inside strings do not change depth.
    let start = s.find('{')?;
    let bytes = s.as_bytes();
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escape = false;
    for (i, &byte) in bytes.iter().enumerate().skip(start) {
        let ch = char::from(byte);
        if in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(s[start..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// Synthesizes a [`ReactDecision::CallTool`] with the sentinel
/// [`MALFORMED_TOOL_NAME`]. The `reason` is preserved in the arguments so the
/// `UnknownTool` observation the [`ToolInvoker`] produces carries enough
/// context for the model to self-correct on the next turn.
///
/// [`ToolInvoker`]: crate::vro::react::ToolInvoker
fn malformed_decision(reason: &str) -> ReactDecision {
    ReactDecision::CallTool {
        name: MALFORMED_TOOL_NAME.to_string(),
        arguments: serde_json::json!({ "reason": reason }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::lmstudio::client::{
        HttpMethod, LmStudioError, LmStudioHttpRequest, LmStudioHttpResponse,
    };
    use crate::vro::react::TrajectoryEntry;
    use std::sync::Mutex;

    // -----------------------------------------------------------------------
    // Decision parser tests (directive 4)
    // -----------------------------------------------------------------------

    #[test]
    fn parser_returns_call_tool_for_valid_action_json() {
        // Directive 4: prove that a valid tool-call JSON parses into
        // ReactDecision::CallTool with the right name + arguments.
        let raw = r#"{"action": {"tool": "read_file", "arguments": {"path": "src/main.rs"}}}"#;
        let decision = parse_react_decision(raw);
        match decision {
            ReactDecision::CallTool { name, arguments } => {
                assert_eq!(name, "read_file");
                assert_eq!(arguments["path"], "src/main.rs");
            }
            other => panic!("expected CallTool, got {other:?}"),
        }
    }

    #[test]
    fn parser_returns_call_tool_for_action_without_arguments() {
        // The `arguments` field is optional; missing it defaults to {}.
        let raw = r#"{"action": {"tool": "list_directory"}}"#;
        let decision = parse_react_decision(raw);
        match decision {
            ReactDecision::CallTool { name, arguments } => {
                assert_eq!(name, "list_directory");
                assert!(arguments.is_object());
                assert!(arguments.as_object().unwrap().is_empty());
            }
            other => panic!("expected CallTool, got {other:?}"),
        }
    }

    #[test]
    fn parser_returns_call_tool_for_non_object_arguments() {
        // If arguments is present but not an object, fall back to {} rather
        // than crashing (the loop's executor would reject any non-object args
        // anyway, but we want to never panic).
        let raw = r#"{"action": {"tool": "read_file", "arguments": "not-an-object"}}"#;
        let decision = parse_react_decision(raw);
        match decision {
            ReactDecision::CallTool { name, arguments } => {
                assert_eq!(name, "read_file");
                assert!(
                    arguments.is_object() && arguments.as_object().unwrap().is_empty(),
                    "non-object arguments should default to empty object: {arguments}"
                );
            }
            other => panic!("expected CallTool, got {other:?}"),
        }
    }

    #[test]
    fn parser_returns_finish_for_answer_json() {
        // Directive 4: prove that a final answer parses into Finish.
        let raw = r#"{"answer": "main.rs is the program entry point"}"#;
        let decision = parse_react_decision(raw);
        match decision {
            ReactDecision::Finish { output } => {
                assert_eq!(
                    output,
                    serde_json::Value::String("main.rs is the program entry point".into())
                );
            }
            other => panic!("expected Finish, got {other:?}"),
        }
    }

    #[test]
    fn parser_returns_finish_for_final_answer_aliases() {
        // `final_answer` and `final` are accepted as answer aliases (some
        // models prefer different field names).
        for key in ["final_answer", "final"] {
            let raw = format!(r#"{{"{key}": "done"}}"#);
            let decision = parse_react_decision(&raw);
            assert!(
                matches!(
                    decision,
                    ReactDecision::Finish { ref output }
                        if output == &serde_json::Value::String("done".into())
                ),
                "{key} should map to Finish: {decision:?}"
            );
        }
    }

    #[test]
    fn parser_returns_finish_for_non_string_answer_value() {
        // A non-string answer value is stringified (e.g. a number or array),
        // not rejected.
        let raw = r#"{"answer": [1, 2, 3]}"#;
        let decision = parse_react_decision(raw);
        match decision {
            ReactDecision::Finish { output } => {
                assert_eq!(output, serde_json::Value::String("[1,2,3]".into()));
            }
            other => panic!("expected Finish, got {other:?}"),
        }
    }

    #[test]
    fn parser_strips_markdown_fences_around_action_json() {
        // Models commonly wrap JSON in ```json fences despite the prompt.
        let raw = "```json\n{\"action\": {\"tool\": \"grep\", \"arguments\": {\"pattern\": \"fn main\"}}}\n```";
        let decision = parse_react_decision(raw);
        match decision {
            ReactDecision::CallTool { name, arguments } => {
                assert_eq!(name, "grep");
                assert_eq!(arguments["pattern"], "fn main");
            }
            other => panic!("expected CallTool, got {other:?}"),
        }
    }

    #[test]
    fn parser_strips_leading_commentary_around_json() {
        // The model may prepend "Here is the action:" before the JSON.
        let raw = "I'll read the file first.\n\n{\"action\": {\"tool\": \"read_file\", \"arguments\": {\"path\": \"a\"}}}";
        let decision = parse_react_decision(raw);
        match decision {
            ReactDecision::CallTool { name, arguments } => {
                assert_eq!(name, "read_file");
                assert_eq!(arguments["path"], "a");
            }
            other => panic!("expected CallTool, got {other:?}"),
        }
    }

    #[test]
    fn parser_returns_finish_for_pure_prose_without_json() {
        // Directive 4 (case 3): no JSON object in the text → Finish with the
        // raw prose as the answer. This is the graceful exit when the model
        // ignores the format entirely.
        let raw = "I don't know how to answer that question.";
        let decision = parse_react_decision(raw);
        match decision {
            ReactDecision::Finish { output } => {
                assert_eq!(
                    output,
                    serde_json::Value::String("I don't know how to answer that question.".into())
                );
            }
            other => panic!("expected Finish, got {other:?}"),
        }
    }

    #[test]
    fn parser_returns_finish_for_empty_output() {
        // Empty / whitespace-only output is treated as a (degenerate) final
        // answer rather than crashing.
        let decision = parse_react_decision("   \n  ");
        match decision {
            ReactDecision::Finish { output } => {
                assert_eq!(output, serde_json::Value::String("".into()));
            }
            other => panic!("expected Finish, got {other:?}"),
        }
    }

    #[test]
    fn parser_synthesizes_malformed_decision_for_unbalanced_json() {
        // Directive 4 (case 4): JSON-like text that does not parse to a
        // recognized shape should synthesize a malformed-action CallTool with
        // the sentinel name so the loop feeds the failure back to the model.
        let raw = r#"{"action": {"tool": "#;
        let decision = parse_react_decision(raw);
        match decision {
            ReactDecision::CallTool { name, arguments } => {
                assert_eq!(
                    name, MALFORMED_TOOL_NAME,
                    "malformed output must use the sentinel name"
                );
                assert!(
                    arguments["reason"]
                        .as_str()
                        .unwrap()
                        .contains("unterminated JSON"),
                    "reason must mention the parse failure: {arguments}"
                );
            }
            other => panic!("expected malformed CallTool, got {other:?}"),
        }
    }

    #[test]
    fn parser_synthesizes_malformed_decision_for_empty_tool_name() {
        let raw = r#"{"action": {"tool": "", "arguments": {}}}"#;
        let decision = parse_react_decision(raw);
        match decision {
            ReactDecision::CallTool { name, arguments } => {
                assert_eq!(name, MALFORMED_TOOL_NAME);
                assert!(
                    arguments["reason"]
                        .as_str()
                        .unwrap()
                        .contains("empty tool name"),
                    "reason must mention empty tool name: {arguments}"
                );
            }
            other => panic!("expected malformed CallTool, got {other:?}"),
        }
    }

    #[test]
    fn parser_synthesizes_malformed_decision_for_action_without_tool_key() {
        let raw = r#"{"action": {"arguments": {}}}"#;
        let decision = parse_react_decision(raw);
        match decision {
            ReactDecision::CallTool { name, arguments } => {
                assert_eq!(name, MALFORMED_TOOL_NAME);
                assert!(
                    arguments["reason"]
                        .as_str()
                        .unwrap()
                        .contains("without tool name"),
                    "reason must mention missing tool name: {arguments}"
                );
            }
            other => panic!("expected malformed CallTool, got {other:?}"),
        }
    }

    #[test]
    fn parser_synthesizes_malformed_decision_for_unrecognized_json_shape() {
        // A JSON object with neither `action` nor an answer field.
        let raw = r#"{"thoughts": "I should call a tool", "plan": "..." }"#;
        let decision = parse_react_decision(raw);
        match decision {
            ReactDecision::CallTool { name, arguments } => {
                assert_eq!(name, MALFORMED_TOOL_NAME);
                assert!(
                    arguments["reason"]
                        .as_str()
                        .unwrap()
                        .contains("unrecognized JSON shape"),
                    "reason must mention unrecognized shape: {arguments}"
                );
            }
            other => panic!("expected malformed CallTool, got {other:?}"),
        }
    }

    #[test]
    fn parser_handles_braces_inside_strings_without_miscounting_depth() {
        // The extract_json_object scanner must not treat `}` inside a string
        // as a depth decrement. Here the answer string contains a brace.
        let raw = r#"{"answer": "the function is `fn() { /* hi */ }`"}"#;
        let decision = parse_react_decision(raw);
        match decision {
            ReactDecision::Finish { output } => {
                assert_eq!(
                    output,
                    serde_json::Value::String("the function is `fn() { /* hi */ }`".into())
                );
            }
            other => panic!("expected Finish, got {other:?}"),
        }
    }

    #[test]
    fn parser_never_panics_on_arbitrary_input() {
        // Fuzz-ish: feed a grab-bag of adversarial inputs and assert we never
        // panic and always produce a variant.
        let adversarial = [
            "",
            "    ",
            "{",
            "}",
            "{{",
            "}}",
            "{{}}",
            "{}",
            "{\"a\":}",
            "{\"a\":1",
            "no json at all just words",
            "{\"action\": null}",
            "{\"action\": []}",
            "{\"action\": {}}",
            "{\"action\": {\"tool\": null}}",
            "{\"action\": {\"tool\": 42}}",
            "```json{\"answer\": \"hi\"}```",
            "{\"answer\": null}",
            "{\"answer\": {}}",
            "```",
            "```json",
            "```json\n```",
        ];
        for input in adversarial {
            // Must not panic; must return a decision.
            let _decision = parse_react_decision(input);
        }
    }

    // -----------------------------------------------------------------------
    // Prompt-formatter tests (directive 1)
    // -----------------------------------------------------------------------

    #[test]
    fn build_messages_emits_system_when_supported() {
        let messages = LmStudioReactAgent::build_messages("hello", &[], true);
        assert_eq!(messages.len(), 2, "system + user");
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[0].content, REACT_SYSTEM_PROMPT);
        assert_eq!(messages[1].role, "user");
        assert_eq!(messages[1].content, "hello");
    }

    #[test]
    fn react_system_prompt_enforces_writes_and_conditional_html_review() {
        // VRO-11.5: the 180s zero-tool turn showed models announcing a plan
        // and yielding instead of executing write_file. Review remains an
        // explicit HTML-only judgment instead of a blanket code mandate.
        assert!(
            REACT_SYSTEM_PROMPT
                .contains("you MUST execute the write_file tool within the same turn"),
            "the write mandate must be present"
        );
        assert!(
            REACT_SYSTEM_PROMPT.contains("workspace-confined HTML")
                && REACT_SYSTEM_PROMPT.contains("Never use it for ordinary source code"),
            "review must be conditional and HTML-only"
        );
        assert!(
            REACT_SYSTEM_PROMPT.contains("Do NOT output your plan and yield to the user"),
            "plan-only yielding must be forbidden"
        );
        assert!(
            REACT_SYSTEM_PROMPT.contains("Execute the tools immediately"),
            "the immediacy mandate must be present"
        );
        assert!(
            REACT_SYSTEM_PROMPT.contains("FAILED turn"),
            "printing artifact content without write_file must be named a failure"
        );
        assert!(
            REACT_SYSTEM_PROMPT.contains("request_human_input"),
            "unresolved planning choices must route through the interactive interview"
        );
    }

    #[test]
    fn build_messages_prepends_contract_to_user_when_system_unsupported() {
        // When the model can't take a system role, the contract is prepended
        // to the user prompt so the model still sees it.
        let messages = LmStudioReactAgent::build_messages("hello", &[], false);
        assert_eq!(messages.len(), 1, "no system message");
        assert_eq!(messages[0].role, "user");
        assert!(messages[0].content.starts_with(REACT_SYSTEM_PROMPT));
        assert!(messages[0].content.ends_with("hello"));
    }

    #[test]
    fn build_messages_replays_action_as_assistant_in_canonical_form() {
        let trajectory = vec![TrajectoryEntry::Action {
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "src/main.rs"}),
        }];
        let messages = LmStudioReactAgent::build_messages("hi", &trajectory, true);
        // system + user prompt + assistant replay
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[2].role, "assistant");
        // The replay is a JSON object with the canonical action shape.
        let parsed: serde_json::Value = serde_json::from_str(&messages[2].content).unwrap();
        assert_eq!(parsed["action"]["tool"], "read_file");
        assert_eq!(parsed["action"]["arguments"]["path"], "src/main.rs");
    }

    #[test]
    fn build_messages_replays_observation_as_user_with_label() {
        let trajectory = vec![
            TrajectoryEntry::Action {
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "a"}),
            },
            TrajectoryEntry::Observation {
                text: "file contents".to_string(),
                success: true,
            },
            TrajectoryEntry::Action {
                name: "missing".to_string(),
                arguments: serde_json::json!({}),
            },
            TrajectoryEntry::Observation {
                text: "no such file".to_string(),
                success: false,
            },
        ];
        let messages = LmStudioReactAgent::build_messages("hi", &trajectory, true);
        // system + user prompt + 2 (Action,Observation) pairs = 6 messages.
        assert_eq!(messages.len(), 6);
        // Observation label is "Observation:" for success, "Error:" for failure.
        assert!(
            messages[3]
                .content
                .starts_with("Observation: file contents")
        );
        assert!(messages[5].content.starts_with("Error: no such file"));
        assert_eq!(messages[3].role, "user");
        assert_eq!(messages[5].role, "user");
    }

    #[test]
    fn build_messages_preserves_trajectory_order() {
        // The agent must see the trajectory in the order it was produced, so
        // a multi-step plan can be reasoned about correctly. Here we replay
        // three actions interleaved with three observations and verify the
        // role sequence.
        let trajectory = vec![
            TrajectoryEntry::Action {
                name: "a".into(),
                arguments: serde_json::json!({}),
            },
            TrajectoryEntry::Observation {
                text: "r1".into(),
                success: true,
            },
            TrajectoryEntry::Action {
                name: "b".into(),
                arguments: serde_json::json!({}),
            },
            TrajectoryEntry::Observation {
                text: "r2".into(),
                success: true,
            },
            TrajectoryEntry::Action {
                name: "c".into(),
                arguments: serde_json::json!({}),
            },
            TrajectoryEntry::Observation {
                text: "r3".into(),
                success: true,
            },
        ];
        let messages = LmStudioReactAgent::build_messages("hi", &trajectory, true);
        let roles: Vec<&str> = messages.iter().map(|m| m.role.as_str()).collect();
        // system, user (prompt), then alternating assistant/user for the
        // action/observation pairs.
        assert_eq!(
            roles,
            vec![
                "system",
                "user",
                "assistant",
                "user",
                "assistant",
                "user",
                "assistant",
                "user"
            ]
        );
    }

    // -----------------------------------------------------------------------
    // End-to-end ReactAgent tests (directive 2)
    // -----------------------------------------------------------------------

    /// Capturing fake transport: records every request and returns a
    /// programmed response. Same pattern as the generator tests.
    struct CapturingTransport {
        requests: Mutex<Vec<LmStudioHttpRequest>>,
        response: LmStudioHttpResponse,
    }

    impl CapturingTransport {
        fn new(response: LmStudioHttpResponse) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                response,
            }
        }
        fn captured(&self) -> Vec<LmStudioHttpRequest> {
            self.requests.lock().expect("poisoned").clone()
        }
    }

    impl LmStudioTransport for CapturingTransport {
        fn send<'a>(
            &'a self,
            req: &'a LmStudioHttpRequest,
        ) -> Pin<Box<dyn Future<Output = Result<LmStudioHttpResponse, LmStudioError>> + Send + 'a>>
        {
            Box::pin(async move {
                self.requests.lock().expect("poisoned").push(req.clone());
                Ok(self.response.clone())
            })
        }
    }

    fn assistant_response(content: &str) -> LmStudioHttpResponse {
        LmStudioHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{"message": {"role": "assistant", "content": content}}],
                "usage": {"completion_tokens": 1}
            }),
        }
    }

    fn basic_agent(transport: Arc<dyn LmStudioTransport>) -> LmStudioReactAgent {
        LmStudioReactAgent::new(
            LmStudioConfig::new("http://localhost:1234/v1").unwrap(),
            "qwen3.6-27b",
            ModelCapabilities::local_server_defaults(),
            transport,
        )
    }

    #[tokio::test]
    async fn next_action_returns_call_tool_when_model_emits_action_json() {
        // Directive 2 + 4: end-to-end through the transport. The model emits
        // a tool-call JSON; the agent returns CallTool.
        let transport = Arc::new(CapturingTransport::new(assistant_response(
            r#"{"action": {"tool": "read_file", "arguments": {"path": "a.txt"}}}"#,
        )));
        let agent = basic_agent(transport.clone());
        let decision = agent.next_action("Read a.txt", &[]).await;
        match decision {
            ReactDecision::CallTool { name, arguments } => {
                assert_eq!(name, "read_file");
                assert_eq!(arguments["path"], "a.txt");
            }
            other => panic!("expected CallTool, got {other:?}"),
        }

        // Verify exactly one POST was sent to chat/completions.
        let captured = transport.captured();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].method, HttpMethod::Post);
        assert_eq!(captured[0].url, "http://localhost:1234/v1/chat/completions");

        // Verify the request body carries the system contract and the prompt.
        let body: serde_json::Value =
            serde_json::from_str(captured[0].body.as_deref().unwrap()).unwrap();
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2, "system + user prompt");
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], REACT_SYSTEM_PROMPT);
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[1]["content"], "Read a.txt");
        assert_eq!(body["model"], "qwen3.6-27b");
        assert_eq!(body["stream"], false);
    }

    #[tokio::test]
    async fn next_action_returns_finish_when_model_emits_answer_json() {
        let transport = Arc::new(CapturingTransport::new(assistant_response(
            r#"{"answer": "the file has 42 lines"}"#,
        )));
        let agent = basic_agent(transport);
        let decision = agent.next_action("How many lines?", &[]).await;
        match decision {
            ReactDecision::Finish { output } => {
                assert_eq!(
                    output,
                    serde_json::Value::String("the file has 42 lines".into())
                );
            }
            other => panic!("expected Finish, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn next_action_returns_finish_when_model_emits_pure_prose() {
        // The model ignored the format and wrote prose. We treat it as the
        // final answer (graceful exit).
        let transport = Arc::new(CapturingTransport::new(assistant_response(
            "I am not sure how to help with that.",
        )));
        let agent = basic_agent(transport);
        let decision = agent.next_action("?", &[]).await;
        match decision {
            ReactDecision::Finish { output } => {
                assert_eq!(
                    output,
                    serde_json::Value::String("I am not sure how to help with that.".into())
                );
            }
            other => panic!("expected Finish, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn next_action_synthesizes_malformed_decision_on_transport_failure() {
        // The transport fails. The agent must NOT crash; it must return a
        // malformed-action CallTool so the loop can feed the failure back.
        struct FailingTransport;
        impl LmStudioTransport for FailingTransport {
            fn send<'a>(
                &'a self,
                _req: &'a LmStudioHttpRequest,
            ) -> Pin<
                Box<dyn Future<Output = Result<LmStudioHttpResponse, LmStudioError>> + Send + 'a>,
            > {
                Box::pin(async move { Err(LmStudioError::Transport("connection refused".into())) })
            }
        }
        let agent = basic_agent(Arc::new(FailingTransport));
        let decision = agent.next_action("hi", &[]).await;
        match decision {
            ReactDecision::CallTool { name, arguments } => {
                assert_eq!(name, MALFORMED_TOOL_NAME);
                assert!(
                    arguments["reason"]
                        .as_str()
                        .unwrap()
                        .contains("transport error"),
                    "reason must surface the transport error: {arguments}"
                );
            }
            other => panic!("expected malformed CallTool, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn next_action_synthesizes_malformed_decision_on_response_parse_failure() {
        // The transport returns a 200 but the body has no `choices[0].message.content`.
        struct EmptyBodyTransport;
        impl LmStudioTransport for EmptyBodyTransport {
            fn send<'a>(
                &'a self,
                _req: &'a LmStudioHttpRequest,
            ) -> Pin<
                Box<dyn Future<Output = Result<LmStudioHttpResponse, LmStudioError>> + Send + 'a>,
            > {
                Box::pin(async move {
                    Ok(LmStudioHttpResponse {
                        status: 200,
                        body: serde_json::json!({"error": "bad"}),
                    })
                })
            }
        }
        let agent = basic_agent(Arc::new(EmptyBodyTransport));
        let decision = agent.next_action("hi", &[]).await;
        match decision {
            ReactDecision::CallTool { name, arguments } => {
                assert_eq!(name, MALFORMED_TOOL_NAME);
                assert!(
                    arguments["reason"]
                        .as_str()
                        .unwrap()
                        .contains("parse error"),
                    "reason must surface the parse error: {arguments}"
                );
            }
            other => panic!("expected malformed CallTool, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn next_action_injects_trajectory_into_request_body() {
        // Directive 1: prove the trajectory is injected into the chat context
        // as assistant/user message pairs, so the model can see its past
        // actions and the resulting observations.
        let transport = Arc::new(CapturingTransport::new(assistant_response(
            r#"{"answer": "done"}"#,
        )));
        let agent = basic_agent(transport.clone());
        let trajectory = vec![
            TrajectoryEntry::Action {
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "a"}),
            },
            TrajectoryEntry::Observation {
                text: "file contents".to_string(),
                success: true,
            },
        ];
        let decision = agent.next_action("Read a", &trajectory).await;
        // Sanity: the model finished.
        assert!(matches!(decision, ReactDecision::Finish { .. }));

        // The captured request body must contain: system + user prompt +
        // assistant (action replay) + user (observation).
        let captured = transport.captured();
        assert_eq!(captured.len(), 1);
        let body: serde_json::Value =
            serde_json::from_str(captured[0].body.as_deref().unwrap()).unwrap();
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 4, "system + user + assistant + user");
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[2]["role"], "assistant");
        // The action replay is the canonical JSON form.
        let replay: serde_json::Value =
            serde_json::from_str(msgs[2]["content"].as_str().unwrap()).unwrap();
        assert_eq!(replay["action"]["tool"], "read_file");
        assert_eq!(replay["action"]["arguments"]["path"], "a");
        assert_eq!(msgs[3]["role"], "user");
        assert_eq!(msgs[3]["content"], "Observation: file contents");
    }

    #[tokio::test]
    async fn malformed_decision_feeds_back_through_loop_as_unknown_tool_observation() {
        // Integration: when the agent returns a malformed CallTool, the
        // VRO-5.1 loop's ToolInvoker returns UnknownTool for the sentinel
        // name, producing a structured failure observation that the agent
        // sees on the next turn. We exercise this end-to-end against the real
        // run_tool_grounded_react loop with a RegistryToolInvoker.
        use crate::executor::uncancellable_context;
        use crate::registry::ToolRegistry;
        use crate::vro::react::run_tool_grounded_react;
        use vesper_domain::{ReasoningBudget, SessionOperatingMode, SessionPermissionMode};

        let context = uncancellable_context(
            Vec::new(),
            SessionOperatingMode::Code,
            SessionPermissionMode::Bypass,
        );
        let invoker = crate::vro::react::RegistryToolInvoker::new(
            ToolRegistry::parity_default(),
            Arc::new(crate::permission::DenyPermissionPort),
            context,
        );

        // Script the agent: first turn returns a malformed decision, second
        // turn finishes (the model self-corrected after seeing the failure).
        use std::sync::Mutex;
        struct TwoTurnAgent {
            turns: Mutex<u32>,
        }
        impl ReactAgent for TwoTurnAgent {
            fn next_action<'a>(
                &'a self,
                _prompt: &'a str,
                trajectory: &'a [TrajectoryEntry],
            ) -> Pin<Box<dyn Future<Output = ReactDecision> + Send + 'a>> {
                let turns = &self.turns;
                Box::pin(async move {
                    let mut count = turns.lock().expect("poisoned");
                    *count += 1;
                    if *count == 1 {
                        // First turn: model emits garbage JSON.
                        return parse_react_decision(r#"{"action": {"tool": "#);
                    }
                    // Second turn: verify the previous observation was the
                    // UnknownTool feedback for our sentinel.
                    let last = trajectory.last().expect("non-empty trajectory");
                    assert!(
                        matches!(last, TrajectoryEntry::Observation { text, success: false }
                            if text.contains(MALFORMED_TOOL_NAME) && text.contains("no executor")),
                        "expected UnknownTool observation for the sentinel, got: {last:?}"
                    );
                    ReactDecision::Finish {
                        output: serde_json::Value::String("recovered".into()),
                    }
                })
            }
        }

        let agent = TwoTurnAgent {
            turns: Mutex::new(0),
        };
        let outcome = run_tool_grounded_react(
            "test",
            &agent,
            &invoker,
            ReasoningBudget {
                max_model_calls: 5,
                max_tool_calls: 5,
                ..ReasoningBudget::balanced()
            },
            // requires_grounding=false so the malformed tool (default
            // Mutating) is not also caught by Read-Before-Write — we are
            // testing the parser feedback path, not R/B/W.
            false,
        )
        .await;
        assert_eq!(outcome.status, vesper_domain::OutcomeStatus::Succeeded);
        assert_eq!(outcome.cost.model_calls, 2);
        // The malformed-action sentinel did consume one tool-call unit
        // (it dispatched to the invoker and got UnknownTool back).
        assert!(
            outcome.cost.total_tokens >= 2,
            "model_calls (2) + at least the malformed dispatch"
        );
    }
}
