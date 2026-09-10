//! Native Responses boundary regression for interview feedback. This verifies
//! adapter serialization, not browser submission or host tool registration.
use super::{auth::AuthenticationMode, tests::fixture_request, wire};
use serde_json::json;
use vesper_domain::*;
use vesper_provider::*;

#[test]
fn interview_tool_calls_and_all_feedback_fields_survive_both_auth_modes() {
    let mut request = fixture_request();
    request.tools[0].id = ToolId::new("request_human_input").unwrap();
    request.tools[0].harness_name = HarnessToolName::new("request_human_input").unwrap();
    request.tools[0].input_schema = json!({"type":"object","properties":{"questions":{"type":"array","items":{"type":"object"}}},"required":["questions"]});
    let arguments = json!({"questions":[{"id":"scope","prompt":"Repair scope?","options":["All findings","Library only"]}]});
    let mut decoder = wire::Decoder::new(&request);
    decoder.event(json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","call_id":"lens_call","name":"request_human_input","arguments":""}})).unwrap();
    let events = decoder.event(json!({"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","call_id":"lens_call","name":"request_human_input","arguments":arguments.to_string()}})).unwrap();
    let ProviderStreamEvent::ToolCallCompleted(call) = events[0].clone() else {
        panic!("missing interview call")
    };
    assert_eq!(call.arguments, arguments);
    assert_eq!(call.tool_id, request.tools[0].id);
    request.messages.push(ConversationMessage {
        id: MessageId::new("lens_assistant").unwrap(),
        role: MessageRole::Assistant,
        content: vec![ContentPart::ToolCall(call)],
        extensions: Default::default(),
    });
    let feedback = json!({"action":"answer","notes":"I approved — repair all findings, including OpenAI.\nKeep my choices.","answers":{"scope":["All findings"],"ledger":"Snapshot readers"}});
    request.messages.push(ConversationMessage {
        id: MessageId::new("lens_result").unwrap(),
        role: MessageRole::Tool,
        content: vec![ContentPart::ToolResult(ToolResult {
            id: ToolResultId::new("lens_result_id").unwrap(),
            call_id: ToolCallId::new("lens_call").unwrap(),
            output: feedback.clone(),
            status: ToolResultStatus::Succeeded,
            locations: vec![],
            diff_summary: None,
            extensions: Default::default(),
        })],
        extensions: Default::default(),
    });
    for mode in [AuthenticationMode::ApiKey, AuthenticationMode::ChatGpt] {
        let body = wire::request(&request, mode, "medium").unwrap();
        assert_eq!(body["tools"][0]["name"], "request_human_input");
        assert_eq!(
            body["tools"][0]["parameters"],
            request.tools[0].input_schema
        );
        let input = body["input"].as_array().unwrap();
        let call = input
            .iter()
            .find(|item| item["type"] == "function_call")
            .unwrap();
        assert_eq!(call["call_id"], "lens_call");
        assert_eq!(call["name"], "request_human_input");
        let result = input
            .iter()
            .find(|item| item["type"] == "function_call_output")
            .unwrap();
        assert_eq!(result["call_id"], "lens_call");
        let decoded: serde_json::Value =
            serde_json::from_str(result["output"].as_str().unwrap()).unwrap();
        assert_eq!(decoded, feedback);
    }
}
