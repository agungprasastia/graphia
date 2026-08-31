use std::fs;
use tempfile::tempdir;

use graphia::mcp::error_codes;
use graphia::mcp::protocol::{JsonRpcRequest, RequestId};
use graphia::mcp::server::{IndexState, McpServer};

#[test]
fn test_mcp_rejects_unindexed_repo_without_auto_index() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();

    fs::write(root.join("a.rs"), "pub fn foo() {}").expect("write a.rs");

    let mut server = McpServer::new_with_auto_index(Some(root.to_path_buf()), false);
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(RequestId::Number(1)),
        method: "tools/call".to_string(),
        params: Some(serde_json::json!({
            "name": "graphia_search_symbol",
            "arguments": {
                "query": "foo"
            }
        })),
    };

    let resp = server.handle_request(req, RequestId::Number(1));
    let error = resp.error.expect("missing index error");
    assert_eq!(error.code, error_codes::REPOSITORY_NOT_INDEXED);
}

#[test]
fn test_mcp_classifies_and_rejects_stale_index() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    fs::write(root.join("a.rs"), "pub fn foo() {}").expect("write source");
    graphia::storage::build_or_update(root, true).expect("build index");
    fs::write(root.join("a.rs"), "pub fn foo() { let changed = 1; }").expect("change source");

    let mut server = McpServer::new_with_auto_index(Some(root.to_path_buf()), false);
    assert!(matches!(
        server.classify_index_state().expect("classify"),
        IndexState::Stale { .. }
    ));
    let response = server.handle_request(
        JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(RequestId::Number(2)),
            method: "tools/call".into(),
            params: Some(
                serde_json::json!({"name":"graphia_search_symbol","arguments":{"query":"foo"}}),
            ),
        },
        RequestId::Number(2),
    );
    assert_eq!(
        response.error.expect("stale error").code,
        error_codes::STALE_INDEX
    );
}

#[test]
fn test_mcp_auto_index_reconciles_stale_index() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    fs::write(root.join("a.rs"), "pub fn foo() {}").expect("write source");
    graphia::storage::build_or_update(root, true).expect("build index");
    fs::write(root.join("a.rs"), "pub fn bar() {}").expect("change source");
    let mut server = McpServer::new_with_auto_index(Some(root.to_path_buf()), true);
    let response = server.handle_request(
        JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(RequestId::Number(3)),
            method: "tools/call".into(),
            params: Some(
                serde_json::json!({"name":"graphia_search_symbol","arguments":{"query":"bar"}}),
            ),
        },
        RequestId::Number(3),
    );
    assert!(response.error.is_none());
}

#[test]
fn test_mcp_auto_index_successfully_builds_and_serves() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();

    fs::write(root.join("a.rs"), "pub fn foo() {}").expect("write a.rs");

    let mut server = McpServer::new_with_auto_index(Some(root.to_path_buf()), true);
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(RequestId::Number(1)),
        method: "tools/call".to_string(),
        params: Some(serde_json::json!({
            "name": "graphia_search_symbol",
            "arguments": {
                "query": "foo"
            }
        })),
    };

    let resp = server.handle_request(req, RequestId::Number(1));
    assert!(
        resp.error.is_none(),
        "auto-index should build graph without error"
    );
    assert!(resp.result.is_some());
}
