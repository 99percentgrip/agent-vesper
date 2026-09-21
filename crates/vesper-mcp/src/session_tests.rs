//! Real stdio fixtures; no live endpoint or user state.
#![cfg(unix)]
use crate::{McpServerConfig, McpSession, McpTransport};
use serde_json::json;
use std::time::{Duration, Instant};

fn fixture() -> McpServerConfig {
    McpServerConfig {
        id: "stateful_fixture".into(),
        transport: McpTransport::Stdio,
        command: Some("python3".into()),
        args: vec!["-u".into(), "-c".into(), r#"
import json, sys, time, os
state = 'about:blank'
last_id = 0
for line in sys.stdin:
    r = json.loads(line)
    if 'id' not in r: continue
    assert r['id'] > last_id
    last_id = r['id']
    method = r['method']
    result = {}
    if method == 'tools/list':
        result = {'tools': [{'name': 'snapshot', 'inputSchema': {'type': 'object'}}]}
    if method == 'tools/call':
        name = r['params']['name']
        if name == 'navigate': state = r['params']['arguments']['url']
        if name == 'click': state += '#clicked'
        if name == 'hang': time.sleep(60)
        if name == 'exit': sys.exit(0)
        if name == 'oversize':
            print('x' * (1024 * 1024 + 2), flush=True)
            continue
        result = {'url': state, 'pid': os.getpid()}
        if name == 'tool_error': result['isError'] = True
        if name == 'rpc_error':
            print(json.dumps({'jsonrpc':'2.0','id':r['id'],'error':{'code':-1,'message':'fixture error'}}), flush=True)
            continue
    print(json.dumps({'jsonrpc':'2.0','method':'notifications/message','params':{}}), flush=True)
    print(json.dumps({'jsonrpc':'2.0','id':r['id'],'result':result}), flush=True)
"#.into()],
        url: None, auth_env: None, label: None,
        created_at: std::time::SystemTime::UNIX_EPOCH,
    }
}

#[test]
fn navigation_snapshot_interaction_retains_state() {
    let config = fixture();
    let session = McpSession::default();
    session
        .call_tool(
            &config,
            "navigate",
            json!({"url":"https://fixture.invalid/"}),
        )
        .unwrap();
    assert_eq!(session.tools(&config).unwrap()[0].name, "snapshot");
    let snapshot = session.call_tool(&config, "snapshot", json!({})).unwrap();
    assert_eq!(snapshot["url"], "https://fixture.invalid/");
    let clicked = session.call_tool(&config, "click", json!({})).unwrap();
    assert_eq!(clicked["url"], "https://fixture.invalid/#clicked");
}

#[test]
fn identical_config_has_independent_owners_and_explicit_close_resets() {
    let config = fixture();
    let a = McpSession::default();
    let b = McpSession::default();
    a.call_tool(&config, "navigate", json!({"url":"private-state"}))
        .unwrap();
    assert_eq!(
        b.call_tool(&config, "snapshot", json!({})).unwrap()["url"],
        "about:blank"
    );
    a.call_tool(&config, "browser_close", json!({})).unwrap();
    assert_eq!(
        a.call_tool(&config, "snapshot", json!({})).unwrap()["url"],
        "about:blank"
    );
    b.reset().unwrap();
}

#[test]
fn timeout_is_bounded_quarantined_and_never_replayed() {
    let config = fixture();
    let session = McpSession::with_timeout(Duration::from_millis(150));
    session.tools(&config).unwrap();
    let start = Instant::now();
    assert!(
        session
            .call_tool(&config, "hang", json!({}))
            .unwrap_err()
            .to_string()
            .contains("timeout")
    );
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(
        session
            .call_tool(&config, "snapshot", json!({}))
            .unwrap_err()
            .to_string()
            .contains("session lost")
    );
    session.close(&config.id).unwrap();
    assert_eq!(
        session.call_tool(&config, "snapshot", json!({})).unwrap()["url"],
        "about:blank"
    );
}

#[test]
fn cancellation_before_dispatch_and_during_io_is_truthful() {
    let config = fixture();
    let session = McpSession::default();
    assert!(
        session
            .call_tool_cancellable(&config, "navigate", json!({"url":"must-not-run"}), &|| true)
            .is_err()
    );
    assert_eq!(
        session.call_tool(&config, "snapshot", json!({})).unwrap()["url"],
        "about:blank"
    );
    let start = Instant::now();
    let error = session
        .call_tool_cancellable(&config, "hang", json!({}), &|| {
            start.elapsed() > Duration::from_millis(100)
        })
        .unwrap_err();
    assert!(error.to_string().contains("cancelled"));
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(session.call_tool(&config, "snapshot", json!({})).is_err());
}

#[test]
fn disconnect_oversize_and_rpc_error_fail_closed() {
    for tool in ["exit", "oversize", "rpc_error"] {
        let config = fixture();
        let session = McpSession::default();
        assert!(
            session.call_tool(&config, tool, json!({})).is_err(),
            "{tool}"
        );
        assert!(
            session
                .call_tool(&config, "snapshot", json!({}))
                .unwrap_err()
                .to_string()
                .contains("session lost")
        );
        assert!(
            session
                .call_tool(&config, "browser_close", json!({}))
                .is_err()
        );
        assert!(session.call_tool(&config, "snapshot", json!({})).is_ok());
    }
}

#[test]
fn changed_config_is_not_silently_rebound() {
    let mut config = fixture();
    let session = McpSession::default();
    session.tools(&config).unwrap();
    config.args.push("changed".into());
    assert!(
        session
            .tools(&config)
            .unwrap_err()
            .to_string()
            .contains("configuration changed")
    );
    session.close(&config.id).unwrap();
    session.tools(&config).unwrap();
}

#[test]
fn closing_or_dropping_owner_reaps_direct_child() {
    let config = fixture();
    for explicit in [true, false] {
        let session = McpSession::default();
        let pid = session.call_tool(&config, "snapshot", json!({})).unwrap()["pid"]
            .as_u64()
            .unwrap();
        if explicit {
            session.close(&config.id).unwrap();
        }
        drop(session);
        #[cfg(target_os = "linux")]
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    }
}

#[test]
fn session_connection_bound_refuses_overflow() {
    let session = McpSession::default();
    for i in 0..crate::session::MAX_SESSION_SERVERS {
        let mut config = fixture();
        config.id = format!("fixture_{i}");
        session.tools(&config).unwrap();
    }
    assert!(
        session
            .tools(&fixture())
            .unwrap_err()
            .to_string()
            .contains("server count")
    );
}

#[test]
fn explicit_browser_close_can_release_a_changed_configuration() {
    let mut config = fixture();
    let session = McpSession::default();
    session.tools(&config).unwrap();
    config.args.push("changed".into());
    assert!(
        session
            .call_tool(&config, "browser_close", json!({}))
            .is_err()
    );
    assert!(
        session.tools(&config).is_ok(),
        "explicit close must release old config, not strand the owner"
    );
}

#[test]
fn closing_unused_session_does_not_spawn_a_server() {
    let mut config = fixture();
    config.command = Some("/no-such-mcp-executable".into());
    assert!(
        McpSession::default()
            .call_tool(&config, "browser_close", json!({}))
            .is_ok()
    );
}
