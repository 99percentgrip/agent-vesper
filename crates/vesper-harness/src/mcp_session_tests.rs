//! Real-process regression through the shared host service and prefix gateway.
#![cfg(unix)]
use super::*;
use serde_json::json;
use vesper_agent::ToolService;

fn call(name: &str, arguments: serde_json::Value) -> vesper_domain::ToolCall {
    vesper_domain::ToolCall {
        id: vesper_domain::ToolCallId::new("fixture-call").unwrap(),
        tool_id: vesper_domain::ToolId::new(name).unwrap(),
        arguments,
        extensions: Default::default(),
    }
}

#[tokio::test]
async fn mcp_browser_gateway_and_discovery_share_one_owner_across_turns() {
    let root = tempfile::tempdir().unwrap();
    let stores = Arc::new(MemoryStores::open_at(
        root.path(),
        root.path().join("no-global"),
    ));
    let service = Arc::new(HarnessToolService::new_with_checkpoint_gate(
        stores,
        root.path().join("cron"),
        root.path().join("mcp"),
        None,
        false,
    ));
    let registry = vesper_mcp::McpRegistry::open(&root.path().join("mcp")).unwrap();
    registry.add(vesper_mcp::McpServerConfig {
        id: "fixture".into(), transport: vesper_mcp::McpTransport::Stdio,
        command: Some("python3".into()), args: vec!["-u".into(), "-c".into(), r#"
import json, sys
state = 'about:blank'
for line in sys.stdin:
    r = json.loads(line)
    if 'id' not in r: continue
    result = {}
    if r['method'] == 'tools/list': result = {'tools':[{'name':'browser_snapshot','inputSchema':{'type':'object'}}]}
    if r['method'] == 'tools/call':
        name = r['params']['name']
        if name == 'browser_navigate': state = r['params']['arguments']['url']
        if name == 'browser_click': state += '#clicked'
        result = {'url':state, 'isError':name == 'failure'}
    print(json.dumps({'jsonrpc':'2.0','id':r['id'],'result':result}), flush=True)
"#.into()], url:None, auth_env:None, label:None, created_at:std::time::SystemTime::UNIX_EPOCH,
    }).unwrap();
    let context = vesper_agent::executor::uncancellable_context(
        vec![vesper_domain::WorkspaceRoot {
            name: vesper_domain::BoundedString::new("fixture").unwrap(),
            path: vesper_domain::BoundedString::new(root.path().to_string_lossy()).unwrap(),
            primary: true,
        }],
        vesper_domain::SessionOperatingMode::Code,
        vesper_domain::SessionPermissionMode::Bypass,
    );
    service
        .execute(
            &call(
                "browser_ui",
                json!({"server":"fixture","action":"navigate","arguments":{"url":"fixture-page"}}),
            ),
            &context,
        )
        .await
        .unwrap();
    let discovery = service
        .execute(
            &call("mcp_list_tools", json!({"server":"fixture"})),
            &context,
        )
        .await
        .unwrap();
    assert_eq!(
        discovery.injected_tools[0].harness_name.as_str(),
        "mcp__fixture__browser_snapshot"
    );
    // Rebuilding a turn registry must keep the same process.
    let tools = service.clone().build_default_registry();
    let snapshot = tools
        .execute(&call("mcp__fixture__browser_snapshot", json!({})), &context)
        .await
        .unwrap();
    assert!(snapshot.text.as_str().contains("fixture-page"));
    service
        .execute(
            &call(
                "mcp_call",
                json!({"server":"fixture","tool":"browser_click","arguments":{}}),
            ),
            &context,
        )
        .await
        .unwrap();
    let snapshot = service
        .execute(
            &call(
                "browser_ui",
                json!({"server":"fixture","action":"snapshot","arguments":{}}),
            ),
            &context,
        )
        .await
        .unwrap();
    assert!(snapshot.text.as_str().contains("fixture-page#clicked"));
    let other = Arc::new(service.fork_mcp_session());
    let snapshot = other
        .execute(
            &call(
                "browser_ui",
                json!({"server":"fixture","action":"snapshot","arguments":{}}),
            ),
            &context,
        )
        .await
        .unwrap();
    assert!(snapshot.text.as_str().contains("about:blank"));
    assert!(
        service
            .execute(
                &call(
                    "mcp_call",
                    json!({"server":"fixture","tool":"failure","arguments":{}})
                ),
                &context
            )
            .await
            .is_err()
    );
    assert!(
        tools
            .execute(&call("mcp__fixture__failure", json!({})), &context)
            .await
            .is_err()
    );
    // Permission/worker restriction must still remove executable gateways.
    assert!(
        !tools
            .restricted_to(&["read_file".to_owned()])
            .has_gateway("mcp__")
    );
}

#[test]
fn mcp_tool_failure_is_not_a_success_receipt() {
    assert!(ensure_mcp_success(&json!({"isError":true})).is_err());
    assert!(ensure_mcp_success(&json!({"isError":false})).is_ok());
}
