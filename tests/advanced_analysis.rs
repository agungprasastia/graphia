use tempfile::tempdir;

use graphia::analysis::advanced::{
    ArchitectureRulesConfig, DataFlowQuery, LayerDefinition, analyze_callgraph,
    build_dataflow_graph, check_architecture_boundaries, compute_change_coupling,
    detect_dead_code_candidates, diff_graphs, diff_public_api, extract_intraprocedural_typeflow,
    find_source_sink_flows,
};
use graphia::cli::{Cli, CliFormat, Commands, run};
use graphia::graph::Graph;
use graphia::model::{Confidence, Edge, EdgeId, Language, Node, NodeId, NodeKind, SourceLocation};

fn loc(file: &str, line: u32) -> SourceLocation {
    SourceLocation {
        file: file.to_string(),
        start_line: line,
        start_col: 1,
        end_line: line + 5,
        end_col: 1,
    }
}

#[test]
fn test_advanced_callgraph_dynamic_dispatch() {
    let nodes = vec![
        Node {
            id: NodeId(1),
            kind: NodeKind::Interface,
            name: "Logger".into(),
            qualified_name: "Logger".into(),
            file: "log.rs".into(),
            location: loc("log.rs", 1),
            language: Some(Language::Rust),
            visibility: graphia::model::Visibility::Public,
            signature: None,
            container: None,
        },
        Node {
            id: NodeId(2),
            kind: NodeKind::Method,
            name: "log".into(),
            qualified_name: "Logger::log".into(),
            file: "log.rs".into(),
            location: loc("log.rs", 2),
            language: Some(Language::Rust),
            visibility: graphia::model::Visibility::Public,
            signature: None,
            container: Some("Logger".into()),
        },
        Node {
            id: NodeId(3),
            kind: NodeKind::Struct,
            name: "ConsoleLogger".into(),
            qualified_name: "ConsoleLogger".into(),
            file: "console.rs".into(),
            location: loc("console.rs", 1),
            language: Some(Language::Rust),
            visibility: graphia::model::Visibility::Public,
            signature: None,
            container: None,
        },
        Node {
            id: NodeId(4),
            kind: NodeKind::Method,
            name: "log".into(),
            qualified_name: "ConsoleLogger::log".into(),
            file: "console.rs".into(),
            location: loc("console.rs", 2),
            language: Some(Language::Rust),
            visibility: graphia::model::Visibility::Public,
            signature: None,
            container: Some("ConsoleLogger".into()),
        },
        Node {
            id: NodeId(5),
            kind: NodeKind::Function,
            name: "app_run".into(),
            qualified_name: "app_run".into(),
            file: "main.rs".into(),
            location: loc("main.rs", 1),
            language: Some(Language::Rust),
            visibility: graphia::model::Visibility::Public,
            signature: None,
            container: None,
        },
    ];

    let edges = vec![
        Edge {
            id: EdgeId(1),
            kind: graphia::model::EdgeKind::Contains,
            from: NodeId(1),
            to: NodeId(2),
            confidence: Confidence::Extracted,
            label: None,
        },
        Edge {
            id: EdgeId(2),
            kind: graphia::model::EdgeKind::Contains,
            from: NodeId(3),
            to: NodeId(4),
            confidence: Confidence::Extracted,
            label: None,
        },
        Edge {
            id: EdgeId(3),
            kind: graphia::model::EdgeKind::Implements,
            from: NodeId(3),
            to: NodeId(1),
            confidence: Confidence::Extracted,
            label: None,
        },
        Edge {
            id: EdgeId(4),
            kind: graphia::model::EdgeKind::Calls,
            from: NodeId(5),
            to: NodeId(2),
            confidence: Confidence::Inferred,
            label: None,
        },
    ];

    let graph = Graph::new(nodes, edges);
    let result = analyze_callgraph(&graph);

    assert_eq!(result.total_call_sites, 1);
    assert_eq!(result.dynamic_dispatch_count, 1);
    assert_eq!(result.call_sites[0].dynamic_dispatch_candidates.len(), 1);
    assert_eq!(
        result.call_sites[0].dynamic_dispatch_candidates[0]
            .target
            .qualified_name,
        "ConsoleLogger::log"
    );
}

#[test]
fn test_advanced_typeflow_and_dataflow() {
    let src = "let x = foo();\nlet y = x;\nreturn y;";
    let flow = extract_intraprocedural_typeflow("test_fn", "src/test.rs", src, 10);
    assert_eq!(flow.assignments.len(), 2);
    assert_eq!(flow.assignments[1].from_var, "x");
    assert_eq!(flow.assignments[1].to_var, "y");
    assert_eq!(flow.return_sources, vec!["y".to_string()]);

    let nodes = vec![
        Node {
            id: NodeId(1),
            kind: NodeKind::Function,
            name: "source_handler".into(),
            qualified_name: "source_handler".into(),
            file: "api.rs".into(),
            location: loc("api.rs", 1),
            language: Some(Language::Rust),
            visibility: graphia::model::Visibility::Public,
            signature: None,
            container: None,
        },
        Node {
            id: NodeId(2),
            kind: NodeKind::Function,
            name: "intermediate_service".into(),
            qualified_name: "intermediate_service".into(),
            file: "service.rs".into(),
            location: loc("service.rs", 1),
            language: Some(Language::Rust),
            visibility: graphia::model::Visibility::Public,
            signature: None,
            container: None,
        },
        Node {
            id: NodeId(3),
            kind: NodeKind::Function,
            name: "database_sink".into(),
            qualified_name: "database_sink".into(),
            file: "db.rs".into(),
            location: loc("db.rs", 1),
            language: Some(Language::Rust),
            visibility: graphia::model::Visibility::Public,
            signature: None,
            container: None,
        },
    ];

    let edges = vec![
        Edge {
            id: EdgeId(1),
            kind: graphia::model::EdgeKind::Calls,
            from: NodeId(1),
            to: NodeId(2),
            confidence: Confidence::Extracted,
            label: None,
        },
        Edge {
            id: EdgeId(2),
            kind: graphia::model::EdgeKind::Calls,
            from: NodeId(2),
            to: NodeId(3),
            confidence: Confidence::Inferred,
            label: None,
        },
    ];

    let graph = Graph::new(nodes, edges);
    let flow_report = find_source_sink_flows(&graph, "source_handler", "database_sink", Some(5));

    assert_eq!(flow_report.paths_found, 0);
}

#[test]
fn test_dataflow_cycles_are_cycle_safe_and_depth_bounded() {
    let nodes = (1..=3)
        .map(|id| Node {
            id: NodeId(id),
            kind: NodeKind::Function,
            name: format!("f{id}"),
            qualified_name: format!("f{id}"),
            file: "cycle.rs".into(),
            location: loc("cycle.rs", id as u32),
            language: Some(Language::Rust),
            visibility: graphia::model::Visibility::Private,
            signature: None,
            container: None,
        })
        .collect::<Vec<_>>();
    let edges = [
        (1, 2, Confidence::Extracted),
        (2, 1, Confidence::Possible),
        (2, 3, Confidence::Inferred),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (from, to, confidence))| Edge {
        id: EdgeId(index as u64 + 1),
        kind: graphia::model::EdgeKind::Calls,
        from: NodeId(from),
        to: NodeId(to),
        confidence,
        label: None,
    })
    .collect();
    let graph = Graph::new(nodes, edges);
    let dataflow = build_dataflow_graph(&graph);
    let query = DataFlowQuery::new(&dataflow);

    let paths = query.trace_flow(NodeId(1), NodeId(3), 3, 5);
    assert!(paths.is_empty());
    assert!(query.trace_flow(NodeId(1), NodeId(3), 1, 5).is_empty());
}

#[test]
fn test_imports_and_contains_never_create_dataflow_paths() {
    let nodes = vec![
        Node {
            id: NodeId(1),
            kind: NodeKind::Function,
            name: "source".into(),
            qualified_name: "source".into(),
            file: "a.rs".into(),
            location: loc("a.rs", 1),
            language: Some(Language::Rust),
            visibility: graphia::model::Visibility::Private,
            signature: None,
            container: None,
        },
        Node {
            id: NodeId(2),
            kind: NodeKind::Function,
            name: "sink".into(),
            qualified_name: "sink".into(),
            file: "b.rs".into(),
            location: loc("b.rs", 1),
            language: Some(Language::Rust),
            visibility: graphia::model::Visibility::Private,
            signature: None,
            container: None,
        },
    ];
    let edges = vec![
        Edge {
            id: EdgeId(1),
            kind: graphia::model::EdgeKind::Imports,
            from: NodeId(1),
            to: NodeId(2),
            confidence: Confidence::Extracted,
            label: None,
        },
        Edge {
            id: EdgeId(2),
            kind: graphia::model::EdgeKind::Contains,
            from: NodeId(1),
            to: NodeId(2),
            confidence: Confidence::Extracted,
            label: None,
        },
    ];
    let graph = Graph::new(nodes, edges);
    let dataflow = build_dataflow_graph(&graph);
    let query = DataFlowQuery::new(&dataflow);

    assert!(query.trace_flow(NodeId(1), NodeId(2), 5, 5).is_empty());
    assert!(
        find_source_sink_flows(&graph, "source", "sink", Some(5))
            .paths
            .is_empty()
    );
}

#[test]
fn runtime_ast_flow_links_parameter_assignment_and_return() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        temp.path().join("flow.rs"),
        "pub fn process(input: String) -> String { let payload = input; return payload; }\n",
    )
    .expect("write flow source");
    let graph = graphia::storage::build_graph_from_repo(temp.path()).expect("build graph");
    let report = find_source_sink_flows(&graph, "input", "return", Some(5));
    assert_eq!(report.paths_found, 1);
    assert!(
        report.paths[0]
            .steps
            .iter()
            .any(|step| step.edge_type == "References")
    );
}

#[test]
fn test_advanced_boundaries_and_drift() {
    let nodes = vec![
        Node {
            id: NodeId(1),
            kind: NodeKind::Function,
            name: "domain_fn".into(),
            qualified_name: "domain_fn".into(),
            file: "src/model/domain.rs".into(),
            location: loc("src/model/domain.rs", 1),
            language: Some(Language::Rust),
            visibility: graphia::model::Visibility::Public,
            signature: None,
            container: None,
        },
        Node {
            id: NodeId(2),
            kind: NodeKind::Function,
            name: "app_fn".into(),
            qualified_name: "app_fn".into(),
            file: "src/cli/app.rs".into(),
            location: loc("src/cli/app.rs", 1),
            language: Some(Language::Rust),
            visibility: graphia::model::Visibility::Public,
            signature: None,
            container: None,
        },
    ];

    // Illegal: domain calling app
    let edges = vec![Edge {
        id: EdgeId(1),
        kind: graphia::model::EdgeKind::Calls,
        from: NodeId(1),
        to: NodeId(2),
        confidence: Confidence::Extracted,
        label: None,
    }];

    let graph = Graph::new(nodes, edges);
    let config = ArchitectureRulesConfig {
        layers: vec![
            LayerDefinition {
                name: "domain".into(),
                path_patterns: vec!["model".into()],
                allowed_dependencies: vec![],
            },
            LayerDefinition {
                name: "app".into(),
                path_patterns: vec!["cli".into()],
                allowed_dependencies: vec!["domain".into()],
            },
        ],
    };

    let report = check_architecture_boundaries(&graph, &config);
    assert!(!report.passed);
    assert_eq!(report.violations_count, 1);
    assert_eq!(report.violations[0].from_layer, "domain");
    assert_eq!(report.violations[0].to_layer, "app");
}

#[test]
fn test_advanced_change_coupling_and_history() {
    let commits = vec![
        graphia::analysis::advanced::GitCommitRecord {
            commit_hash: "c1".into(),
            author: "dev1".into(),
            timestamp: 100,
            files_changed: vec!["src/a.rs".into(), "src/b.rs".into()],
        },
        graphia::analysis::advanced::GitCommitRecord {
            commit_hash: "c2".into(),
            author: "dev2".into(),
            timestamp: 200,
            files_changed: vec!["src/a.rs".into(), "src/b.rs".into(), "src/c.rs".into()],
        },
        graphia::analysis::advanced::GitCommitRecord {
            commit_hash: "c3".into(),
            author: "dev1".into(),
            timestamp: 300,
            files_changed: vec!["src/a.rs".into()],
        },
    ];

    let report = compute_change_coupling(&commits, Some(0.1));
    assert_eq!(report.total_commits_analyzed, 3);
    assert!(!report.pairs.is_empty());
    let ab_pair = report
        .pairs
        .iter()
        .find(|p| p.file_a == "src/a.rs" && p.file_b == "src/b.rs")
        .expect("pair a-b found");
    assert_eq!(ab_pair.co_commits, 2);
    assert_eq!(ab_pair.commits_a, 3);
    assert_eq!(ab_pair.commits_b, 2);
    assert!((ab_pair.confidence_b_to_a - 1.0).abs() < 1e-6);
}

#[test]
fn test_advanced_dead_code_and_diffs() {
    let old_nodes = vec![
        Node {
            id: NodeId(1),
            kind: NodeKind::Function,
            name: "live_fn".into(),
            qualified_name: "live_fn".into(),
            file: "src/lib.rs".into(),
            location: loc("src/lib.rs", 1),
            language: Some(Language::Rust),
            visibility: graphia::model::Visibility::Public,
            signature: None,
            container: None,
        },
        Node {
            id: NodeId(2),
            kind: NodeKind::Function,
            name: "dead_fn".into(),
            qualified_name: "dead_fn".into(),
            file: "src/lib.rs".into(),
            location: loc("src/lib.rs", 10),
            language: Some(Language::Rust),
            visibility: graphia::model::Visibility::Private,
            signature: None,
            container: None,
        },
    ];
    let old_edges = vec![];
    let old_graph = Graph::new(old_nodes, old_edges);

    let dead = detect_dead_code_candidates(&old_graph);
    assert_eq!(dead.candidates_count, 2);

    let new_nodes = vec![
        Node {
            id: NodeId(1),
            kind: NodeKind::Function,
            name: "live_fn".into(),
            qualified_name: "live_fn".into(),
            file: "src/lib.rs".into(),
            location: loc("src/lib.rs", 5), // moved lines
            language: Some(Language::Rust),
            visibility: graphia::model::Visibility::Public,
            signature: None,
            container: None,
        },
        Node {
            id: NodeId(3),
            kind: NodeKind::Function,
            name: "new_pub_fn".into(),
            qualified_name: "new_pub_fn".into(),
            file: "src/lib.rs".into(),
            location: loc("src/lib.rs", 20),
            language: Some(Language::Rust),
            visibility: graphia::model::Visibility::Public,
            signature: None,
            container: None,
        },
    ];
    let new_edges = vec![];
    let new_graph = Graph::new(new_nodes, new_edges);

    let gdiff = diff_graphs(&old_graph, &new_graph);
    assert_eq!(gdiff.added_nodes.len(), 1);
    assert_eq!(gdiff.removed_nodes.len(), 1);
    assert_eq!(gdiff.modified_nodes.len(), 1);

    let apidiff = diff_public_api(&old_graph, &new_graph);
    assert_eq!(apidiff.added_public_symbols.len(), 1);
    assert_eq!(apidiff.removed_public_symbols.len(), 0);
}

#[test]
fn test_api_diff_reports_only_changed_overload() {
    let overload = |signature: &str, line: u32| Node {
        id: NodeId(line as u64),
        kind: NodeKind::Function,
        name: "foo".into(),
        qualified_name: "src/api.rs::foo".into(),
        file: "src/api.rs".into(),
        location: loc("src/api.rs", line),
        language: Some(Language::Rust),
        visibility: graphia::model::Visibility::Public,
        signature: Some(signature.into()),
        container: None,
    };
    let old = Graph::new(vec![overload("(int)", 1), overload("(string)", 10)], vec![]);
    let new = Graph::new(
        vec![overload("(int)", 1), overload("(string, bool)", 10)],
        vec![],
    );

    let diff = diff_public_api(&old, &new);
    assert_eq!(diff.modified_signatures.len(), 1);
    assert_eq!(diff.added_public_symbols.len(), 0);
    assert_eq!(diff.removed_public_symbols.len(), 0);
    assert_eq!(
        diff.modified_signatures[0].symbol,
        "src/api.rs::foo(string, bool)"
    );
    assert_eq!(
        diff.modified_signatures[0].old_signature.as_deref(),
        Some("(string)")
    );
}

#[test]
fn test_git_history_structured_status_and_binary_numstat() {
    let non_repo = tempdir().expect("tempdir");
    assert!(matches!(
        graphia::analysis::advanced::analyze_git_history(non_repo.path(), Some(1)),
        graphia::analysis::advanced::GitHistoryResult::NotGitRepository
    ));

    let empty_repo = tempdir().expect("tempdir");
    std::process::Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(empty_repo.path())
        .status()
        .expect("git init");
    assert!(matches!(
        graphia::analysis::advanced::analyze_git_history(empty_repo.path(), Some(1)),
        graphia::analysis::advanced::GitHistoryResult::EmptyHistory
    ));

    std::fs::write(empty_repo.path().join("image.bin"), [0_u8, 1, 2, 255]).expect("binary file");
    std::process::Command::new("git")
        .args(["add", "image.bin"])
        .current_dir(empty_repo.path())
        .status()
        .expect("git add");
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--quiet",
            "-m",
            "binary",
        ])
        .current_dir(empty_repo.path())
        .status()
        .expect("git commit");
    let result = graphia::analysis::advanced::analyze_git_history(empty_repo.path(), Some(1));
    let graphia::analysis::advanced::GitHistoryResult::Success(summary) = result else {
        panic!("expected successful binary history");
    };
    assert_eq!(summary.files[0].binary_files, 1);
    assert_eq!(summary.files[0].additions, 0);
    assert_eq!(summary.files[0].deletions, 0);
}

#[test]
fn test_cli_advanced_analysis_commands() {
    let repo = tempdir().expect("tempdir");
    let graph = Graph::new(vec![], vec![]);
    graphia::storage::save_graph_json(&graph, &repo.path().join("graph.json")).expect("save");

    run(Cli {
        command: Commands::Flow {
            repo: Some(repo.path().to_path_buf()),
            source: "src".into(),
            sink: "sink".into(),
            limit: Some(5),
            format: CliFormat::Human,
        },
    })
    .expect("flow command");

    run(Cli {
        command: Commands::ArchitectureCheck {
            repo: Some(repo.path().to_path_buf()),
            config: None,
            format: CliFormat::Human,
        },
    })
    .expect("architecture check");

    run(Cli {
        command: Commands::Deadcode {
            repo: Some(repo.path().to_path_buf()),
            format: CliFormat::Human,
        },
    })
    .expect("deadcode command");
}
