use graphia::graph::Graph;
use graphia::model::{Confidence, Edge, EdgeId, EdgeKind, Node, NodeId, NodeKind, SourceLocation};
use graphia::query::{QueryIndex, TraversalError, TraversalLimits};

fn node(id: u64, name: &str, file: &str) -> Node {
    Node {
        id: NodeId(id),
        kind: NodeKind::Function,
        name: name.to_string(),
        qualified_name: format!("{file}::{name}"),
        file: file.to_string(),
        location: SourceLocation {
            file: file.to_string(),
            start_line: 2,
            start_col: 3,
            end_line: 2,
            end_col: 8,
        },
        language: None,
    }
}

fn edge(id: u64, kind: EdgeKind, from: u64, to: u64) -> Edge {
    Edge {
        id: EdgeId(id),
        kind,
        from: NodeId(from),
        to: NodeId(to),
        confidence: Confidence::Extracted,
        label: None,
    }
}

#[test]
fn query_supports_relationship_kinds_and_edge_paths() {
    let graph = Graph::new(
        vec![
            node(10, "caller", "src/a.rs"),
            node(20, "target", "src/b.rs"),
        ],
        vec![edge(30, EdgeKind::Calls, 10, 20)],
    );
    let index = QueryIndex::new(&graph);
    assert_eq!(index.find(&graph, "src/b.rs::target").len(), 1);
    assert_eq!(index.outgoing(NodeId(10)), &[NodeId(20)]);
    let explanation = index.explain(&graph, NodeId(20)).expect("node exists");
    assert_eq!(explanation.callers, vec![NodeId(10)]);
    assert_eq!(
        index.shortest_path(NodeId(10), NodeId(20), TraversalLimits::new(1, 2)),
        Ok(Some(vec![EdgeId(30)]))
    );
}

#[test]
fn lookup_unions_exact_indexes_and_handles_partial_results() {
    let graph = Graph::new(
        vec![
            node(10, "shared", "shared"),
            node(20, "other", "shared"),
            node(30, "shared", "other.rs"),
        ],
        vec![],
    );
    let index = QueryIndex::new(&graph);
    let exact: Vec<_> = index
        .find(&graph, "shared")
        .iter()
        .map(|node| node.id)
        .collect();
    assert_eq!(exact, vec![NodeId(10), NodeId(20), NodeId(30)]);
    assert_eq!(index.find(&graph, "missing").len(), 0);
    assert_eq!(
        index
            .find(&graph, "har")
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        vec![NodeId(10), NodeId(20), NodeId(30)]
    );
}

#[test]
fn explain_reports_all_relationship_categories() {
    let graph = Graph::new(
        vec![
            node(1, "parent", "a.rs"),
            node(2, "target", "a.rs"),
            node(3, "caller", "b.rs"),
            node(4, "callee", "c.rs"),
            node(5, "module", "d.rs"),
            node(6, "other_parent", "e.rs"),
        ],
        vec![
            edge(1, EdgeKind::Contains, 1, 2),
            edge(0, EdgeKind::Contains, 6, 2),
            edge(2, EdgeKind::Calls, 3, 2),
            edge(3, EdgeKind::Calls, 2, 4),
            edge(4, EdgeKind::Imports, 2, 5),
        ],
    );
    let explanation = QueryIndex::new(&graph)
        .explain(&graph, NodeId(2))
        .expect("target exists");
    assert_eq!(explanation.kind, NodeKind::Function);
    assert_eq!(explanation.location, "a.rs:2:3");
    assert_eq!(explanation.parent, Some(NodeId(1)));
    assert_eq!(explanation.incoming, vec![NodeId(1), NodeId(3), NodeId(6)]);
    assert_eq!(explanation.outgoing, vec![NodeId(4), NodeId(5)]);
    assert_eq!(explanation.callers, vec![NodeId(3)]);
    assert_eq!(explanation.callees, vec![NodeId(4)]);
    assert_eq!(explanation.imports, vec![NodeId(5)]);
}

#[test]
fn query_path_limits_protect_cycles_and_unknown_nodes() {
    let graph = Graph::new(
        vec![
            node(10, "a", "a.rs"),
            node(20, "b", "b.rs"),
            node(30, "c", "c.rs"),
        ],
        vec![
            edge(1, EdgeKind::Calls, 10, 20),
            edge(2, EdgeKind::Calls, 20, 10),
            edge(3, EdgeKind::Calls, 20, 30),
        ],
    );
    let index = QueryIndex::new(&graph);
    assert_eq!(
        index.shortest_path(NodeId(10), NodeId(30), TraversalLimits::new(10, 3)),
        Ok(Some(vec![EdgeId(1), EdgeId(3)]))
    );
    assert!(
        index
            .shortest_path(NodeId(10), NodeId(30), TraversalLimits::new(1, 3))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        index.shortest_path(NodeId(10), NodeId(30), TraversalLimits::new(2, 2)),
        Err(TraversalError {
            visited: 3,
            limit: 2
        })
    );
    assert_eq!(
        index.shortest_path(NodeId(10), NodeId(30), TraversalLimits::new(2, 3)),
        Ok(Some(vec![EdgeId(1), EdgeId(3)]))
    );
    assert!(
        index
            .shortest_path(NodeId(999), NodeId(30), TraversalLimits::new(1, 1))
            .unwrap()
            .is_none()
    );
    assert!(
        index
            .shortest_path(NodeId(10), NodeId(999), TraversalLimits::new(1, 1))
            .unwrap()
            .is_none()
    );
}
