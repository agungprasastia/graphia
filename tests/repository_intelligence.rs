use graphia::cli::{Cli, CliFormat, CliNodeKind, Commands, run};
use graphia::graph::Graph;
use graphia::intelligence::{
    EntrypointKind, ImpactKind, NeighborhoodOptions, SearchOptions, analyze_impact,
    detect_entrypoints, discover_tests, get_architecture_overview, get_neighborhood,
    map_source_to_tests, search_graph,
};
use graphia::model::{
    Confidence, Edge, EdgeKind, Language, Node, NodeId, NodeKind, SourceLocation,
};
use graphia::storage;
use tempfile::tempdir;

fn make_node(name: &str, file: &str, kind: NodeKind, lang: Option<Language>) -> Node {
    let loc = SourceLocation {
        file: file.to_string(),
        start_line: 1,
        start_col: 1,
        end_line: 1,
        end_col: 1,
    };
    let qualified_name = format!("{file}::{name}");
    let id = graphia::graph::stable_node_id(&graphia::model::NodeIdentity::new(
        file,
        kind,
        &qualified_name,
        &loc,
    ));
    Node {
        id,
        kind,
        name: name.to_string(),
        qualified_name,
        file: file.to_string(),
        location: loc,
        language: lang,
    }
}

fn make_edge(from: NodeId, to: NodeId, kind: EdgeKind) -> Edge {
    let id = graphia::graph::stable_edge_id(&graphia::model::EdgeIdentity::new(
        from,
        to,
        kind,
        Confidence::Extracted,
        None,
    ));
    Edge {
        id,
        kind,
        from,
        to,
        confidence: Confidence::Extracted,
        label: None,
    }
}

#[test]
fn test_search_relevance_ranking_and_filters() {
    let node1 = make_node(
        "process_data",
        "src/pipeline.rs",
        NodeKind::Function,
        Some(Language::Rust),
    );
    let node2 = make_node(
        "process_data_fast",
        "src/pipeline.rs",
        NodeKind::Function,
        Some(Language::Rust),
    );
    let node3 = make_node(
        "do_process_data",
        "src/pipeline.rs",
        NodeKind::Function,
        Some(Language::Rust),
    );
    let node4 = make_node(
        "process_data",
        "src/other.py",
        NodeKind::Function,
        Some(Language::Python),
    );

    let nodes = vec![node1.clone(), node2.clone(), node3.clone(), node4.clone()];
    let edges = vec![
        make_edge(node2.id, node1.id, EdgeKind::Calls), // B calls A (A gets centrality boost)
        make_edge(node3.id, node1.id, EdgeKind::Calls), // C calls A
    ];
    let graph = Graph::new(nodes, edges);

    // Search for "process_data"
    let results = search_graph(
        &graph,
        &SearchOptions {
            query: "process_data".to_string(),
            kind_filter: None,
            file_filter: None,
            limit: None,
        },
    );

    assert_eq!(results.len(), 4);
    // Exact matches should rank first
    assert!(results[0].node.name == "process_data");
    assert!(results[1].node.name == "process_data");
    // Node 1 has higher centrality than Node 4
    assert_eq!(results[0].node.id, node1.id);

    // Prefix should rank above substring
    let prefix_idx = results
        .iter()
        .position(|r| r.node.name == "process_data_fast")
        .unwrap();
    let substr_idx = results
        .iter()
        .position(|r| r.node.name == "do_process_data")
        .unwrap();
    assert!(prefix_idx < substr_idx);

    // Test with kind filter
    let func_results = search_graph(
        &graph,
        &SearchOptions {
            query: "process_data".to_string(),
            kind_filter: Some(NodeKind::Struct),
            file_filter: None,
            limit: None,
        },
    );
    assert_eq!(func_results.len(), 0);

    // Test with file filter
    let py_results = search_graph(
        &graph,
        &SearchOptions {
            query: "process_data".to_string(),
            kind_filter: None,
            file_filter: Some("other.py".to_string()),
            limit: None,
        },
    );
    assert_eq!(py_results.len(), 1);
    assert_eq!(py_results[0].node.id, node4.id);
}

#[test]
fn test_bounded_neighborhood_extraction() {
    let node1 = make_node(
        "src/lib.rs",
        "src/lib.rs",
        NodeKind::File,
        Some(Language::Rust),
    );
    let node2 = make_node(
        "MyTrait",
        "src/lib.rs",
        NodeKind::Trait,
        Some(Language::Rust),
    );
    let node3 = make_node(
        "MyService",
        "src/lib.rs",
        NodeKind::Struct,
        Some(Language::Rust),
    );
    let node4 = make_node(
        "handle",
        "src/lib.rs",
        NodeKind::Method,
        Some(Language::Rust),
    );
    let node5 = make_node(
        "caller_fn",
        "src/api.rs",
        NodeKind::Function,
        Some(Language::Rust),
    );
    let node6 = make_node(
        "callee_fn",
        "src/util.rs",
        NodeKind::Function,
        Some(Language::Rust),
    );
    let node7 = make_node(
        "test_handle",
        "tests/integration_test.rs",
        NodeKind::Function,
        Some(Language::Rust),
    );

    let nodes = vec![
        node1.clone(),
        node2.clone(),
        node3.clone(),
        node4.clone(),
        node5.clone(),
        node6.clone(),
        node7.clone(),
    ];
    let edges = vec![
        make_edge(node1.id, node4.id, EdgeKind::Contains), // File contains handle
        make_edge(node4.id, node2.id, EdgeKind::Implements), // handle implements MyTrait
        make_edge(node5.id, node4.id, EdgeKind::Calls),    // caller_fn calls handle
        make_edge(node4.id, node6.id, EdgeKind::Calls),    // handle calls callee_fn
        make_edge(node7.id, node4.id, EdgeKind::Calls),    // test_handle calls handle
    ];
    let graph = Graph::new(nodes, edges);

    let neighborhood = get_neighborhood(
        &graph,
        &NeighborhoodOptions {
            target: "handle".to_string(),
            depth: 1,
            limit: 10,
        },
    )
    .expect("neighborhood found");

    assert_eq!(neighborhood.target.name, "handle");
    assert!(neighborhood.container.is_some());
    assert_eq!(neighborhood.container.unwrap().name, "src/lib.rs");
    assert_eq!(neighborhood.callers.len(), 2); // caller_fn and test_handle
    assert_eq!(neighborhood.callees.len(), 1); // callee_fn
    assert_eq!(neighborhood.trait_implementations.len(), 1); // MyTrait
    assert_eq!(neighborhood.related_tests.len(), 1); // test_handle
}

#[test]
fn test_impact_analysis_and_explanations() {
    let node1 = make_node(
        "leaf_util",
        "src/leaf.rs",
        NodeKind::Function,
        Some(Language::Rust),
    );
    let node2 = make_node(
        "middle_svc",
        "src/mid.rs",
        NodeKind::Function,
        Some(Language::Rust),
    );
    let node3 = make_node(
        "root_handler",
        "src/root.rs",
        NodeKind::Function,
        Some(Language::Rust),
    );
    let node4 = make_node(
        "test_mid",
        "tests/test_mid.rs",
        NodeKind::Function,
        Some(Language::Rust),
    );

    let nodes = vec![node1.clone(), node2.clone(), node3.clone(), node4.clone()];
    let edges = vec![
        make_edge(node2.id, node1.id, EdgeKind::Calls), // mid calls leaf
        make_edge(node3.id, node2.id, EdgeKind::Calls), // root calls mid
        make_edge(node4.id, node2.id, EdgeKind::Calls), // test_mid calls mid
    ];
    let graph = Graph::new(nodes, edges);

    let impact = analyze_impact(&graph, "leaf_util", 3).expect("impact analysis");

    assert_eq!(impact.target.name, "leaf_util");
    assert_eq!(impact.total_impacted, 3);
    assert_eq!(impact.direct_count, 1); // middle_svc
    assert_eq!(impact.transitive_count, 2); // root_handler & test_mid

    // Verify explanation paths
    let mid_impact = impact
        .impacted_nodes
        .iter()
        .find(|n| n.node.name == "middle_svc")
        .unwrap();
    assert_eq!(mid_impact.kind, ImpactKind::DirectImpact);
    assert_eq!(mid_impact.depth, 1);
    assert!(
        mid_impact
            .explanation
            .because
            .contains("middle_svc -> calls -> leaf_util")
    );

    let root_impact = impact
        .impacted_nodes
        .iter()
        .find(|n| n.node.name == "root_handler")
        .unwrap();
    assert_eq!(root_impact.kind, ImpactKind::TransitiveImpact);
    assert_eq!(root_impact.depth, 2);

    // Verify impacted files and test mapping
    assert!(impact.impacted_files.contains(&"src/leaf.rs".to_string()));
    assert!(impact.impacted_files.contains(&"src/mid.rs".to_string()));
    assert!(impact.impacted_files.contains(&"src/root.rs".to_string()));
    assert!(
        impact
            .related_tests
            .contains(&"tests/test_mid.rs".to_string())
    );
}

#[test]
fn test_deterministic_test_discovery() {
    let node1 = make_node(
        "src/auth.rs",
        "src/auth.rs",
        NodeKind::File,
        Some(Language::Rust),
    );
    let node2 = make_node(
        "login",
        "src/auth.rs",
        NodeKind::Function,
        Some(Language::Rust),
    );
    let node3 = make_node(
        "tests/auth_test.rs",
        "tests/auth_test.rs",
        NodeKind::File,
        Some(Language::Rust),
    );
    let node4 = make_node(
        "test_login",
        "tests/auth_test.rs",
        NodeKind::Function,
        Some(Language::Rust),
    );

    let nodes = vec![node1.clone(), node2.clone(), node3.clone(), node4.clone()];
    let edges = vec![
        make_edge(node1.id, node2.id, EdgeKind::Contains),
        make_edge(node3.id, node4.id, EdgeKind::Contains),
        make_edge(node4.id, node2.id, EdgeKind::Calls), // test_login calls login
    ];
    let graph = Graph::new(nodes, edges);

    let report = discover_tests(&graph);
    assert!(report.total_tests >= 1);

    let tests_for_login = map_source_to_tests(&graph, "login");
    assert_eq!(tests_for_login.len(), 1);
    assert_eq!(tests_for_login[0].test_file, "tests/auth_test.rs");
    assert_eq!(
        tests_for_login[0].test_symbol.as_deref(),
        Some("tests/auth_test.rs::test_login")
    );
}

#[test]
fn test_multi_language_entrypoints_detection() {
    let nodes = vec![
        make_node(
            "main",
            "src/main.rs",
            NodeKind::Function,
            Some(Language::Rust),
        ),
        make_node(
            "main",
            "cmd/server/main.go",
            NodeKind::Function,
            Some(Language::Go),
        ),
        make_node(
            "main",
            "src/app.py",
            NodeKind::Function,
            Some(Language::Python),
        ),
        make_node(
            "cmd_migrate",
            "src/cli.rs",
            NodeKind::Function,
            Some(Language::Rust),
        ),
        make_node(
            "helper",
            "src/util.rs",
            NodeKind::Function,
            Some(Language::Rust),
        ),
    ];
    let edges = vec![];
    let graph = Graph::new(nodes, edges);

    let entrypoints = detect_entrypoints(&graph);
    assert_eq!(entrypoints.len(), 4);

    assert!(
        entrypoints
            .iter()
            .any(|e| e.node.file == "src/main.rs" && e.kind == EntrypointKind::MainFunction)
    );
    assert!(
        entrypoints
            .iter()
            .any(|e| e.node.file == "cmd/server/main.go" && e.kind == EntrypointKind::MainFunction)
    );
    assert!(
        entrypoints
            .iter()
            .any(|e| e.node.file == "src/app.py" && e.kind == EntrypointKind::GuardedScript)
    );
    assert!(
        entrypoints
            .iter()
            .any(|e| e.node.name == "cmd_migrate" && e.kind == EntrypointKind::CliCommand)
    );
}

#[test]
fn test_architecture_overview_template() {
    let node1 = make_node(
        "main",
        "app/main.rs",
        NodeKind::Function,
        Some(Language::Rust),
    );
    let node2 = make_node(
        "serve",
        "core/server.rs",
        NodeKind::Function,
        Some(Language::Rust),
    );
    let node3 = make_node(
        "query",
        "db/client.rs",
        NodeKind::Function,
        Some(Language::Rust),
    );

    let nodes = vec![node1.clone(), node2.clone(), node3.clone()];
    let edges = vec![
        make_edge(node1.id, node2.id, EdgeKind::Calls),
        make_edge(node2.id, node3.id, EdgeKind::Calls),
        make_edge(node3.id, node2.id, EdgeKind::Calls), // Cycle between core and db
    ];
    let graph = Graph::new(nodes, edges);

    let arch = get_architecture_overview(&graph);
    assert_eq!(arch.total_nodes, 3);
    assert_eq!(arch.total_edges, 3);
    assert_eq!(arch.module_count, 3); // app, core, db
    assert_eq!(arch.entrypoints.len(), 1);
    assert_eq!(arch.cycle_count, 1);
    assert!(!arch.high_centrality_modules.is_empty());
}

#[test]
fn test_cli_intelligence_subcommands_e2e() {
    let repo = tempdir().expect("tempdir");
    let node1 = make_node(
        "main",
        "src/main.rs",
        NodeKind::Function,
        Some(Language::Rust),
    );
    let node2 = make_node(
        "process_order",
        "src/orders.rs",
        NodeKind::Function,
        Some(Language::Rust),
    );
    let node3 = make_node(
        "test_process_order",
        "tests/orders_test.rs",
        NodeKind::Function,
        Some(Language::Rust),
    );

    let nodes = vec![node1.clone(), node2.clone(), node3.clone()];
    let edges = vec![
        make_edge(node1.id, node2.id, EdgeKind::Calls),
        make_edge(node3.id, node2.id, EdgeKind::Calls),
    ];
    let graph = Graph::new(nodes, edges);
    storage::save_graph_json(&graph, &repo.path().join("graph.json")).expect("save");

    // 1. Search
    run(Cli {
        command: Commands::Search {
            repo: repo.path().to_path_buf(),
            query: "process".to_string(),
            kind: Some(CliNodeKind::Function),
            file: None,
            limit: Some(5),
            format: CliFormat::Human,
        },
    })
    .expect("search human");

    run(Cli {
        command: Commands::Search {
            repo: repo.path().to_path_buf(),
            query: "process".to_string(),
            kind: None,
            file: None,
            limit: Some(5),
            format: CliFormat::Json,
        },
    })
    .expect("search json");

    // 2. Neighborhood
    run(Cli {
        command: Commands::Neighborhood {
            repo: repo.path().to_path_buf(),
            target: "process_order".to_string(),
            depth: 2,
            limit: 10,
            format: CliFormat::Human,
        },
    })
    .expect("neighborhood human");

    run(Cli {
        command: Commands::Neighborhood {
            repo: repo.path().to_path_buf(),
            target: "process_order".to_string(),
            depth: 2,
            limit: 10,
            format: CliFormat::Json,
        },
    })
    .expect("neighborhood json");

    // 3. Impact
    run(Cli {
        command: Commands::Impact {
            repo: repo.path().to_path_buf(),
            target: "process_order".to_string(),
            depth: 3,
            files: true,
            format: CliFormat::Human,
        },
    })
    .expect("impact human files");

    run(Cli {
        command: Commands::Impact {
            repo: repo.path().to_path_buf(),
            target: "process_order".to_string(),
            depth: 3,
            files: false,
            format: CliFormat::Json,
        },
    })
    .expect("impact json");

    // 4. Tests
    run(Cli {
        command: Commands::Tests {
            repo: repo.path().to_path_buf(),
            target: Some("process_order".to_string()),
            format: CliFormat::Human,
        },
    })
    .expect("tests human target");

    run(Cli {
        command: Commands::Tests {
            repo: repo.path().to_path_buf(),
            target: None,
            format: CliFormat::Json,
        },
    })
    .expect("tests json all");

    // 5. Entrypoints
    run(Cli {
        command: Commands::Entrypoints {
            repo: repo.path().to_path_buf(),
            format: CliFormat::Human,
        },
    })
    .expect("entrypoints human");

    run(Cli {
        command: Commands::Entrypoints {
            repo: repo.path().to_path_buf(),
            format: CliFormat::Json,
        },
    })
    .expect("entrypoints json");

    // 6. Architecture
    run(Cli {
        command: Commands::Architecture {
            repo: repo.path().to_path_buf(),
            format: CliFormat::Human,
        },
    })
    .expect("architecture human");

    run(Cli {
        command: Commands::Architecture {
            repo: repo.path().to_path_buf(),
            format: CliFormat::Json,
        },
    })
    .expect("architecture json");
}

#[test]
fn test_negative_cases_and_missing_targets() {
    let repo = tempdir().expect("tempdir");
    let graph = Graph::new(vec![], vec![]);
    storage::save_graph_json(&graph, &repo.path().join("graph.json")).expect("save");

    // Neighborhood with non-existent target should error
    let res_neigh = run(Cli {
        command: Commands::Neighborhood {
            repo: repo.path().to_path_buf(),
            target: "non_existent".to_string(),
            depth: 1,
            limit: 10,
            format: CliFormat::Human,
        },
    });
    assert!(res_neigh.is_err());

    // Impact with non-existent target should error
    let res_impact = run(Cli {
        command: Commands::Impact {
            repo: repo.path().to_path_buf(),
            target: "non_existent".to_string(),
            depth: 1,
            files: false,
            format: CliFormat::Human,
        },
    });
    assert!(res_impact.is_err());

    // Search with empty query should return empty
    let empty_search = search_graph(
        &graph,
        &SearchOptions {
            query: "".to_string(),
            kind_filter: None,
            file_filter: None,
            limit: None,
        },
    );
    assert!(empty_search.is_empty());
}
