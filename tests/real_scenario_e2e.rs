use std::fs;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use graphia::daemon::{DaemonConfig, DaemonServer, GraphGeneration};
use graphia::mcp::error_codes;
use graphia::mcp::protocol::{JsonRpcRequest, RequestId};
use graphia::mcp::server::McpServer;
use tempfile::tempdir;

fn wait_for_status<F>(repo: &std::path::Path, predicate: F) -> graphia::daemon::DaemonStatusInfo
where
    F: Fn(&graphia::daemon::DaemonStatusInfo) -> bool,
{
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(Some(status)) = DaemonServer::read_daemon_status(repo)
            && predicate(&status)
        {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "daemon status condition timed out"
        );
        thread::sleep(Duration::from_millis(25));
    }
}

fn search_request(query: &str, id: i64) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".into(),
        id: Some(RequestId::Number(id)),
        method: "tools/call".into(),
        params: Some(serde_json::json!({
            "name": "graphia_search_symbol",
            "arguments": { "query": query }
        })),
    }
}

#[test]
fn real_runtime_sequence_covers_build_daemon_incremental_mcp_flow_and_shutdown() {
    // Given: a real multi-file repository with a connected call/data-flow path.
    let fixture = tempdir().expect("fixture directory");
    let repo = fixture.path();
    fs::write(
        repo.join("pipeline.rs"),
        "pub fn request() { transform(); }\npub fn transform() { execute(); }\npub fn execute() {}\n",
    )
    .expect("write pipeline fixture");
    fs::write(repo.join("unrelated.rs"), "pub fn unrelated() {}\n")
        .expect("write unrelated fixture");
    fs::write(repo.join("nested.rs"), "pub fn nested() {}\n").expect("write nested fixture");

    // Step 1: graphia build creates canonical binary index.
    let build = Command::new(env!("CARGO_BIN_EXE_graphia"))
        .args(["build"])
        .arg(repo)
        .output()
        .expect("run graphia build");
    assert!(build.status.success(), "build failed: {:?}", build);
    assert!(repo.join(".graphia/index.bin").is_file());
    let initial_graph = graphia::storage::load_graph_binary(&repo.join(".graphia/index.bin"))
        .expect("load initial index");

    // Step 2: start daemon on fixture directory.
    let mut daemon = DaemonServer::new(DaemonConfig {
        repo_root: repo.to_path_buf(),
        debounce_duration: Duration::from_millis(40),
        queue_capacity: 100,
        persistence_interval: Duration::from_millis(40),
    })
    .expect("create daemon");
    let shutdown = daemon.shutdown_signal();
    let live_state = daemon.state_manager();
    let daemon_thread = thread::spawn(move || daemon.run());
    wait_for_status(repo, |status| {
        status.running && status.generation == GraphGeneration(1)
    });

    // Step 3: modify exactly one source file.
    fs::write(
        repo.join("unrelated.rs"),
        "pub fn unrelated() {}\npub fn changed() {}\n",
    )
    .expect("modify one source file");

    // Steps 4-5: generation advances once and exactly one file is reparsed.
    let updated = wait_for_status(repo, |status| {
        status.running && status.generation == GraphGeneration(2) && status.files_reparsed == 1
    });
    assert_eq!(updated.generation, GraphGeneration(2));
    assert_eq!(updated.files_reparsed, 1);
    let incremental_graph = live_state.read_snapshot().graph.as_ref().clone();

    shutdown.trigger();
    daemon_thread
        .join()
        .expect("join daemon")
        .expect("daemon shutdown");
    assert!(
        DaemonServer::read_daemon_status(repo)
            .expect("read stopped status")
            .is_none()
    );

    // Step 7: query MCP tools against current persisted index.
    let mut mcp = McpServer::new_with_auto_index(Some(repo.to_path_buf()), false)
        .with_graph(incremental_graph.clone());
    let response = mcp.handle_request(search_request("changed", 1), RequestId::Number(1));
    assert!(
        response.error.is_none(),
        "MCP query failed: {:?}",
        response.error
    );
    assert!(response.result.is_some());

    // Step 6: incremental graph equals clean rebuild graph.
    let (clean_graph, _) = graphia::storage::build_or_update(repo, true).expect("clean rebuild");
    assert_eq!(incremental_graph, clean_graph);
    assert_ne!(initial_graph, incremental_graph);

    // Step 8: make external change, then verify stale failure and recovery.
    fs::write(
        repo.join("unrelated.rs"),
        "pub fn unrelated() {}\npub fn changed() {}\npub fn external() {}\n",
    )
    .expect("external modification");
    let mut stale_mcp = McpServer::new_with_auto_index(Some(repo.to_path_buf()), false);
    let stale = stale_mcp.handle_request(search_request("external", 2), RequestId::Number(2));
    assert_eq!(
        stale.error.expect("stale MCP error").code,
        error_codes::STALE_INDEX
    );

    let mut recovering_mcp = McpServer::new_with_auto_index(Some(repo.to_path_buf()), true);
    let recovered =
        recovering_mcp.handle_request(search_request("external", 3), RequestId::Number(3));
    assert!(
        recovered.error.is_none(),
        "auto-index failed: {:?}",
        recovered.error
    );
    assert!(recovered.result.is_some());

    // Step 9: graphia flow reports potential source-to-sink paths.
    let flow = Command::new(env!("CARGO_BIN_EXE_graphia"))
        .args([
            "flow", "--source", "request", "--sink", "execute", "--format", "json",
        ])
        .arg(repo)
        .output()
        .expect("run graphia flow");
    assert!(flow.status.success(), "flow failed: {:?}", flow);
    let report: serde_json::Value = serde_json::from_slice(&flow.stdout).expect("parse flow JSON");
    assert!(
        report["paths_found"]
            .as_u64()
            .is_some_and(|count| count >= 1)
    );

    // Step 10: graceful shutdown completed above through daemon shutdown signal.
    assert!(!repo.join(".graphia/daemon.json").exists());
}
