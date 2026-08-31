use std::fs;
use std::io::Cursor;

use graphia::cli::{Cli, Commands};
use graphia::graph::Graph;
use graphia::mcp::error_codes;
use graphia::mcp::protocol::{
    CallToolResult, Content, InitializeResult, JsonRpcRequest, JsonRpcResponse, ListToolsResult,
    RequestId,
};
use graphia::mcp::{CancellationToken, call_tool_with_cancellation};
use graphia::mcp::{McpServer, get_tool_definitions};
use graphia::model::{Confidence, Edge, EdgeId, EdgeKind, Node, NodeId, NodeKind, SourceLocation};
use tempfile::tempdir;

fn build_mock_graph() -> Graph {
    let nodes = vec![
        Node {
            id: NodeId(1),
            kind: NodeKind::Function,
            name: "calculate_total".to_string(),
            qualified_name: "billing::calculate_total".to_string(),
            file: "src/billing.rs".to_string(),
            location: SourceLocation {
                file: "src/billing.rs".to_string(),
                start_line: 5,
                start_col: 1,
                end_line: 15,
                end_col: 2,
            },
            language: Some(graphia::model::Language::Rust),
            visibility: graphia::model::Visibility::Public,
            signature: None,
            container: None,
        },
        Node {
            id: NodeId(2),
            kind: NodeKind::Function,
            name: "apply_discount".to_string(),
            qualified_name: "billing::apply_discount".to_string(),
            file: "src/billing.rs".to_string(),
            location: SourceLocation {
                file: "src/billing.rs".to_string(),
                start_line: 17,
                start_col: 1,
                end_line: 25,
                end_col: 2,
            },
            language: Some(graphia::model::Language::Rust),
            visibility: graphia::model::Visibility::Public,
            signature: None,
            container: None,
        },
        Node {
            id: NodeId(3),
            kind: NodeKind::Function,
            name: "checkout".to_string(),
            qualified_name: "order::checkout".to_string(),
            file: "src/order.rs".to_string(),
            location: SourceLocation {
                file: "src/order.rs".to_string(),
                start_line: 10,
                start_col: 1,
                end_line: 30,
                end_col: 2,
            },
            language: Some(graphia::model::Language::Rust),
            visibility: graphia::model::Visibility::Public,
            signature: None,
            container: None,
        },
        Node {
            id: NodeId(4),
            kind: NodeKind::Struct,
            name: "Order".to_string(),
            qualified_name: "order::Order".to_string(),
            file: "src/order.rs".to_string(),
            location: SourceLocation {
                file: "src/order.rs".to_string(),
                start_line: 1,
                start_col: 1,
                end_line: 8,
                end_col: 2,
            },
            language: Some(graphia::model::Language::Rust),
            visibility: graphia::model::Visibility::Public,
            signature: None,
            container: None,
        },
        Node {
            id: NodeId(5),
            kind: NodeKind::Function,
            name: "test_calculate_total".to_string(),
            qualified_name: "order_test::test_calculate_total".to_string(),
            file: "tests/order_test.rs".to_string(),
            location: SourceLocation {
                file: "tests/order_test.rs".to_string(),
                start_line: 5,
                start_col: 1,
                end_line: 12,
                end_col: 2,
            },
            language: Some(graphia::model::Language::Rust),
            visibility: graphia::model::Visibility::Public,
            signature: None,
            container: None,
        },
    ];

    let edges = vec![
        // checkout -> calculate_total
        Edge {
            id: EdgeId(1),
            kind: EdgeKind::Calls,
            from: NodeId(3),
            to: NodeId(1),
            confidence: Confidence::Extracted,
            label: None,
        },
        // calculate_total -> apply_discount
        Edge {
            id: EdgeId(2),
            kind: EdgeKind::Calls,
            from: NodeId(1),
            to: NodeId(2),
            confidence: Confidence::Extracted,
            label: None,
        },
        // test_calculate_total -> calculate_total
        Edge {
            id: EdgeId(3),
            kind: EdgeKind::Calls,
            from: NodeId(5),
            to: NodeId(1),
            confidence: Confidence::Extracted,
            label: None,
        },
    ];

    Graph::new(nodes, edges)
}

#[test]
fn test_mcp_tool_definitions_count_and_schema() {
    let tools = get_tool_definitions();
    assert_eq!(tools.len(), 11);

    let expected_names = [
        "graphia_search_symbol",
        "graphia_get_symbol",
        "graphia_find_callers",
        "graphia_find_callees",
        "graphia_find_references",
        "graphia_dependency_path",
        "graphia_neighborhood",
        "graphia_impact",
        "graphia_find_tests",
        "graphia_architecture",
        "graphia_context",
    ];

    for name in &expected_names {
        let tool = tools.iter().find(|t| t.name == *name);
        assert!(tool.is_some(), "Tool {name} must be defined");
        let t = tool.unwrap();
        assert!(t.description.is_some());
        assert!(t.input_schema.is_object());
    }
}

#[test]
fn test_mcp_initialize_and_notification() {
    let mut server = McpServer::new(None).with_graph(build_mock_graph());

    let init_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(RequestId::Number(100)),
        method: "initialize".to_string(),
        params: Some(serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "1.0.0"
            }
        })),
    };

    let resp = server.handle_request(init_req, RequestId::Number(100));
    assert_eq!(resp.id, RequestId::Number(100));
    assert!(resp.error.is_none());

    let init_res: InitializeResult =
        serde_json::from_value(resp.result.unwrap()).expect("parse init result");
    assert_eq!(init_res.protocol_version, "2024-11-05");
    assert_eq!(init_res.server_info.name, "graphia-mcp");
    assert!(init_res.capabilities.tools.is_some());

    // Send initialized notification
    let notif = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: None,
        method: "notifications/initialized".to_string(),
        params: None,
    };
    assert!(server.handle_notification(&notif).is_ok());
}

#[test]
fn test_mcp_tools_list() {
    let mut server = McpServer::new(None).with_graph(build_mock_graph());

    let list_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(RequestId::String("req-tools".to_string())),
        method: "tools/list".to_string(),
        params: None,
    };

    let resp = server.handle_request(list_req, RequestId::String("req-tools".to_string()));
    assert_eq!(resp.id, RequestId::String("req-tools".to_string()));
    assert!(resp.error.is_none());

    let list_res: ListToolsResult =
        serde_json::from_value(resp.result.unwrap()).expect("parse list tools result");
    assert_eq!(list_res.tools.len(), 11);
}

#[test]
fn test_mcp_cancelled_tool_returns_cancelled_error() {
    let token = CancellationToken::new();
    token.cancel();
    let graph = build_mock_graph();
    let args = serde_json::json!({"from":"checkout","to":"apply_discount"});
    let args = args.as_object();
    let result = call_tool_with_cancellation(&graph, None, "graphia_dependency_path", args, &token);
    assert!(matches!(result, Err(graphia::mcp::McpError::Cancelled)));
}

#[test]
fn test_mcp_malformed_json_is_reported_and_server_continues() {
    let input = "{ malformed\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n";
    let mut output = Vec::new();
    McpServer::new(None)
        .run_stream(Cursor::new(input.as_bytes()), &mut output)
        .expect("server resilience");
    let output = String::from_utf8(output).expect("utf8");
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 2);
    let parse: JsonRpcResponse = serde_json::from_str(lines[0]).expect("parse response");
    assert_eq!(
        parse.error.expect("parse error").code,
        error_codes::PARSE_ERROR
    );
    let ping: JsonRpcResponse = serde_json::from_str(lines[1]).expect("ping response");
    assert!(ping.error.is_none());
}

#[test]
fn test_mcp_all_11_tools_execution() {
    let graph = build_mock_graph();
    let repo_dir = tempdir().expect("tempdir");

    // Write dummy files for context engine slicing
    fs::create_dir_all(repo_dir.path().join("src")).unwrap();
    fs::write(
        repo_dir.path().join("src/billing.rs"),
        "// billing\npub fn calculate_total() {\n    apply_discount();\n}\npub fn apply_discount() {}\n",
    )
    .unwrap();

    let mut server = McpServer::new(Some(repo_dir.path().to_path_buf())).with_graph(graph.clone());

    // 1. graphia_search_symbol
    let resp = server.handle_request(
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(1)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "graphia_search_symbol",
                "arguments": { "query": "calculate" }
            })),
        },
        RequestId::Number(1),
    );
    let val: CallToolResult = serde_json::from_value(resp.result.unwrap()).unwrap();
    assert_eq!(val.is_error, None);
    let Content::Text { text } = &val.content[0];
    assert!(text.contains("calculate_total"));

    // 2. graphia_get_symbol
    let resp = server.handle_request(
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(2)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "graphia_get_symbol",
                "arguments": { "symbol": "calculate_total" }
            })),
        },
        RequestId::Number(2),
    );
    let val: CallToolResult = serde_json::from_value(resp.result.unwrap()).unwrap();
    let Content::Text { text } = &val.content[0];
    assert!(text.contains("billing::calculate_total"));

    // 3. graphia_find_callers
    let resp = server.handle_request(
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(3)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "graphia_find_callers",
                "arguments": { "symbol": "calculate_total" }
            })),
        },
        RequestId::Number(3),
    );
    let val: CallToolResult = serde_json::from_value(resp.result.unwrap()).unwrap();
    let Content::Text { text } = &val.content[0];
    assert!(text.contains("checkout"));

    // 4. graphia_find_callees
    let resp = server.handle_request(
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(4)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "graphia_find_callees",
                "arguments": { "symbol": "calculate_total" }
            })),
        },
        RequestId::Number(4),
    );
    let val: CallToolResult = serde_json::from_value(resp.result.unwrap()).unwrap();
    let Content::Text { text } = &val.content[0];
    assert!(text.contains("apply_discount"));

    // 5. graphia_find_references
    let resp = server.handle_request(
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(5)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "graphia_find_references",
                "arguments": { "symbol": "calculate_total" }
            })),
        },
        RequestId::Number(5),
    );
    let val: CallToolResult = serde_json::from_value(resp.result.unwrap()).unwrap();
    let Content::Text { text } = &val.content[0];
    assert!(text.contains("calls"));

    // 6. graphia_dependency_path
    let resp = server.handle_request(
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(6)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "graphia_dependency_path",
                "arguments": {
                    "from": "checkout",
                    "to": "apply_discount"
                }
            })),
        },
        RequestId::Number(6),
    );
    let val: CallToolResult = serde_json::from_value(resp.result.unwrap()).unwrap();
    let Content::Text { text } = &val.content[0];
    assert!(text.contains("calculate_total"));

    // 7. graphia_neighborhood
    let resp = server.handle_request(
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(7)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "graphia_neighborhood",
                "arguments": { "symbol": "calculate_total" }
            })),
        },
        RequestId::Number(7),
    );
    let val: CallToolResult = serde_json::from_value(resp.result.unwrap()).unwrap();
    let Content::Text { text } = &val.content[0];
    assert!(text.contains("callees"));

    // 8. graphia_impact
    let resp = server.handle_request(
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(8)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "graphia_impact",
                "arguments": { "symbol": "apply_discount" }
            })),
        },
        RequestId::Number(8),
    );
    let val: CallToolResult = serde_json::from_value(resp.result.unwrap()).unwrap();
    let Content::Text { text } = &val.content[0];
    assert!(text.contains("calculate_total"));

    // 9. graphia_find_tests
    let resp = server.handle_request(
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(9)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "graphia_find_tests",
                "arguments": { "symbol": "calculate_total" }
            })),
        },
        RequestId::Number(9),
    );
    let val: CallToolResult = serde_json::from_value(resp.result.unwrap()).unwrap();
    let Content::Text { text } = &val.content[0];
    assert!(text.contains("test_calculate_total"));

    // 10. graphia_architecture
    let resp = server.handle_request(
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(10)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "graphia_architecture"
            })),
        },
        RequestId::Number(10),
    );
    let val: CallToolResult = serde_json::from_value(resp.result.unwrap()).unwrap();
    let Content::Text { text } = &val.content[0];
    assert!(text.contains("total_nodes"));

    // 11. graphia_context
    let resp = server.handle_request(
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(11)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "graphia_context",
                "arguments": {
                    "symbol": "calculate_total",
                    "token_budget": 500
                }
            })),
        },
        RequestId::Number(11),
    );
    let val: CallToolResult = serde_json::from_value(resp.result.unwrap()).unwrap();
    let Content::Text { text } = &val.content[0];
    assert!(text.contains("calculate_total"));
}

#[test]
fn test_stdio_isolation_and_stream_roundtrip() {
    let graph = build_mock_graph();
    let input_lines = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"graphia_search_symbol","arguments":{"query":"checkout"}}}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"ping"}"#,
    ];
    let input_payload = input_lines.join("\n") + "\n";

    let mut output_bytes = Vec::new();
    let mut server = McpServer::new(None).with_graph(graph);

    server
        .run_stream(Cursor::new(input_payload.as_bytes()), &mut output_bytes)
        .expect("run stream");

    let stdout_content = String::from_utf8(output_bytes).expect("utf-8 output");
    let response_lines: Vec<&str> = stdout_content.lines().collect();
    assert_eq!(response_lines.len(), 4);

    for (idx, line) in response_lines.iter().enumerate() {
        let resp: JsonRpcResponse =
            serde_json::from_str(line).expect("each line must be valid json-rpc");
        assert_eq!(resp.id, RequestId::Number((idx + 1) as i64));
        assert!(resp.error.is_none());
    }
}

#[test]
fn test_stdio_cancel_notification_does_not_kill_server() {
    let input = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"graphia_dependency_path","arguments":{"from":"checkout","to":"apply_discount","max_depth":10000}}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":1}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
        "\n",
    );
    let mut output = Vec::new();
    McpServer::new(None)
        .with_graph(build_mock_graph())
        .run_stream(Cursor::new(input.as_bytes()), &mut output)
        .expect("stdio cancellation");
    let output = String::from_utf8(output).expect("utf8 output");
    let responses = output
        .lines()
        .map(|line| serde_json::from_str::<JsonRpcResponse>(line).expect("json response"))
        .collect::<Vec<_>>();
    assert!(
        responses
            .iter()
            .any(|response| response.id == RequestId::Number(2))
    );
}

#[test]
fn test_path_traversal_sandboxing() {
    let mut server = McpServer::new(None).with_graph(build_mock_graph());

    let disallowed_paths = [
        "../../secrets.txt",
        "/etc/passwd",
        "C:\\Windows\\System32\\cmd.exe",
        "foo/../../bar",
    ];

    for (idx, bad_path) in disallowed_paths.iter().enumerate() {
        let resp = server.handle_request(
            JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: Some(RequestId::Number(idx as i64 + 1)),
                method: "tools/call".to_string(),
                params: Some(serde_json::json!({
                    "name": "graphia_find_tests",
                    "arguments": { "file": bad_path }
                })),
            },
            RequestId::Number(idx as i64 + 1),
        );

        assert!(resp.result.is_none());
        let err = resp.error.expect("error expected");
        assert_eq!(err.code, error_codes::PATH_TRAVERSAL_DETECTED);
        assert!(err.message.contains("Path traversal"));
    }
}

#[test]
fn test_unknown_method_and_invalid_params_handling() {
    let mut server = McpServer::new(None).with_graph(build_mock_graph());

    // Unknown method
    let resp = server.handle_request(
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(1)),
            method: "custom/unknown".to_string(),
            params: None,
        },
        RequestId::Number(1),
    );
    assert_eq!(resp.error.unwrap().code, error_codes::METHOD_NOT_FOUND);

    // Missing arguments for required parameter
    let resp = server.handle_request(
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(2)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "graphia_search_symbol",
                "arguments": {}
            })),
        },
        RequestId::Number(2),
    );
    assert_eq!(resp.error.unwrap().code, error_codes::INVALID_PARAMS);
}

#[test]
fn test_cli_mcp_invocation() {
    let repo = tempdir().expect("tempdir");
    let cli = Cli {
        command: Commands::Mcp {
            repo: Some(repo.path().to_path_buf()),
            auto_index: true,
        },
    };
    assert!(matches!(cli.command, Commands::Mcp { .. }));
}
