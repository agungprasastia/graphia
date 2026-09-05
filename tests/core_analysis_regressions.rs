use std::fs;

use graphia::analysis::centrality::compute_pagerank;
use graphia::analysis::{
    AnalysisLevel, CommunityConfig, PageRankConfig, ProjectedEdge, ProjectedGraph, ProjectedNode,
    detect_communities, project_graph,
};
use graphia::daemon::debounce::SemanticAction;
use graphia::graph::Graph;
use graphia::incremental::IncrementalWorkspace;
use graphia::model::{Confidence, Edge, EdgeId, EdgeKind, Node, NodeId, NodeKind, SourceLocation};
use tempfile::tempdir;

fn node(id: u64, qualified_name: &str, file: &str) -> Node {
    Node {
        id: NodeId(id),
        kind: NodeKind::Function,
        name: qualified_name.to_string(),
        qualified_name: qualified_name.to_string(),
        file: file.to_string(),
        location: SourceLocation {
            file: file.to_string(),
            start_line: 1,
            start_col: 1,
            end_line: 1,
            end_col: 1,
        },
        language: None,
        visibility: graphia::model::Visibility::Unknown,
        signature: None,
        container: None,
    }
}

fn projected_node(id: &str) -> ProjectedNode {
    ProjectedNode {
        id: id.to_string(),
        name: id.to_string(),
        level: AnalysisLevel::Symbol,
        member_count: 1,
    }
}

#[test]
fn symbol_projection_disambiguates_equal_qualified_names() {
    let nodes = vec![
        node(1, "helper", "a.rs"),
        node(2, "helper", "b.rs"),
        node(3, "helper#1", "c.rs"),
    ];
    let edge = Edge {
        id: EdgeId(10),
        kind: EdgeKind::Calls,
        from: NodeId(1),
        to: NodeId(2),
        confidence: Confidence::Extracted,
        label: None,
    };

    let projected = project_graph(&Graph::new(nodes, vec![edge]), AnalysisLevel::Symbol, None);

    assert_eq!(projected.nodes.len(), 3);
    let ids: std::collections::HashSet<_> = projected.nodes.iter().map(|node| &node.id).collect();
    assert_eq!(ids.len(), 3);
    assert_eq!(projected.edges.len(), 1);
    assert_ne!(projected.edges[0].from, projected.edges[0].to);
}

#[test]
fn community_detection_does_not_oscillate_on_two_nodes() {
    let graph = ProjectedGraph {
        level: AnalysisLevel::Symbol,
        nodes: vec![projected_node("a"), projected_node("b")],
        edges: vec![ProjectedEdge {
            from: "a".into(),
            to: "b".into(),
            weight: 1,
            kinds: vec![EdgeKind::Calls],
        }],
    }
    .to_adjacency();

    for max_iterations in [1, 2, 3] {
        let communities = detect_communities(&graph, CommunityConfig { max_iterations });
        assert_eq!(communities.len(), 1);
        assert_eq!(communities[0].members, vec!["a", "b"]);
    }
}

#[test]
fn pagerank_respects_projected_edge_weights() {
    let graph = ProjectedGraph {
        level: AnalysisLevel::Symbol,
        nodes: vec![
            projected_node("a"),
            projected_node("b"),
            projected_node("c"),
        ],
        edges: vec![
            ProjectedEdge {
                from: "a".into(),
                to: "b".into(),
                weight: 1,
                kinds: vec![EdgeKind::Calls],
            },
            ProjectedEdge {
                from: "a".into(),
                to: "c".into(),
                weight: 3,
                kinds: vec![EdgeKind::Calls],
            },
        ],
    }
    .to_adjacency();

    let ranks = compute_pagerank(&graph, PageRankConfig::default());
    assert!(ranks[2] > ranks[1]);
}

#[test]
fn incremental_summary_counts_replaced_component_records() {
    let temp = tempdir().expect("tempdir");
    let source = temp.path().join("lib.rs");
    fs::write(&source, "pub fn one() {}\n").expect("write source");
    let mut workspace = IncrementalWorkspace::new(temp.path().to_path_buf()).expect("workspace");
    let old_nodes = workspace.graph.nodes.len();
    let old_edges = workspace.graph.edges.len();

    fs::write(&source, "pub fn one() {}\npub fn two() {}\n").expect("modify source");
    let summary = workspace
        .apply_changes_selective(&[SemanticAction::Modified(source)])
        .expect("incremental update");

    assert_eq!(summary.nodes_removed, old_nodes);
    assert_eq!(summary.nodes_added, workspace.graph.nodes.len());
    assert_eq!(summary.edges_removed, old_edges);
    assert_eq!(summary.edges_added, workspace.graph.edges.len());
}
