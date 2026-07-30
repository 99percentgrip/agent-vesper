use std::{fs, path::PathBuf};

use serde_json::{Value, json};
use vesper_domain::SessionId;
use vesper_sessions::{
    BoundViolation, BoxSessionFuture, CorruptLegacyRecord, LegacyDecodeBounds, LegacyLoadOutcome,
    LegacySessionDecoder, MetadataOrigin, SessionMetadata, SessionReadIntent, SessionReader,
    SessionSource, SessionStoreError,
};

fn metadata(id: &str) -> SessionMetadata {
    SessionMetadata {
        session_id: SessionId::new(id).unwrap(),
        source: SessionSource::LegacyNativeGlm { profile: None },
        byte_len: 0,
        modified: None,
        record_path: None,
        metadata_path: None,
        origin: MetadataOrigin::FilesystemEntry,
        title: None,
        cwd: String::new(),
        updated_at: None,
        model: None,
        provider: None,
        parent_session_id: None,
        branch_root_id: None,
        safe_preview: None,
        read_only: true,
    }
}

fn fixture_result(scenario: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/sessions/v1")
        .join(scenario)
        .join("result.python.json");
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn all_seven_authoritative_session_scenarios_have_typed_decode_coverage() {
    let decoder = LegacySessionDecoder::default();
    for scenario in [
        "schema1-complete",
        "minimal-legacy",
        "replay-and-lineage",
        "reasoning-enabled",
        "reasoning-disabled",
    ] {
        let result = fixture_result(scenario);
        let bytes = serde_json::to_vec(&result["final_state"]).unwrap();
        let LegacyLoadOutcome::Loaded(decoded) =
            decoder.decode_record(metadata("fixture-session"), &bytes)
        else {
            panic!("fixture {scenario} did not decode");
        };
        assert_eq!(decoded.session.version, 1);
        assert_eq!(decoded.session.cwd, "$WORKSPACE");
    }

    let corrupt = fixture_result("corrupt-json");
    assert!(corrupt["final_state"]["loaded"].is_null());
    assert_eq!(
        decoder.decode_record(metadata("corrupt"), b"{broken"),
        LegacyLoadOutcome::Corrupt(CorruptLegacyRecord::MalformedJson)
    );

    let unknown = fixture_result("unknown-fields");
    assert_eq!(unknown["final_state"]["unknown_accepted"], true);
    let LegacyLoadOutcome::Loaded(decoded) = decoder.decode_record(
        metadata("unknown"),
        br#"{"future_field":{"preserve":true}}"#,
    ) else {
        panic!("unknown-field vector did not decode");
    };
    let round_trip = decoded.session.encode_json().unwrap();
    let LegacyLoadOutcome::Loaded(round_tripped) =
        decoder.decode_record(metadata("unknown"), &round_trip)
    else {
        panic!("unknown-field round trip did not decode");
    };
    assert_eq!(
        round_tripped.session.unknown_fields["future_field"],
        json!({"preserve": true})
    );
}

#[test]
fn omitted_fields_and_legacy_compatibility_surfaces_are_preserved() {
    let bytes = serde_json::to_vec(&json!({
        "cwd": "/workspace",
        "additional_directories": ["/workspace/vendor"],
        "model": "glm-5.2",
        "api_endpoint": "coding",
        "thought_level": "high",
        "messages": [{
            "role": "assistant",
            "content": "done",
            "reasoning_content": "provider-visible",
            "tool_calls": [{"id": "call-1", "function": {"name": "read_file", "arguments": "{}"}}]
        }],
        "loaded_tool_names": ["read_file"],
        "total_input_tokens": 10,
        "total_output_tokens": 5,
        "total_cached_tokens": 2,
        "future_extension": {"retain": ["all", "values"]}
    }))
    .unwrap();
    let LegacyLoadOutcome::Loaded(decoded) =
        LegacySessionDecoder::default().decode_record(metadata("compat"), &bytes)
    else {
        panic!("compatibility record did not decode");
    };
    assert_eq!(decoded.session.version, 1);
    assert_eq!(decoded.session.generation_profile, "balanced");
    assert_eq!(decoded.session.permission_mode, "ask");
    assert!(decoded.session.contains_persisted_reasoning());
    assert!(
        decoded
            .session
            .unknown_fields
            .contains_key("additional_directories")
    );
    assert!(
        decoded
            .session
            .unknown_fields
            .contains_key("future_extension")
    );
}

#[test]
fn version_and_each_high_risk_bound_have_typed_outcomes() {
    let decoder = LegacySessionDecoder::new(LegacyDecodeBounds {
        max_file_bytes: 1_024,
        max_messages: 1,
        max_content_bytes: 4,
        max_plan_items: 1,
        max_plan_bytes: 16,
        max_roots: 1,
        max_root_bytes: 8,
        max_metadata_extension_fields: 1,
        max_unknown_bytes: 1_024,
        max_unknown_nodes: 16,
        max_json_depth: 4,
        max_lineage_id_bytes: 8,
        max_compatibility_array_items: 1,
        max_compatibility_value_bytes: 1_024,
    });

    assert_eq!(
        decoder.decode_record(metadata("v2"), br#"{"version":2}"#),
        LegacyLoadOutcome::UnsupportedVersion(2)
    );
    for (bytes, field) in [
        (br#"{"messages":[{},{}]}"#.as_slice(), "messages"),
        (
            br#"{"messages":[{"content":"12345"}]}"#.as_slice(),
            "message_content",
        ),
        (br#"{"plan":[{},{}]}"#.as_slice(), "plan"),
        (
            br#"{"additional_roots":["/one","/two"]}"#.as_slice(),
            "additional_roots",
        ),
        (
            br#"{"parent_session_id":"123456789"}"#.as_slice(),
            "parent_session_id",
        ),
        (
            br#"{"loaded_tool_names":["one","two"]}"#.as_slice(),
            "loaded_tool_names",
        ),
        (
            br#"{"future_one":true,"future_two":true}"#.as_slice(),
            "metadata_extensions",
        ),
    ] {
        let outcome = decoder.decode_record(metadata("bounded"), bytes);
        assert!(
            matches!(
                outcome,
                LegacyLoadOutcome::RejectedByBounds(BoundViolation {
                    field: actual,
                    ..
                }) if actual == field
            ),
            "{field}: {outcome:?}"
        );
    }

    let file_bound_decoder = LegacySessionDecoder::new(LegacyDecodeBounds {
        max_file_bytes: 4,
        ..LegacyDecodeBounds::default()
    });
    assert_eq!(
        file_bound_decoder.decode_record(metadata("large"), br#"{"version":1}"#),
        LegacyLoadOutcome::RejectedByBounds(BoundViolation {
            field: "file_bytes",
            maximum: 4
        })
    );
}

struct OutcomeReader {
    error: Option<SessionStoreError>,
}

impl SessionReader for OutcomeReader {
    fn source(&self) -> SessionSource {
        SessionSource::LegacyNativeGlm { profile: None }
    }

    fn read<'a>(
        &'a self,
        _session_id: &'a SessionId,
        _intent: SessionReadIntent,
    ) -> BoxSessionFuture<'a, Result<Option<vesper_sessions::SessionRecord>, SessionStoreError>>
    {
        let result = self.error.as_ref().map_or(Ok(None), |error| {
            Err(match error {
                SessionStoreError::PathEscapesRoot => SessionStoreError::PathEscapesRoot,
                SessionStoreError::Io(error) => {
                    SessionStoreError::Io(std::io::Error::from(error.kind()))
                }
                _ => panic!("unsupported test error"),
            })
        });
        Box::pin(async move { result })
    }
}

#[tokio::test]
async fn missing_permission_and_unsafe_paths_are_distinct() {
    let decoder = LegacySessionDecoder::default();
    let id = SessionId::new("fixture").unwrap();
    assert_eq!(
        decoder.load(&OutcomeReader { error: None }, &id).await,
        LegacyLoadOutcome::Missing
    );
    assert_eq!(
        decoder
            .load(
                &OutcomeReader {
                    error: Some(SessionStoreError::Io(std::io::Error::from(
                        std::io::ErrorKind::PermissionDenied,
                    ))),
                },
                &id,
            )
            .await,
        LegacyLoadOutcome::PermissionDenied
    );
    assert_eq!(
        decoder
            .load(
                &OutcomeReader {
                    error: Some(SessionStoreError::PathEscapesRoot),
                },
                &id,
            )
            .await,
        LegacyLoadOutcome::UnsafePath
    );
}
