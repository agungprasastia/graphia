use std::fs;
use tempfile::tempdir;

use graphia::cli::{Cli, CliBudgetType, CliFormat, Commands, run};
use graphia::context::{
    BudgetValueType, ContextRequest, ExpansionOptions, allocate_budget, bundle_and_deduplicate,
    estimate_approx_tokens, expand_candidates, extract_lines, extract_source_slice,
    generate_context, rank_candidates, resolve_seeds, score_candidate,
};

#[test]
fn test_generate_context_api() {
    let node1 = make_node(
        "main",
        "src/main.rs",
        NodeKind::Function,
        Some(Language::Rust),
        1,
        5,
    );
    let graph = Graph::new(vec![node1.clone()], vec![]);
    let req = ContextRequest {
        symbol: Some("main".to_string()),
        ..Default::default()
    };
    let bundle = generate_context(&graph, &req, None);
    assert_eq!(bundle.schema_version, 1);
    assert_eq!(bundle.total_items, 1);
    let score = score_candidate(&graphia::context::ContextCandidate {
        node: node1,
        role: graphia::context::CandidateRole::Seed,
        distance: 0,
        reason: "seed".into(),
    });
    assert_eq!(score, 1000.0);
}
use graphia::graph::Graph;
use graphia::model::{
    Confidence, Edge, EdgeKind, Language, Node, NodeId, NodeKind, SourceLocation,
};
use graphia::storage;

fn make_node(
    name: &str,
    file: &str,
    kind: NodeKind,
    lang: Option<Language>,
    start_line: u32,
    end_line: u32,
) -> Node {
    let loc = SourceLocation {
        file: file.to_string(),
        start_line,
        start_col: 1,
        end_line,
        end_col: 1,
    };
    let qualified_name = format!("{file}::{name}");
    let id = graphia::graph::stable_node_id(&graphia::model::NodeIdentity::new(
        lang,
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
        language: lang,
        visibility: graphia::model::Visibility::Public,
        signature: None,
        container: None,
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
fn test_context_seed_resolution() {
    let node1 = make_node(
        "auth_service",
        "src/auth.rs",
        NodeKind::Struct,
        Some(Language::Rust),
        1,
        10,
    );
    let node2 = make_node(
        "login",
        "src/auth.rs",
        NodeKind::Method,
        Some(Language::Rust),
        12,
        20,
    );
    let node3 = make_node(
        "db_pool",
        "src/db.rs",
        NodeKind::Struct,
        Some(Language::Rust),
        1,
        5,
    );

    let nodes = vec![node1.clone(), node2.clone(), node3.clone()];
    let edges = vec![];
    let graph = Graph::new(nodes, edges);

    // 1. Resolve by symbol
    let req_sym = ContextRequest {
        symbol: Some("login".to_string()),
        ..Default::default()
    };
    let seeds_sym = resolve_seeds(&graph, &req_sym, None);
    assert_eq!(seeds_sym.len(), 1);
    assert_eq!(seeds_sym[0].name, "login");

    // 2. Resolve by file
    let req_file = ContextRequest {
        file: Some("src/auth.rs".to_string()),
        ..Default::default()
    };
    let seeds_file = resolve_seeds(&graph, &req_file, None);
    assert_eq!(seeds_file.len(), 2);

    // 3. Resolve by query text
    let req_query = ContextRequest {
        query: Some("auth".to_string()),
        ..Default::default()
    };
    let seeds_query = resolve_seeds(&graph, &req_query, None);
    assert!(!seeds_query.is_empty());
}

#[test]
fn test_candidate_expansion_and_distance_decay_scoring() {
    let node_file = make_node(
        "src/auth.rs",
        "src/auth.rs",
        NodeKind::File,
        Some(Language::Rust),
        1,
        50,
    );
    let node_trait = make_node(
        "Authenticator",
        "src/auth.rs",
        NodeKind::Trait,
        Some(Language::Rust),
        5,
        10,
    );
    let node_struct = make_node(
        "AuthService",
        "src/auth.rs",
        NodeKind::Struct,
        Some(Language::Rust),
        12,
        30,
    );
    let node_fn = make_node(
        "login",
        "src/auth.rs",
        NodeKind::Function,
        Some(Language::Rust),
        32,
        40,
    );
    let node_caller = make_node(
        "handle_login",
        "src/api.rs",
        NodeKind::Function,
        Some(Language::Rust),
        10,
        25,
    );
    let node_callee = make_node(
        "verify_hash",
        "src/crypto.rs",
        NodeKind::Function,
        Some(Language::Rust),
        5,
        15,
    );
    let node_test = make_node(
        "test_login",
        "tests/auth_test.rs",
        NodeKind::Function,
        Some(Language::Rust),
        1,
        20,
    );

    let nodes = vec![
        node_file.clone(),
        node_trait.clone(),
        node_struct.clone(),
        node_fn.clone(),
        node_caller.clone(),
        node_callee.clone(),
        node_test.clone(),
    ];

    let edges = vec![
        make_edge(node_file.id, node_fn.id, EdgeKind::Contains),
        make_edge(node_fn.id, node_trait.id, EdgeKind::Implements),
        make_edge(node_caller.id, node_fn.id, EdgeKind::Calls),
        make_edge(node_fn.id, node_callee.id, EdgeKind::Calls),
        make_edge(node_test.id, node_fn.id, EdgeKind::Calls),
    ];

    let graph = Graph::new(nodes, edges);

    let seeds = vec![node_fn.clone()];
    let expansion_opts = ExpansionOptions {
        max_depth: 3,
        max_candidates: 50,
    };
    let candidates = expand_candidates(&graph, &seeds, &expansion_opts);
    assert!(candidates.len() >= 5);

    let ranked = rank_candidates(candidates);
    // Seed must rank #1 with score 1000
    assert_eq!(ranked[0].candidate.node.id, node_fn.id);
    assert_eq!(ranked[0].score, 1000.0);

    // Callers/callees should rank high (800)
    let callee_rank = ranked
        .iter()
        .find(|r| r.candidate.node.id == node_callee.id)
        .unwrap();
    assert_eq!(callee_rank.score, 800.0);

    // Container should rank 750
    let container_rank = ranked
        .iter()
        .find(|r| r.candidate.node.id == node_file.id)
        .unwrap();
    assert_eq!(container_rank.score, 750.0);

    // Test should score 200
    let test_rank = ranked
        .iter()
        .find(|r| r.candidate.node.id == node_test.id)
        .unwrap();
    assert_eq!(test_rank.score, 200.0);
}

#[test]
fn test_ast_source_slicing_and_token_estimation() {
    let source_code = "line 1: fn foo() {\nline 2:     let x = 1;\nline 3:     let y = 2;\nline 4: }\nline 5: fn bar() {}\n";
    let lines_extracted = extract_lines(source_code, 2, 1, 4, 1);
    assert_eq!(
        lines_extracted,
        "line 2:     let x = 1;\nline 3:     let y = 2;\nline 4: }"
    );

    let tokens = estimate_approx_tokens(&lines_extracted);
    assert!(tokens > 0);
    assert_eq!(tokens, lines_extracted.chars().count().div_ceil(4));

    let dir = tempdir().expect("tempdir");
    let file_path = dir.path().join("test.rs");
    fs::write(&file_path, source_code).expect("write file");

    let loc = SourceLocation {
        file: "test.rs".to_string(),
        start_line: 1,
        start_col: 1,
        end_line: 4,
        end_col: 1,
    };
    let slice = extract_source_slice(Some(dir.path()), &loc).expect("extract");
    assert_eq!(slice.start_line, 1);
    assert_eq!(slice.end_line, 4);
    assert_eq!(
        slice.content,
        "line 1: fn foo() {\nline 2:     let x = 1;\nline 3:     let y = 2;\nline 4: }"
    );
}

#[test]
fn test_budget_allocation_and_enforcement() {
    let node1 = make_node(
        "fn1",
        "src/lib.rs",
        NodeKind::Function,
        Some(Language::Rust),
        1,
        10,
    );
    let node2 = make_node(
        "fn2",
        "src/lib.rs",
        NodeKind::Function,
        Some(Language::Rust),
        11,
        20,
    );
    let node3 = make_node(
        "fn3",
        "src/lib.rs",
        NodeKind::Function,
        Some(Language::Rust),
        21,
        30,
    );

    let dir = tempdir().expect("tempdir");
    let content = (1..=35)
        .map(|i| format!("line {i}: content\n"))
        .collect::<String>();
    fs::write(dir.path().join("src").join("lib.rs").as_path(), "").ok();
    fs::create_dir_all(dir.path().join("src")).expect("mkdir");
    fs::write(dir.path().join("src/lib.rs"), &content).expect("write");

    let candidates = vec![
        graphia::context::ContextCandidate {
            node: node1.clone(),
            role: graphia::context::CandidateRole::Seed,
            distance: 0,
            reason: "seed".into(),
        },
        graphia::context::ContextCandidate {
            node: node2.clone(),
            role: graphia::context::CandidateRole::Callee,
            distance: 1,
            reason: "callee".into(),
        },
        graphia::context::ContextCandidate {
            node: node3.clone(),
            role: graphia::context::CandidateRole::Callee,
            distance: 2,
            reason: "callee2".into(),
        },
    ];

    let ranked = rank_candidates(candidates);

    // Tight budget allowing only 1-2 items
    let config = graphia::context::BudgetConfig {
        limit: 50, // very small token limit
        budget_type: BudgetValueType::ApproxTokens,
    };

    let (items, report) = allocate_budget(ranked, &config, Some(dir.path()));
    assert!(report.items_included >= 1);
    assert!(report.items_omitted >= 1);
    assert!(report.budget_used <= 50 || items.len() == 1);
}

#[test]
fn test_context_bundle_deduplication_and_json_schema() {
    let node_parent = make_node(
        "MyClass",
        "src/service.rs",
        NodeKind::Class,
        Some(Language::Rust),
        1,
        30,
    );
    let node_child = make_node(
        "my_method",
        "src/service.rs",
        NodeKind::Method,
        Some(Language::Rust),
        5,
        15,
    );

    let dir = tempdir().expect("tempdir");
    fs::create_dir_all(dir.path().join("src")).expect("mkdir");
    let content = (1..=35)
        .map(|i| format!("    line {i}\n"))
        .collect::<String>();
    fs::write(dir.path().join("src/service.rs"), &content).expect("write");

    let slice_parent =
        extract_source_slice(Some(dir.path()), &node_parent.location).expect("slice parent");
    let slice_child =
        extract_source_slice(Some(dir.path()), &node_child.location).expect("slice child");

    let items = vec![
        graphia::context::BudgetedItem {
            node: node_parent.clone(),
            role: graphia::context::CandidateRole::Container,
            distance: 1,
            score: 750.0,
            reason: "container".into(),
            slice: slice_parent,
        },
        graphia::context::BudgetedItem {
            node: node_child.clone(),
            role: graphia::context::CandidateRole::Seed,
            distance: 0,
            score: 1000.0,
            reason: "seed".into(),
            slice: slice_child,
        },
    ];

    let report = graphia::context::BudgetReport {
        budget_type: BudgetValueType::ApproxTokens,
        budget_limit: 8000,
        budget_used: 100,
        items_included: 2,
        items_omitted: 0,
    };

    let bundle = bundle_and_deduplicate(items, report);
    assert_eq!(bundle.schema_version, 1);
    assert_eq!(bundle.files.len(), 1);
    // Overlapping child should be enclosed and deduplicated
    assert_eq!(bundle.files[0].slices.len(), 1);
    assert_eq!(bundle.files[0].slices[0].start_line, 1);
    assert_eq!(bundle.files[0].slices[0].end_line, 30);

    let json = serde_json::to_string_pretty(&bundle).expect("serialize");
    assert!(json.contains("\"schema_version\": 1"));
    assert!(json.contains("\"slices\":"));
}

#[test]
fn test_cli_context_subcommand_e2e() {
    let repo = tempdir().expect("tempdir");
    fs::create_dir_all(repo.path().join("src")).expect("mkdir");
    fs::write(
        repo.path().join("src/main.rs"),
        "fn main() {\n    process_order();\n}\n\nfn process_order() {\n    println!(\"order\");\n}\n",
    )
    .expect("write main.rs");

    let node1 = make_node(
        "main",
        "src/main.rs",
        NodeKind::Function,
        Some(Language::Rust),
        1,
        3,
    );
    let node2 = make_node(
        "process_order",
        "src/main.rs",
        NodeKind::Function,
        Some(Language::Rust),
        5,
        7,
    );

    let nodes = vec![node1.clone(), node2.clone()];
    let edges = vec![make_edge(node1.id, node2.id, EdgeKind::Calls)];
    let graph = Graph::new(nodes, edges);
    storage::save_graph_json(&graph, &repo.path().join("graph.json")).expect("save");

    // Human format
    run(Cli {
        command: Commands::Context {
            repo: repo.path().to_path_buf(),
            symbol: Some("process_order".to_string()),
            file: None,
            query: None,
            changed: false,
            token_budget: Some(4000),
            budget_type: CliBudgetType::Tokens,
            depth: 2,
            limit: 50,
            format: CliFormat::Human,
        },
    })
    .expect("context human");

    // Json format
    run(Cli {
        command: Commands::Context {
            repo: repo.path().to_path_buf(),
            symbol: None,
            file: Some("src/main.rs".to_string()),
            query: None,
            changed: false,
            token_budget: Some(4000),
            budget_type: CliBudgetType::Tokens,
            depth: 2,
            limit: 50,
            format: CliFormat::Json,
        },
    })
    .expect("context json");
}
