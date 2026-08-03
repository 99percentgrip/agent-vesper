use std::fs;

use vesper_sessions::{
    DiscoveryBounds, FilesystemSessionStore, SessionSearchRequest, SessionSource,
    SessionStoreError, search_sessions,
};

#[tokio::test]
async fn searches_visible_history_and_returns_context() {
    let root = test_root("visible");
    fs::write(
        root.join("alpha.json"),
        br#"{"version":1,"messages":[{"role":"user","content":"fix the export bug"},{"role":"assistant","content":"I will inspect export"},{"role":"tool","content":"secret tool output"}]}"#,
    )
    .unwrap();
    let store = FilesystemSessionStore::new(
        root.clone(),
        SessionSource::AgentVesper,
        DiscoveryBounds::default(),
    )
    .unwrap();
    let hits = search_sessions(&store, SessionSearchRequest::new("export"))
        .await
        .unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().all(|hit| hit.snippet.contains("export")));
    assert!(
        hits.iter()
            .all(|hit| hit.context.iter().all(|message| message.role != "tool"))
    );
}

#[tokio::test]
async fn empty_query_browses_and_oversized_query_fails_closed() {
    let root = test_root("browse");
    fs::write(
        root.join("alpha.json"),
        br#"{"messages":[{"role":"user","content":"hello"}]}"#,
    )
    .unwrap();
    let store = FilesystemSessionStore::new(
        root.clone(),
        SessionSource::AgentVesper,
        DiscoveryBounds::default(),
    )
    .unwrap();
    assert_eq!(
        search_sessions(&store, SessionSearchRequest::new(""))
            .await
            .unwrap()
            .len(),
        1
    );
    let error = search_sessions(&store, SessionSearchRequest::new("x".repeat(1_025)))
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        SessionStoreError::SearchQueryTooLong { .. }
    ));
}

fn test_root(label: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "agent-vesper-search-{label}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}
