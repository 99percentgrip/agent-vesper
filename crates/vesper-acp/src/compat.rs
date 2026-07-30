use agent_client_protocol::schema::v1::StopReason;
use serde_json::{Value, json};
use vesper_domain::MessageId;

/// Pinned official Rust SDK crate version.
pub const ACP_SDK_VERSION: &str = "2.0.0";
/// Negotiated ACP wire protocol.
pub const ACP_WIRE_PROTOCOL: u8 = 1;

/// Produces the frozen protocol-v1 prompt result shape.
///
/// SDK 2.0.0 does not expose `userMessageId` on its typed `PromptResponse`.
/// The adapter therefore uses the SDK's supported erased JSON response path for
/// this response only.
#[must_use]
pub fn prompt_response_value(stop_reason: StopReason, user_message_id: &MessageId) -> Value {
    json!({
        "stopReason": stop_reason,
        "userMessageId": user_message_id.as_str(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_user_message_id_is_top_level() {
        let value =
            prompt_response_value(StopReason::EndTurn, &MessageId::new("message-1").unwrap());
        assert_eq!(value["stopReason"], "end_turn");
        assert_eq!(value["userMessageId"], "message-1");
    }
}
