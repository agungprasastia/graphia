use graphia::graph::build_graph;
use graphia::model::{EdgeKind, Language as GraphiaLanguage};
use graphia::parser::parse_file;

#[test]
fn test_build_graph_creates_edges_from_ir() {
    let code_a = r#"
pub struct Service {
    pub name: String,
}

#[test]
fn test_build_graph_links_relationship_ir() {
    let base = parse_file("base.rs", GraphiaLanguage::Rust, "trait Render {} struct Parent;");
    let child = parse_file(
        "child.rs",
        GraphiaLanguage::Rust,
        "struct Child; impl Render for Child {} fn make() { Child; }",
    );
    let graph = build_graph(vec![
        ("base.rs".to_string(), Some(GraphiaLanguage::Rust), base),
        ("child.rs".to_string(), Some(GraphiaLanguage::Rust), child),
    ]);
    assert!(graph.edges.iter().any(|e| e.kind == EdgeKind::Instantiates));
    assert!(graph.edges.iter().any(|e| e.kind == EdgeKind::Implements));
}

pub fn create_service() -> Service {
    Service { name: String::from("test") }
}
"#;
    let code_b = r#"
use crate::service::Service;

pub fn execute(s: Service) {
    println!("{}", s.name);
}
"#;
    let pf_a = parse_file("src/service.rs", GraphiaLanguage::Rust, code_a);
    let pf_b = parse_file("src/main.rs", GraphiaLanguage::Rust, code_b);

    let files = vec![
        (
            "src/service.rs".to_string(),
            Some(GraphiaLanguage::Rust),
            pf_a,
        ),
        ("src/main.rs".to_string(), Some(GraphiaLanguage::Rust), pf_b),
    ];

    let graph = build_graph(files);
    assert!(!graph.nodes.is_empty(), "nodes should exist");
    assert!(!graph.edges.is_empty(), "edges should exist");

    let has_exports = graph.edges.iter().any(|e| e.kind == EdgeKind::Exports);
    let has_type_refs = graph
        .edges
        .iter()
        .any(|e| e.kind == EdgeKind::TypeReferences || e.kind == EdgeKind::References);
    assert!(has_exports, "graph should contain Exports edges");
    assert!(
        has_type_refs,
        "graph should contain References or TypeReferences edges"
    );
}
