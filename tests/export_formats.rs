use std::fs;
use tempfile::tempdir;

use graphia::cli::{Cli, Commands, run};
use graphia::export::{
    export_cytoscape, export_dot, export_gexf, export_graphml, export_mermaid, export_obsidian,
};
use graphia::graph::{Graph, stable_edge_id, stable_node_id};
use graphia::model::{
    Confidence, Edge, EdgeIdentity, EdgeKind, Language, Node, NodeId, NodeIdentity, NodeKind,
    SourceLocation, Visibility,
};

fn make_test_node(name: &str, file: &str, kind: NodeKind) -> Node {
    let loc = SourceLocation {
        file: file.to_string(),
        start_line: 1,
        start_col: 1,
        end_line: 10,
        end_col: 1,
    };
    let qualified_name = format!("{file}::{name}");
    let id = stable_node_id(&NodeIdentity::new(
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

fn make_test_edge(from: NodeId, to: NodeId, kind: EdgeKind) -> Edge {
    let id = stable_edge_id(&EdgeIdentity::new(
        from,
        to,
        kind,
        Confidence::Resolved,
        None,
    ));
    Edge {
        id,
        kind,
        from,
        to,
        confidence: Confidence::Resolved,
        label: None,
    }
}

fn create_sample_graph() -> Graph {
    let node1 = make_test_node("process_order", "src/service.rs", NodeKind::Function);
    let node2 = make_test_node("OrderRepository", "src/repo.rs", NodeKind::Struct);
    let edge1 = make_test_edge(node1.id, node2.id, EdgeKind::Calls);

    let mut graph = Graph::new(vec![node1, node2], vec![edge1]);
    graph.canonicalize().expect("canonicalize graph");
    graph
}

#[test]
fn test_export_dot_syntax() {
    let graph = create_sample_graph();
    let dot = export_dot(&graph);

    assert!(dot.contains("digraph Graphia"));
    assert!(dot.contains("process_order"));
    assert!(dot.contains("OrderRepository"));
    assert!(dot.contains("->"));
}

#[test]
fn test_export_mermaid_syntax() {
    let graph = create_sample_graph();
    let mermaid = export_mermaid(&graph, None);

    assert!(mermaid.starts_with("flowchart TD"));
    assert!(mermaid.contains("subgraph"));
    assert!(mermaid.contains("process_order"));
    assert!(mermaid.contains("OrderRepository"));
    assert!(mermaid.contains("-->|calls|"));
}

#[test]
fn test_export_graphml_xml() {
    let graph = create_sample_graph();
    let graphml = export_graphml(&graph);

    assert!(graphml.contains("<?xml"));
    assert!(graphml.contains("<graphml"));
    assert!(graphml.contains("<key id=\"d_name\""));
    assert!(graphml.contains("process_order"));
    assert!(graphml.contains("OrderRepository"));
    assert!(graphml.contains("<edge id="));
}

#[test]
fn test_export_gexf_xml() {
    let graph = create_sample_graph();
    let gexf = export_gexf(&graph);

    assert!(gexf.contains("<gexf"));
    assert!(gexf.contains("label=\"process_order\""));
    assert!(gexf.contains("label=\"OrderRepository\""));
    assert!(gexf.contains("label=\"Calls\""));
}

#[test]
fn test_export_cytoscape_json() {
    let graph = create_sample_graph();
    let cyto = export_cytoscape(&graph);

    let parsed: serde_json::Value = serde_json::from_str(&cyto).expect("valid json");
    assert!(parsed["elements"]["nodes"].is_array());
    assert_eq!(parsed["elements"]["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(parsed["elements"]["edges"].as_array().unwrap().len(), 1);
}

#[test]
fn test_export_obsidian_vault_structure() {
    let graph = create_sample_graph();
    let dir = tempdir().expect("tempdir");

    export_obsidian(&graph, dir.path()).expect("export obsidian");

    assert!(dir.path().join(".obsidian/graph.json").exists());
    assert!(dir.path().join("00-Index.md").exists());
    assert!(dir.path().join("01-Files.md").exists());

    let symbol_file = dir.path().join("symbols/Function/process_order.md");
    assert!(symbol_file.exists());
    let symbol_content = fs::read_to_string(&symbol_file).expect("read symbol file");
    assert!(symbol_content.contains("name: \"process_order\""));
    assert!(symbol_content.contains("kind: Function"));
    assert!(symbol_content.contains("[[symbols/Struct/OrderRepository|OrderRepository]]"));
}

#[test]
fn test_cli_export_subcommands() {
    let repo = tempdir().expect("tempdir");
    let graph = create_sample_graph();

    fs::create_dir_all(repo.path().join(".graphia")).expect("create .graphia");
    graphia::storage::save_graph_binary(&graph, &repo.path().join(".graphia/index.bin"))
        .expect("save binary");

    let formats = vec![
        ("dot", "output.dot"),
        ("mermaid", "output.mmd"),
        ("graphml", "output.graphml"),
        ("gexf", "output.gexf"),
        ("cytoscape", "output.cyto.json"),
        ("json", "output.json"),
    ];

    for (fmt, filename) in formats {
        let out_file = repo.path().join(filename);
        let cli = Cli {
            command: Commands::Export {
                repo: repo.path().to_path_buf(),
                format: fmt.to_string(),
                output: Some(out_file.clone()),
            },
        };
        run(cli).expect("run cli export");
        assert!(out_file.exists(), "file {filename} should exist");
        assert!(fs::metadata(&out_file).unwrap().len() > 0);
    }

    // Test obsidian format CLI
    let vault_dir = repo.path().join("my-vault");
    let cli_obsidian = Cli {
        command: Commands::Export {
            repo: repo.path().to_path_buf(),
            format: "obsidian".to_string(),
            output: Some(vault_dir.clone()),
        },
    };
    run(cli_obsidian).expect("run obsidian export");
    assert!(vault_dir.join("00-Index.md").exists());
}
