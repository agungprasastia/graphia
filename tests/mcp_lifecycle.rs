use std::fs;
use tempfile::tempdir;

use graphia::mcp::protocol::{JsonRpcRequest, RequestId};
use graphia::mcp::server::McpServer;

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
    assert!(resp.error.is_some(), "expected error for unindexed repo");
    let err_msg = resp.error.unwrap().message;
    assert!(err_msg.contains("not indexed") || err_msg.contains("auto-index"));
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
