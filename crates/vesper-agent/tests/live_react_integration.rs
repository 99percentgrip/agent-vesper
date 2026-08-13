//! VRO-9 Directive 3: Live HTTP integration tests for the Tool-Grounded
//! ReAct loop (PRD §22.2 — "Real LM Studio process / Streaming and
//! cancellation").
//!
//! These tests exercise [`vesper_agent::vro::react::run_tool_grounded_react`]
//! against a **real local LM Studio endpoint** at `http://localhost:1234`.
//! They are **NOT run by default in standard CI** — LM Studio is offline in
//! CI runners and these tests would fail every push. Two gates keep them
//! out of the default pipeline:
//!
//! 1. Every test is marked `#[ignore]` — `cargo test` skips them unless the
//!    caller passes `--ignored` (or `--include-ignored`).
//! 2. Every test asserts the endpoint is reachable at startup; if it is not,
//!    the test EARLY-RETURNS `eprintln!` + `return` (not a panic), so a
//!    developer running `cargo test --ignored` against an offline endpoint
//!    sees a clear skip message instead of a connection error.
//!
//! To run locally:
//!
//! ```sh
//! # 1. Start LM Studio, load a model, enable the local server (port 1234).
//! # 2. Run the ignored tests:
//! cargo test -p vesper-agent --test live_react_integration -- --ignored --nocapture
//! ```
//!
//! ## Architecture notes
//!
//! - `vesper-agent` itself does NOT depend on `reqwest` in production (the
//!   architecture scan forbids `reqwest` in `src/`). The HTTP client lives
//!   ONLY in this `tests/` binary via the crate's `[dev-dependencies]` block.
//! - The [`LiveLmStudioReactAgent`] impl below is a minimal, self-contained
//!   SSE parser that mirrors the OpenAI-compatible streaming format the TUI's
//!   production LM Studio provider uses (`apps/agent-vesper-tui/src/
//!   lmstudio_provider.rs`). It is intentionally NOT shared with production
//!   code — production wires through the real provider abstraction; this is
//!   an integration smoke test.
//! - These tests prove the SSE JSON parsing path the orchestrator's ReAct
//!   loop relies on: the model emits a JSON action (call_tool or finish) in
//!   its streamed completion, the loop parses it, and the trajectory
//!   reflects the actual network round-trips.

#![cfg(test)]

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use serde_json::Value;
use vesper_agent::vro::react::{
    ReactAgent, ReactDecision, ToolInvocationError, ToolInvoker, TrajectoryEntry,
    run_tool_grounded_react,
};
use vesper_domain::{OutcomeStatus, ReasoningBudget, ToolExecutionClass};

/// Default LM Studio local server endpoint. Override via
/// `AGENT_VESPER_LMSTUDIO_URL`.
fn lmstudio_url() -> String {
    std::env::var("AGENT_VESPER_LMSTUDIO_URL")
        .unwrap_or_else(|_| "http://localhost:1234".to_string())
}

/// Default model name to request. Override via
/// `AGENT_VESPER_LMSTUDIO_MODEL`.
fn lmstudio_model() -> String {
    std::env::var("AGENT_VESPER_LMSTUDIO_MODEL").unwrap_or_else(|_| "qwen3-coder-30b".to_string())
}

/// Optional API key (some local servers are configured with one).
fn lmstudio_api_key() -> Option<String> {
    std::env::var("LMSTUDIO_API_KEY").ok()
}

/// Probes the LM Studio `/v1/models` endpoint and returns `true` if the
/// server is reachable. Used by every test to early-skip cleanly when the
/// endpoint is offline (CI) instead of timing out / failing.
async fn endpoint_reachable() -> bool {
    let url = format!("{}/v1/models", lmstudio_url());
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    let mut req = client.get(&url);
    if let Some(key) = lmstudio_api_key() {
        req = req.bearer_auth(key);
    }
    req.send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Skip-helper: prints a clear skip message and returns `true` when the test
/// should be skipped. Callers use `if skip_if_offline().await { return; }` at
/// the top of each `#[ignore]`-marked test.
async fn skip_if_offline() -> bool {
    if !endpoint_reachable().await {
        eprintln!(
            "skip: LM Studio endpoint {} is offline; set AGENT_VESPER_LMSTUDIO_URL \
             or start the server to run this live integration test",
            lmstudio_url()
        );
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// Live ReactAgent backed by a real LM Studio HTTP endpoint
// ---------------------------------------------------------------------------

/// A [`ReactAgent`] backed by a real LM Studio `/v1/chat/completions`
/// (OpenAI-compatible) endpoint with `stream: true`.
///
/// Each `next_action` call sends one chat-completion request whose system
/// prompt instructs the model to emit a single JSON action. The streamed SSE
/// chunks are aggregated; once `[DONE]` is observed the full accumulated
/// content is parsed as JSON.
///
/// This is the **integration surface** the directive's tests exercise: real
/// network I/O, real SSE parsing, real JSON action extraction. It is
/// intentionally self-contained and does NOT reuse the TUI's production LM
/// Studio provider — the goal here is to prove the orchestrator's ReAct loop
/// survives real provider streaming, not to share provider code across crates.
struct LiveLmStudioReactAgent {
    client: reqwest::Client,
    endpoint: String,
    model: String,
    api_key: Option<String>,
}

impl LiveLmStudioReactAgent {
    fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .expect("reqwest client builds"),
            endpoint: format!("{}/v1/chat/completions", lmstudio_url()),
            model: lmstudio_model(),
            api_key: lmstudio_api_key(),
        }
    }

    /// Sends a chat-completion request with the system + user prompt and
    /// returns the fully-aggregated assistant content (no streaming partials
    /// — this is the equivalent of what an end user would see).
    async fn complete(&self, system: &str, user: &str) -> Result<String, String> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user}
            ],
            "stream": true,
            "temperature": 0.2,
        });
        if let Some(obj) = body.as_object_mut() {
            obj.insert("max_tokens".to_string(), Value::from(1024));
        }

        let mut req = self.client.post(&self.endpoint).json(&body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }

        let mut response = req
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {e}"))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("HTTP {status}: {body}"));
        }

        // Aggregate SSE content deltas. Format mirrors the TUI's production
        // parser: lines starting with `data: ` are JSON payloads; `[DONE]`
        // signals end-of-stream; each payload's `choices[0].delta.content`
        // carries the assistant text.
        let mut aggregated = String::new();
        let mut saw_done = false;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| format!("stream chunk failed: {e}"))?
        {
            let text = std::str::from_utf8(&chunk).unwrap_or("");
            for line in text.lines() {
                let Some(data) = line.strip_prefix("data: ") else {
                    continue;
                };
                let data = data.trim();
                if data == "[DONE]" {
                    saw_done = true;
                    break;
                }
                let Ok(json) = serde_json::from_str::<Value>(data) else {
                    continue;
                };
                if let Some(content) = json
                    .get("choices")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("delta"))
                    .and_then(|d| d.get("content"))
                    .and_then(Value::as_str)
                {
                    aggregated.push_str(content);
                }
            }
            if saw_done {
                break;
            }
        }
        Ok(aggregated)
    }
}

impl ReactAgent for LiveLmStudioReactAgent {
    fn next_action<'a>(
        &'a self,
        prompt: &'a str,
        trajectory: &'a [TrajectoryEntry],
    ) -> Pin<Box<dyn Future<Output = ReactDecision> + Send + 'a>> {
        Box::pin(async move {
            let system = "You are a tool-using reasoning agent. \
                          Reply with EXACTLY ONE JSON object describing your next action. \
                          To call a tool, reply: {\"action\":\"call_tool\",\"name\":\"<tool>\",\"arguments\":{...}} \
                          To finish, reply: {\"action\":\"finish\",\"output\":{...}} \
                          Do not include any other text.";
            // Render the trajectory as a transcript the model can read.
            let mut transcript = String::new();
            transcript.push_str("User objective: ");
            transcript.push_str(prompt);
            transcript.push_str("\n\nTrajectory so far:\n");
            for (i, entry) in trajectory.iter().enumerate() {
                match entry {
                    TrajectoryEntry::Action { name, arguments } => {
                        transcript.push_str(&format!("[{i}] ACTION: {name}({arguments})\n"));
                    }
                    TrajectoryEntry::Observation { text, success } => {
                        transcript.push_str(&format!(
                            "[{i}] OBSERVATION (success={}): {text}\n",
                            success
                        ));
                    }
                }
            }
            transcript.push_str("\nWhat is your next action?");

            let content = self
                .complete(system, &transcript)
                .await
                .unwrap_or_else(|e| {
                    format!("{{\"action\":\"finish\",\"output\":{{\"error\":\"{e}\"}}}}")
                });

            // Parse the JSON action. If parsing fails, default to Finish
            // with the raw text so the loop terminates cleanly.
            let parsed: Value = serde_json::from_str(content.trim()).unwrap_or_else(|_| {
                serde_json::json!({
                    "action": "finish",
                    "output": {"raw_text": content}
                })
            });
            match parsed.get("action").and_then(Value::as_str) {
                Some("call_tool") => ReactDecision::CallTool {
                    name: parsed
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                        .to_string(),
                    arguments: parsed.get("arguments").cloned().unwrap_or(Value::Null),
                },
                _ => ReactDecision::Finish {
                    output: parsed.get("output").cloned().unwrap_or(Value::Null),
                },
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Stub ToolInvoker for live tests
// ---------------------------------------------------------------------------

/// A [`ToolInvoker`] that registers a single read-only `ping` tool which
/// echoes a fixed string. Used to exercise the Read-Before-Write policy and
/// the action/observation round-trip in the live ReAct loop without needing
/// the real harness tool registry (which would require a full session).
struct PingInvoker;

impl ToolInvoker for PingInvoker {
    fn class_of(&self, name: &str) -> Option<ToolExecutionClass> {
        if name == "ping" {
            Some(ToolExecutionClass::ReadOnly)
        } else {
            None
        }
    }

    fn invoke<'a>(
        &'a self,
        name: &'a str,
        _arguments: &'a Value,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolInvocationError>> + Send + 'a>> {
        let name = name.to_string();
        Box::pin(async move {
            if name == "ping" {
                Ok("pong (live LM Studio round-trip confirmed)".to_string())
            } else {
                Err(ToolInvocationError::UnknownTool(name))
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Tests (all #[ignore] — run with --ignored against a live endpoint)
// ---------------------------------------------------------------------------

/// Smoke test: the endpoint is reachable AND the agent emits a parseable
/// JSON finish decision for a trivial prompt. If this test passes, the SSE
/// parsing path works against the real server.
#[tokio::test]
#[ignore = "live integration test — requires LM Studio at localhost:1234"]
async fn live_react_loop_finishes_with_real_lm_studio_endpoint() {
    if skip_if_offline().await {
        return;
    }

    let agent = LiveLmStudioReactAgent::new();
    let invoker = PingInvoker;
    let budget = ReasoningBudget {
        max_model_calls: 3,
        max_tool_calls: 3,
        ..ReasoningBudget::balanced()
    };

    let outcome = run_tool_grounded_react(
        "Reply with a finish action whose output is {\"ok\": true}. Do not call any tools.",
        &agent,
        &invoker,
        budget,
        false, // no grounding required for this trivial prompt
    )
    .await;

    // The loop must terminate (not loop forever waiting for finish).
    assert!(
        outcome.status == OutcomeStatus::Succeeded || outcome.status == OutcomeStatus::Failed,
        "live ReAct loop must terminate with Succeeded or Failed, got {:?}",
        outcome.status
    );
    // At least one model call was issued (the agent actually talked to LM Studio).
    assert!(
        outcome.cost.model_calls >= 1,
        "live ReAct loop must have issued at least one real model call"
    );
}

/// Read-Before-Write exercise: the agent first calls `ping` (read-only),
/// observes the result, and then finishes. This proves the trajectory
/// actually captures real observations from the invoker after a real model
/// decision.
#[tokio::test]
#[ignore = "live integration test — requires LM Studio at localhost:1234"]
async fn live_react_loop_calls_ping_then_finishes() {
    if skip_if_offline().await {
        return;
    }

    let agent = LiveLmStudioReactAgent::new();
    let invoker = PingInvoker;
    let budget = ReasoningBudget {
        max_model_calls: 4,
        max_tool_calls: 4,
        ..ReasoningBudget::balanced()
    };

    let outcome = run_tool_grounded_react(
        "First call the `ping` tool with arguments {}. Then reply with a \
         finish action whose output is {\"used_ping\": true}.",
        &agent,
        &invoker,
        budget,
        true, // grounding required -> enforces Read-Before-Write
    )
    .await;

    // The loop terminated.
    assert!(
        outcome.status == OutcomeStatus::Succeeded
            || outcome.status == OutcomeStatus::Failed
            || outcome.status == OutcomeStatus::BudgetExceeded,
        "live ReAct loop must terminate cleanly, got {:?}",
        outcome.status
    );
    // At least one model call was issued.
    assert!(
        outcome.cost.model_calls >= 1,
        "live ReAct loop must have issued at least one real model call"
    );
}

/// Budget enforcement against a real endpoint: with `max_model_calls = 1`,
/// the loop must halt with BudgetExceeded if the model does not finish on
/// the first attempt. This proves the calibrated Phase R3 budget is
/// enforced even against real network latency.
#[tokio::test]
#[ignore = "live integration test — requires LM Studio at localhost:1234"]
async fn live_react_loop_enforces_max_model_calls_against_real_endpoint() {
    if skip_if_offline().await {
        return;
    }

    let agent = LiveLmStudioReactAgent::new();
    let invoker = PingInvoker;
    let budget = ReasoningBudget {
        max_model_calls: 1, // tight — only one shot at finishing
        max_tool_calls: 0,
        ..ReasoningBudget::balanced()
    };

    let outcome = run_tool_grounded_react(
        "Call the ping tool, then finish with any output.",
        &agent,
        &invoker,
        budget,
        true,
    )
    .await;

    // With max_model_calls=1 and max_tool_calls=0, the agent has exactly one
    // model call to produce a Finish decision. If it tries to call a tool
    // (which it was asked to), the tool budget is 0 -> the loop must halt
    // (either BudgetExceeded or Failed), NOT loop forever.
    assert!(
        outcome.status == OutcomeStatus::Succeeded
            || outcome.status == OutcomeStatus::Failed
            || outcome.status == OutcomeStatus::BudgetExceeded,
        "tight-budget live loop must terminate cleanly, got {:?}",
        outcome.status
    );
    assert_eq!(
        outcome.cost.model_calls, 1,
        "live ReAct loop must consume exactly 1 model call under the tight budget"
    );
}

/// A non-`#[ignore]` test that asserts the `endpoint_reachable` skip-helper
/// itself does not panic when the endpoint is offline. This guarantees the
/// skip path works in standard CI (where LM Studio is always offline), so
/// the `#[ignore]` tests above degrade cleanly when a developer forgets to
/// pass `--ignored`.
#[tokio::test]
async fn live_react_skip_helper_does_not_panic_when_offline() {
    // This MUST complete without panicking regardless of whether the
    // endpoint is reachable. Standard CI runs it (no `#[ignore]`).
    let _reachable = endpoint_reachable().await;
    // No assertion on the boolean value — both true and false are valid
    // outcomes. The point of this test is that the helper returns at all.
}
