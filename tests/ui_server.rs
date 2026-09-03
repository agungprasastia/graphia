use std::io::{Read, Write};
use std::net::TcpStream;

use graphia::graph::Graph;
use graphia::model::{
    Confidence, Edge, EdgeIdentity, EdgeKind, Language, Node, NodeId, NodeIdentity, NodeKind,
    SourceLocation, Visibility,
};
use graphia::ui::server::{UiServer, UiStats};
use tempfile::tempdir;

fn make_node(name: &str, file: &str, kind: NodeKind) -> Node {
    let loc = SourceLocation {
        file: file.to_string(),
        start_line: 1,
        start_col: 1,
        end_line: 10,
        end_col: 1,
    };
    let qualified_name = format!("{file}::{name}");
    let id = graphia::graph::stable_node_id(&NodeIdentity::new(
        Some(Language::Rust),
        file,
        kind,
        &qualified_name,
        None,
        None,
    ));
    Node {
        id,
        kind,
        name: name.to_string(),
        qualified_name,
        file: file.to_string(),
        location: loc,
        language: Some(Language::Rust),
        visibility: Visibility::Public,
        signature: None,
        container: None,
    }
}

fn make_edge(from: NodeId, to: NodeId, kind: EdgeKind) -> Edge {
    let id = graphia::graph::stable_edge_id(&EdgeIdentity::new(
        from,
        to,
        kind,
        Confidence::Extracted,
        None,
    ));
    Edge {
        id,
        from,
        to,
        kind,
        confidence: Confidence::Extracted,
        label: None,
    }
}

fn http_get(port: u16, path: &str) -> (String, String) {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).expect("Failed to connect");
    let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).expect("Failed to write");

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("Failed to read");

    let mut parts = response.splitn(2, "\r\n\r\n");
    let header = parts.next().unwrap_or("").to_string();
    let body = parts.next().unwrap_or("").to_string();
    (header, body)
}

#[test]
fn test_ui_server_lifecycle_and_endpoints() {
    let dir = tempdir().expect("tempdir failed");
    let n1 = make_node("UserService", "src/user.rs", NodeKind::Struct);
    let n2 = make_node("get_user", "src/user.rs", NodeKind::Function);
    let n3 = make_node("handle_request", "src/server.rs", NodeKind::Function);

    let e1 = make_edge(n2.id, n1.id, EdgeKind::References);
    let e2 = make_edge(n3.id, n2.id, EdgeKind::Calls);

    let graph = Graph::new(vec![n1, n2, n3], vec![e1, e2]);

    let server = UiServer::new(graph, dir.path().to_path_buf(), 0);
    let port = server.start().expect("server start failed");
    assert!(port > 0);

    // 1. Root HTML
    let (header, body) = http_get(port, "/");
    assert!(header.contains("200 OK"));
    assert!(header.contains("text/html"));
    assert!(body.contains("Graphia"));
    assert!(body.contains("graphCanvas"));

    // 2. Stats endpoint
    let (header, body) = http_get(port, "/api/stats");
    assert!(header.contains("200 OK"));
    assert!(header.contains("application/json"));
    let stats: UiStats = serde_json::from_str(&body).expect("Stats deserialize");
    assert_eq!(stats.node_count, 3);
    assert_eq!(stats.edge_count, 2);
    assert!(stats.initial_symbol.is_some());

    // 3. Search endpoint
    let (header, body) = http_get(port, "/api/search?q=user&limit=5");
    assert!(header.contains("200 OK"));
    assert!(body.contains("UserService") || body.contains("get_user"));

    // 4. Explore endpoint
    let (header, body) = http_get(port, "/api/explore?symbol=UserService&depth=1");
    assert!(header.contains("200 OK"));
    assert!(body.contains("UserService"));

    // 5. Neighborhood endpoint
    let (header, body) = http_get(port, "/api/neighborhood?symbol=UserService&depth=1");
    assert!(header.contains("200 OK"));
    assert!(body.contains("target"));

    // 6. Hotspots endpoint
    let (header, body) = http_get(port, "/api/hotspots");
    assert!(header.contains("200 OK"));
    assert!(body.starts_with('['));

    // 7. Cycles endpoint
    let (header, body) = http_get(port, "/api/cycles");
    assert!(header.contains("200 OK"));
    assert!(body.starts_with('['));

    // 8. 404 endpoint
    let (header, _) = http_get(port, "/nonexistent");
    assert!(header.contains("404 Not Found"));

    server.stop();
}
