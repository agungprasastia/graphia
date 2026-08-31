use std::fs;
use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

use graphia::analysis::advanced::{
    ArchitectureRulesConfig, GitCommitRecord, check_architecture_boundaries,
    compute_change_coupling, diff_graphs, find_source_sink_flows,
};
use graphia::context::{BudgetValueType, ContextRequest, generate_context};
use graphia::daemon::LiveStateManager;
use graphia::graph::build_graph;
use graphia::intelligence::{
    NeighborhoodOptions, SearchOptions, analyze_impact, get_neighborhood, search_graph,
};
use graphia::mcp::{CallToolParams, CallToolResult, JsonRpcRequest, RequestId, call_tool};
use graphia::model::Language;
use graphia::parser::parse_bytes;
use graphia::query::{QueryIndex, TraversalLimits};
use graphia::scan::scan_repo;
use graphia::storage::{load_graph_binary, save_graph_binary, save_graph_json};
use tempfile::TempDir;

const ITERATIONS: usize = 1_000;

#[derive(Clone, Copy)]
struct Dataset {
    name: &'static str,
    files: usize,
}

const DATASETS: [Dataset; 3] = [
    Dataset {
        name: "small",
        files: 3,
    },
    Dataset {
        name: "medium",
        files: 12,
    },
    Dataset {
        name: "large",
        files: 48,
    },
];

fn main() {
    println!(
        "dataset,files,source_bytes,nodes,edges,stage,observed_ns,index_bytes,peak_rss,graphify"
    );
    for dataset in DATASETS {
        let fixture = create_fixture(dataset);
        measure_dataset(dataset, &fixture);
    }
}

fn create_fixture(dataset: Dataset) -> TempDir {
    let dir = tempfile::tempdir().expect("create benchmark directory");
    for index in 0..dataset.files {
        let path = dir.path().join(format!("src/module_{index:03}.rs"));
        fs::create_dir_all(path.parent().expect("fixture parent")).expect("create fixture parent");
        fs::write(&path, source_for(index, dataset.files)).expect("write benchmark source");
    }
    dir
}

fn source_for(index: usize, files: usize) -> String {
    let next = (index + 1) % files;
    format!(
        "pub struct Type{index};\npub fn function_{index}() -> usize {{ helper_{next}() }}\npub fn helper_{index}() -> usize {{ {index} }}\n"
    )
}

fn measure_dataset(dataset: Dataset, fixture: &TempDir) {
    let scan_start = Instant::now();
    let scanned = scan_repo(fixture.path()).expect("scan benchmark fixture");
    assert_eq!(scanned.len(), dataset.files, "fixture scan count changed");
    let scan = scan_start.elapsed().as_nanos();
    let source_bytes = scanned
        .iter()
        .map(|file| {
            fs::metadata(&file.absolute_path)
                .expect("fixture metadata")
                .len()
        })
        .sum::<u64>();
    let parse_start = Instant::now();
    let parsed = scanned
        .iter()
        .map(|file| {
            let language = file.language.expect("Rust fixture language");
            let source = fs::read(&file.absolute_path).expect("read benchmark source");
            (
                file.relative_path.clone(),
                Some(language),
                parse_bytes(&file.relative_path, language, &source)
                    .expect("parse benchmark source"),
            )
        })
        .collect::<Vec<_>>();
    let parse = parse_start.elapsed().as_nanos();
    let graph_start = Instant::now();
    let mut graph = build_graph(parsed.clone());
    graph.canonicalize().expect("canonicalize benchmark graph");
    let graph_build = graph_start.elapsed().as_nanos();
    let query = QueryIndex::new(&graph);
    let query_start = Instant::now();
    for _ in 0..ITERATIONS {
        black_box(query.find(&graph, "function_0"));
    }
    let query_time = query_start.elapsed().as_nanos() / ITERATIONS as u128;
    let bfs_start = Instant::now();
    if let (Some(from), Some(to)) = (graph.nodes.first(), graph.nodes.last()) {
        for _ in 0..ITERATIONS {
            let result = query.shortest_path(
                from.id,
                to.id,
                TraversalLimits::new(100, graph.nodes.len().saturating_add(1)),
            );
            let _ = black_box(result);
        }
    }
    let bfs = bfs_start.elapsed().as_nanos() / ITERATIONS as u128;
    let output = fixture.path().join("graph.json");
    let json_start = Instant::now();
    save_graph_json(&graph, &output).expect("write benchmark JSON");
    let json = json_start.elapsed().as_nanos();
    let binary = fixture.path().join("index.bin");
    let binary_start = Instant::now();
    save_graph_binary(&graph, &binary).expect("write benchmark binary");
    let binary_write = binary_start.elapsed().as_nanos();
    let binary_load_start = Instant::now();
    let loaded = load_graph_binary(&binary).expect("load benchmark binary");
    assert_eq!(
        loaded.nodes, graph.nodes,
        "loaded binary nodes changed source graph"
    );
    assert_eq!(
        loaded.edges, graph.edges,
        "loaded binary edges changed source graph"
    );
    let binary_load = binary_load_start.elapsed().as_nanos();
    let index_bytes = fs::metadata(&binary)
        .expect("benchmark index metadata")
        .len();
    assert_deterministic(fixture.path(), &parsed, &graph, &binary);
    graphia::storage::build_or_update(fixture.path(), true).expect("establish benchmark cache");
    fs::write(
        fixture.path().join("src/module_000.rs"),
        source_for(dataset.files + 1, dataset.files + 1),
    )
    .expect("modify benchmark source");
    fs::write(
        fixture.path().join("src/module_added.rs"),
        source_for(dataset.files + 1, dataset.files + 1),
    )
    .expect("add benchmark source");
    fs::remove_file(fixture.path().join("src/module_001.rs")).expect("delete benchmark source");
    let incremental_start = Instant::now();
    let (updated, changes) =
        graphia::storage::build_or_update(fixture.path(), false).expect("incremental benchmark");
    let incremental = incremental_start.elapsed().as_nanos();
    assert!(
        changes
            .iter()
            .any(|change| change.path == "src/module_added.rs")
    );
    assert!(
        changes
            .iter()
            .any(|change| change.path == "src/module_001.rs")
    );
    assert!(
        changes
            .iter()
            .any(|change| change.path == "src/module_000.rs")
    );
    assert_eq!(updated.node_count(), dataset.files * 4);
    let clean =
        graphia::storage::build_graph_from_repo(fixture.path()).expect("clean rebuild benchmark");
    assert_eq!(
        updated, clean,
        "incremental graph differs from clean rebuild"
    );

    let search_options = SearchOptions {
        query: "function_0".to_string(),
        kind_filter: None,
        file_filter: None,
        limit: Some(10),
    };
    let intel_search_start = Instant::now();
    for _ in 0..ITERATIONS {
        let results = search_graph(&graph, &search_options);
        let _ = black_box(results);
    }
    let intel_search = intel_search_start.elapsed().as_nanos() / ITERATIONS as u128;

    let target_symbol = graph
        .nodes
        .first()
        .map(|n| n.name.as_str())
        .unwrap_or("function_0");
    let intel_neighborhood_start = Instant::now();
    let n_opts = NeighborhoodOptions {
        target: target_symbol.to_string(),
        depth: 2,
        limit: 20,
    };
    for _ in 0..ITERATIONS {
        let neighborhood = get_neighborhood(&graph, &n_opts);
        let _ = black_box(neighborhood);
    }
    let intel_neighborhood = intel_neighborhood_start.elapsed().as_nanos() / ITERATIONS as u128;

    let intel_impact_start = Instant::now();
    for _ in 0..ITERATIONS {
        let impact = analyze_impact(&graph, target_symbol, 3);
        let _ = black_box(impact);
    }
    let intel_impact = intel_impact_start.elapsed().as_nanos() / ITERATIONS as u128;

    let ctx_request = ContextRequest {
        symbol: Some("function_0".to_string()),
        file: None,
        query: None,
        changed: false,
        budget: Some(2000),
        budget_type: BudgetValueType::ApproxTokens,
        max_depth: 3,
        max_candidates: 50,
    };
    let context_gen_start = Instant::now();
    for _ in 0..ITERATIONS {
        let bundle = generate_context(&graph, &ctx_request, Some(fixture.path()));
        let _ = black_box(bundle);
    }
    let context_gen = context_gen_start.elapsed().as_nanos() / ITERATIONS as u128;

    let mcp_call_params = CallToolParams {
        name: "graphia_search_symbol".to_string(),
        arguments: Some(
            serde_json::json!({
                "query": "function_0",
                "limit": 10
            })
            .as_object()
            .cloned()
            .unwrap(),
        ),
    };
    let mcp_exec_start = Instant::now();
    for _ in 0..ITERATIONS {
        let res = call_tool(
            &graph,
            Some(fixture.path()),
            &mcp_call_params.name,
            mcp_call_params.arguments.as_ref(),
        );
        let _ = black_box(res);
    }
    let mcp_tool_call = mcp_exec_start.elapsed().as_nanos() / ITERATIONS as u128;

    let sample_tool_res = CallToolResult::text("benchmark payload text content for serialization");
    let json_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(RequestId::Number(42)),
        method: "tools/call".to_string(),
        params: Some(serde_json::to_value(&mcp_call_params).unwrap()),
    };
    let mcp_serde_start = Instant::now();
    for _ in 0..ITERATIONS {
        let req_json = serde_json::to_string(&json_req).expect("serialize request");
        let _deser_req: JsonRpcRequest =
            serde_json::from_str(&req_json).expect("deserialize request");
        let res_json = serde_json::to_string(&sample_tool_res).expect("serialize response");
        let _deser_res: CallToolResult =
            serde_json::from_str(&res_json).expect("deserialize response");
        let _ = black_box((req_json, res_json));
    }
    let mcp_serde = mcp_serde_start.elapsed().as_nanos() / ITERATIONS as u128;

    let daemon_state = LiveStateManager::initialize(fixture.path()).expect("init daemon state");
    let daemon_update_start = Instant::now();
    for _ in 0..ITERATIONS {
        let updated_gen = daemon_state.update_graph(graph.clone());
        let _ = black_box(updated_gen);
    }
    let daemon_update = daemon_update_start.elapsed().as_nanos() / ITERATIONS as u128;

    let flow_query_start = Instant::now();
    for _ in 0..ITERATIONS {
        let flow_report = find_source_sink_flows(&graph, "function_0", "helper_0", Some(5));
        let _ = black_box(flow_report);
    }
    let flow_query = flow_query_start.elapsed().as_nanos() / ITERATIONS as u128;

    let arch_config = ArchitectureRulesConfig::default();
    let boundary_check_start = Instant::now();
    for _ in 0..ITERATIONS {
        let bound_report = check_architecture_boundaries(&graph, &arch_config);
        let _ = black_box(bound_report);
    }
    let boundary_check = boundary_check_start.elapsed().as_nanos() / ITERATIONS as u128;

    let diff_start = Instant::now();
    for _ in 0..ITERATIONS {
        let diff_report = diff_graphs(&graph, &updated);
        let _ = black_box(diff_report);
    }
    let graph_diff = diff_start.elapsed().as_nanos() / ITERATIONS as u128;

    let dummy_commits = vec![
        GitCommitRecord {
            commit_hash: "commit1".to_string(),
            author: "dev".to_string(),
            timestamp: 1000,
            files_changed: vec![
                "src/module_000.rs".to_string(),
                "src/module_001.rs".to_string(),
            ],
        },
        GitCommitRecord {
            commit_hash: "commit2".to_string(),
            author: "dev".to_string(),
            timestamp: 2000,
            files_changed: vec![
                "src/module_000.rs".to_string(),
                "src/module_002.rs".to_string(),
            ],
        },
    ];
    let change_coupling_start = Instant::now();
    for _ in 0..ITERATIONS {
        let coupling_report = compute_change_coupling(&dummy_commits, Some(0.1));
        let _ = black_box(coupling_report);
    }
    let change_coupling = change_coupling_start.elapsed().as_nanos() / ITERATIONS as u128;

    let values = [
        ("scan", scan),
        ("parse_extract", parse),
        ("graph_resolution", graph_build),
        ("query_exact", query_time),
        ("query_bfs", bfs),
        ("json", json),
        ("binary_write", binary_write),
        ("binary_load", binary_load),
        ("incremental", incremental),
        ("intel_search", intel_search),
        ("intel_neighborhood", intel_neighborhood),
        ("intel_impact", intel_impact),
        ("context_generation", context_gen),
        ("mcp_tool_call", mcp_tool_call),
        ("mcp_serialization", mcp_serde),
        ("snapshot_publish", daemon_update),
        ("flow_query", flow_query),
        ("boundary_check", boundary_check),
        ("graph_diff", graph_diff),
        ("change_coupling", change_coupling),
    ];
    for (stage, duration) in values {
        println!(
            "{},{},{},{},{},{},{},{},unavailable,unavailable",
            dataset.name,
            dataset.files,
            source_bytes,
            graph.node_count(),
            graph.edge_count(),
            stage,
            duration,
            index_bytes
        );
    }
}

fn assert_deterministic(
    root: &Path,
    parsed: &[(String, Option<Language>, graphia::parser::ParsedFile)],
    graph: &graphia::graph::Graph,
    binary: &Path,
) {
    let repeat = build_graph(parsed.to_vec());
    let mut repeat = repeat;
    repeat.canonicalize().expect("canonicalize repeat graph");
    assert_eq!(graph, &repeat, "repeat build changed graph");
    let repeat_json = root.join("graph-repeat.json");
    save_graph_json(&repeat, &repeat_json).expect("write repeat JSON");
    assert_eq!(
        fs::read(root.join("graph.json")).expect("read benchmark JSON"),
        fs::read(repeat_json).expect("read repeat JSON"),
        "repeat build changed JSON output"
    );
    let repeat_binary = root.join("index-repeat.bin");
    save_graph_binary(&repeat, &repeat_binary).expect("write repeat binary");
    assert_eq!(
        fs::read(binary).expect("read benchmark binary"),
        fs::read(repeat_binary).expect("read repeat binary"),
        "repeat build changed binary output"
    );
}
