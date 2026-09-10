//! GLM serialization counterpart to native OpenAI interview feedback coverage.
//! This is adapter-boundary evidence, not browser/host end-to-end acceptance.
use serde_json::json;
use vesper_domain::*;

#[test]
fn interview_feedback_preserves_notes_answers_and_call_linkage() {
    let mut request = super::request(true);
    request.endpoint_id = None;
    request.tools.truncate(1);
    request.tools[0].id = ToolId::new("request_human_input").unwrap();
    request.tools[0].harness_name = HarnessToolName::new("request_human_input").unwrap();
    let feedback = json!({"action":"answer","notes":"I approved — repair all findings, including OpenAI.\nKeep my choices.","answers":{"scope":["All findings"],"ledger":"Snapshot readers"}});
    request.messages = vec![
        ConversationMessage {
            id: MessageId::new("lens_call_message").unwrap(),
            role: MessageRole::Assistant,
            content: vec![ContentPart::ToolCall(ToolCall {
                id: ToolCallId::new("lens_call").unwrap(),
                tool_id: request.tools[0].id.clone(),
                arguments: json!({"questions":[{"id":"scope","prompt":"Repair scope?"}]}),
                extensions: Default::default(),
            })],
            extensions: Default::default(),
        },
        ConversationMessage {
            id: MessageId::new("lens_result_message").unwrap(),
            role: MessageRole::Tool,
            content: vec![ContentPart::ToolResult(ToolResult {
                id: ToolResultId::new("lens_result").unwrap(),
                call_id: ToolCallId::new("lens_call").unwrap(),
                output: feedback.clone(),
                status: ToolResultStatus::Succeeded,
                locations: vec![],
                diff_summary: None,
                extensions: Default::default(),
            })],
            extensions: Default::default(),
        },
    ];
    let config = crate::GlmConfig {
        model: request.model.model_id.clone(),
        ..Default::default()
    };
    let body = crate::request::serialize_request(&request, &config)
        .unwrap()
        .body;
    assert_eq!(body["tools"][0]["function"]["name"], "request_human_input");
    assert_eq!(
        body["tools"][0]["function"]["parameters"],
        request.tools[0].input_schema
    );
    let messages = body["messages"].as_array().unwrap();
    let assistant = messages
        .iter()
        .find(|message| message["role"] == "assistant")
        .unwrap();
    assert_eq!(assistant["tool_calls"][0]["id"], "lens_call");
    let result = messages
        .iter()
        .find(|message| message["role"] == "tool")
        .unwrap();
    assert_eq!(result["tool_call_id"], "lens_call");
    let decoded: serde_json::Value =
        serde_json::from_str(result["content"].as_str().unwrap()).unwrap();
    assert_eq!(decoded, feedback);
}
