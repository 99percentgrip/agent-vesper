//! Disposable ACP SDK compatibility probes.

pub const WIRE_PROTOCOL: u8 = 1;

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use agent_client_protocol::{
        Agent, Client, ConnectionTo, Responder,
        schema::{
            ProtocolVersion,
            v1::{
                CancelNotification, CloseSessionRequest, ContentBlock, ContentChunk,
                ForkSessionRequest, InitializeRequest, InitializeResponse, ListSessionsRequest,
                LoadSessionRequest, NewSessionRequest, NewSessionResponse, PromptRequest,
                PromptResponse, ResumeSessionRequest, SessionId, SessionNotification,
                SessionUpdate, StopReason, TextContent, UsageUpdate,
            },
        },
    };
    use serde_json::{Value, json};

    #[test]
    fn python_fixture_is_consumable_and_protocol_is_v1() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../fixtures/acp/initialization/result.python.json"
        ))
        .expect("Python ACP fixture must remain valid JSON");
        assert_eq!(fixture["scenario_id"], "acp.initialization");

        let request = InitializeRequest::new(ProtocolVersion::V1);
        assert_eq!(
            serde_json::to_value(request).expect("serialize initialize")["protocolVersion"],
            1
        );
    }

    #[test]
    fn lifecycle_prompt_cancel_and_ids_serialize_as_protocol_v1() {
        let session = SessionId::new("fixture-session");
        let cwd = PathBuf::from("/fixture");
        let values = [
            serde_json::to_value(NewSessionRequest::new(cwd.clone())).unwrap(),
            serde_json::to_value(LoadSessionRequest::new(session.clone(), cwd.clone())).unwrap(),
            serde_json::to_value(ResumeSessionRequest::new(session.clone(), cwd.clone())).unwrap(),
            serde_json::to_value(ForkSessionRequest::new(session.clone(), cwd)).unwrap(),
            serde_json::to_value(ListSessionsRequest::new()).unwrap(),
            serde_json::to_value(CloseSessionRequest::new(session.clone())).unwrap(),
            serde_json::to_value(PromptRequest::new(
                session.clone(),
                vec![ContentBlock::Text(TextContent::new("fixture"))],
            ))
            .unwrap(),
            serde_json::to_value(CancelNotification::new(session)).unwrap(),
        ];
        assert_eq!(values[1]["sessionId"], "fixture-session");
        assert_eq!(values[2]["sessionId"], "fixture-session");
        assert_eq!(values[3]["sessionId"], "fixture-session");
        assert_eq!(values[5]["sessionId"], "fixture-session");
        assert_eq!(values[6]["sessionId"], "fixture-session");
        assert_eq!(values[7]["sessionId"], "fixture-session");
    }

    #[test]
    fn normalized_update_order_and_usage_shape_are_preserved() {
        let session = SessionId::new("fixture-session");
        let updates = [
            SessionUpdate::AgentThoughtChunk(ContentChunk::new(ContentBlock::Text(
                TextContent::new("think"),
            ))),
            SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                TextContent::new("answer"),
            ))),
            SessionUpdate::UsageUpdate(UsageUpdate::new(12, 131_072)),
        ];
        let encoded: Vec<Value> = updates
            .into_iter()
            .map(|update| {
                serde_json::to_value(SessionNotification::new(session.clone(), update)).unwrap()
            })
            .collect();
        assert_eq!(encoded[0]["update"]["sessionUpdate"], "agent_thought_chunk");
        assert_eq!(encoded[1]["update"]["sessionUpdate"], "agent_message_chunk");
        assert_eq!(encoded[2]["update"]["sessionUpdate"], "usage_update");
        assert_eq!(encoded[2]["update"]["used"], 12);
        assert_eq!(encoded[2]["update"]["size"], 131_072);
    }

    #[test]
    fn malformed_protocol_fields_are_rejected_or_defaulted_by_sdk_schema() {
        let wrong_required = json!({"protocolVersion": "one"});
        assert!(serde_json::from_value::<InitializeRequest>(wrong_required).is_err());

        let tolerant_optional = json!({"protocolVersion": 1, "clientCapabilities": "invalid"});
        let decoded: InitializeRequest =
            serde_json::from_value(tolerant_optional).expect("optional capability defaults");
        assert_eq!(decoded.protocol_version, ProtocolVersion::V1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn session_callback_can_consume_later_updates_without_deadlock() {
        let session_id = SessionId::new("ordered-session");
        let response_id = session_id.clone();
        let prompt_id = session_id.clone();
        let agent = Agent
            .builder()
            .on_receive_request(
                async move |_request: NewSessionRequest,
                            responder: Responder<NewSessionResponse>,
                            _connection: ConnectionTo<Client>| {
                    responder.respond(NewSessionResponse::new(response_id.clone()))
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |request: PromptRequest,
                            responder: Responder<PromptResponse>,
                            connection: ConnectionTo<Client>| {
                    assert_eq!(request.session_id, prompt_id);
                    connection.send_notification(SessionNotification::new(
                        request.session_id,
                        SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                            TextContent::new("ordered response"),
                        ))),
                    ))?;
                    responder.respond(PromptResponse::new(StopReason::EndTurn))
                },
                agent_client_protocol::on_receive_request!(),
            );

        let client = Client
            .builder()
            .connect_with(agent, async move |connection| {
                connection
                    .build_session_cwd()?
                    .block_task()
                    .run_until(async |mut session| {
                        session.send_prompt("fixture")?;
                        assert_eq!(session.read_to_string().await?, "ordered response");
                        Ok(())
                    })
                    .await
            });
        tokio::time::timeout(Duration::from_secs(2), client)
            .await
            .expect("session callback deadlocked")
            .expect("connection failed");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ordered_callbacks_block_later_inbound_dispatch() {
        let order = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&order);
        let agent = Agent.builder().on_receive_notification(
            async move |notification: CancelNotification, _cx| {
                let id = notification.session_id.to_string();
                observed.lock().unwrap().push(format!("start-{id}"));
                if id == "one" {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                observed.lock().unwrap().push(format!("end-{id}"));
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        );

        Client
            .builder()
            .connect_with(agent, async move |connection| {
                connection.send_notification(CancelNotification::new("one"))?;
                connection.send_notification(CancelNotification::new("two"))?;
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok(())
            })
            .await
            .expect("connection failed");

        assert_eq!(
            *order.lock().unwrap(),
            ["start-one", "end-one", "start-two", "end-two"]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn clean_shutdown_completes_when_both_sides_finish() {
        let agent = Agent.builder().on_receive_request(
            async move |initialize: InitializeRequest, responder, _cx| {
                responder.respond(InitializeResponse::new(initialize.protocol_version))
            },
            agent_client_protocol::on_receive_request!(),
        );
        tokio::time::timeout(
            Duration::from_secs(2),
            Client
                .builder()
                .connect_with(agent, async move |connection| {
                    let response = connection
                        .send_request(InitializeRequest::new(ProtocolVersion::V1))
                        .block_task()
                        .await?;
                    assert_eq!(response.protocol_version, ProtocolVersion::V1);
                    Ok(())
                }),
        )
        .await
        .expect("shutdown timed out")
        .expect("connection failed");
    }
}
