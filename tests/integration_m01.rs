use graphia::graph::{Graph, build_graph, stable_edge_id, stable_node_id};
use graphia::model::{
    Confidence, Edge, EdgeIdentity, EdgeKind, Language, NodeId, NodeIdentity, NodeKind,
    SourceLocation,
};
use graphia::parser::parse_file;
use graphia::parser::{Call, Import, ParsedFile, Symbol};

fn location(file: &str, line: u32) -> SourceLocation {
    SourceLocation {
        file: file.to_string(),
        start_line: line,
        start_col: 1,
        end_line: line,
        end_col: 2,
    }
}

fn function(file: &str, name: &str, line: u32) -> Symbol {
    Symbol {
        kind: NodeKind::Function,
        name: name.to_string(),
        qualified_name: format!("{file}::{name}"),
        location: location(file, line),
        parent: None,
    }
}

#[test]
fn identity_ids_ignore_absolute_root_and_input_order() {
    let identity = NodeIdentity::new(
        "src/lib.rs",
        NodeKind::Function,
        "src/lib.rs::run",
        &location("src/lib.rs", 3),
    );
    let same_identity = NodeIdentity::new(
        "src/lib.rs",
        NodeKind::Function,
        "src/lib.rs::run",
        &location("src/lib.rs", 3),
    );
    assert_eq!(stable_node_id(&identity), stable_node_id(&same_identity));
    let root_a = NodeIdentity::new(
        "C:/workspace/repo/src/lib.rs",
        NodeKind::Function,
        "src/lib.rs::run",
        &SourceLocation {
            file: "C:/workspace/repo/src/lib.rs".to_string(),
            ..location("src/lib.rs", 3)
        },
    );
    let root_b = NodeIdentity::new(
        "D:/other/repo/src/lib.rs",
        NodeKind::Function,
        "src/lib.rs::run",
        &SourceLocation {
            file: "D:/other/repo/src/lib.rs".to_string(),
            ..location("src/lib.rs", 3)
        },
    );
    assert_eq!(root_a.file, root_b.file);
    assert_eq!(root_a.location.file, root_b.location.file);
    assert_eq!(stable_node_id(&root_a), stable_node_id(&root_b));
    assert_ne!(
        stable_node_id(&identity),
        stable_node_id(&NodeIdentity::new(
            "src/lib.rs",
            NodeKind::Function,
            "src/lib.rs::other",
            &location("src/lib.rs", 4)
        ))
    );
    let edge = EdgeIdentity::new(
        NodeId(1),
        NodeId(2),
        EdgeKind::Calls,
        Confidence::Inferred,
        None,
    );
    assert_eq!(stable_edge_id(&edge), stable_edge_id(&edge));
    assert_ne!(
        stable_edge_id(&edge),
        stable_edge_id(&EdgeIdentity::new(
            NodeId(1),
            NodeId(2),
            EdgeKind::Calls,
            Confidence::Inferred,
            Some(String::new())
        ))
    );
    assert_ne!(
        stable_node_id(&identity),
        stable_node_id(&NodeIdentity::new(
            "src/lib.rs",
            NodeKind::Function,
            "src/lib.rs::run",
            &SourceLocation {
                end_line: 4,
                ..location("src/lib.rs", 3)
            }
        ))
    );
}

#[test]
fn resolver_reports_ambiguous_calls_without_fabricating_edge() {
    let caller = ParsedFile {
        symbols: vec![function("caller.rs", "run", 1)],
        imports: vec![
            Import {
                path: "first.rs".to_string(),
                location: location("caller.rs", 1),
            },
            Import {
                path: "second.rs".to_string(),
                location: location("caller.rs", 1),
            },
        ],
        calls: vec![Call {
            caller: "caller.rs::run".to_string(),
            callee: "same".to_string(),
            location: location("caller.rs", 2),
        }],
    };
    let first = ParsedFile {
        symbols: vec![function("first.rs", "same", 1)],
        imports: vec![],
        calls: vec![],
    };
    let second = ParsedFile {
        symbols: vec![function("second.rs", "same", 1)],
        imports: vec![],
        calls: vec![],
    };
    let mut graph = build_graph(vec![
        ("second.rs".to_string(), Some(Language::Rust), second),
        ("caller.rs".to_string(), Some(Language::Rust), caller),
        ("first.rs".to_string(), Some(Language::Rust), first),
    ]);
    let report = graph.resolve_cross_file().expect("resolution report");
    assert_eq!(report.ambiguous_calls, 1);
    assert!(graph.edges.iter().all(|edge| edge.kind != EdgeKind::Calls));
    graph.validate().expect("valid graph");
}

#[test]
fn resolver_resolves_imported_fixture_targets_as_inferred_edges() {
    for (caller_path, target_path, language, caller_source, target_source) in [
        (
            "rust/cross_file.rs",
            "rust/target.rs",
            Language::Rust,
            include_str!("fixtures/rust/cross_file.rs"),
            include_str!("fixtures/rust/target.rs"),
        ),
        (
            "python/cross_file.py",
            "python/target.py",
            Language::Python,
            include_str!("fixtures/python/cross_file.py"),
            include_str!("fixtures/python/target.py"),
        ),
        (
            "typescript/cross_file.ts",
            "typescript/target.ts",
            Language::TypeScript,
            include_str!("fixtures/typescript/cross_file.ts"),
            include_str!("fixtures/typescript/target.ts"),
        ),
    ] {
        let mut graph = build_graph(vec![
            (
                caller_path.to_string(),
                Some(language),
                parse_file(caller_path, language, caller_source),
            ),
            (
                target_path.to_string(),
                Some(language),
                parse_file(target_path, language, target_source),
            ),
        ]);
        let report = graph.resolve_cross_file().expect("resolution report");
        assert_eq!(report.resolved_calls, 1);
        let caller_id = graph
            .nodes
            .iter()
            .find(|node| node.qualified_name == format!("{caller_path}::caller"))
            .expect("fixture caller")
            .id;
        let target_id = graph
            .nodes
            .iter()
            .find(|node| node.qualified_name == format!("{target_path}::target"))
            .expect("fixture target")
            .id;
        let caller_file_id = graph
            .nodes
            .iter()
            .find(|node| node.qualified_name == caller_path)
            .expect("caller file")
            .id;
        let target_file_id = graph
            .nodes
            .iter()
            .find(|node| node.qualified_name == target_path)
            .expect("target file")
            .id;
        assert!(graph.edges.iter().any(|edge| edge.kind == EdgeKind::Imports
            && edge.from == caller_file_id
            && edge.to == target_file_id
            && edge.confidence == Confidence::Inferred));
        assert!(graph.edges.iter().any(|edge| edge.kind == EdgeKind::Calls
            && edge.from == caller_id
            && edge.to == target_id
            && edge.confidence == Confidence::Inferred));
    }
}

#[test]
fn graph_validation_rejects_dangling_endpoints() {
    let graph = Graph::new(
        vec![],
        vec![Edge {
            id: graphia::model::EdgeId(1),
            kind: EdgeKind::Calls,
            from: NodeId(1),
            to: NodeId(2),
            confidence: Confidence::Inferred,
            label: None,
        }],
    );
    assert!(graph.validate().is_err());
}
