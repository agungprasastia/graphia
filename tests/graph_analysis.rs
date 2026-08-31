use graphia::analysis::{
    AnalysisLevel, AnalysisOptions, CommunityConfig, CycleConfig, PageRankConfig,
    compute_centrality, compute_coupling, compute_hotspots, detect_communities, find_cycles,
    project_graph, run_analysis, tarjan_scc,
};
use graphia::graph::Graph;
use graphia::model::{Confidence, Edge, EdgeId, EdgeKind, Node, NodeId, NodeKind, SourceLocation};

fn make_node(id: u64, name: &str, file: &str) -> Node {
    Node {
        id: NodeId(id),
        kind: NodeKind::Function,
        name: name.to_string(),
        qualified_name: format!("{file}::{name}"),
        file: file.to_string(),
        location: SourceLocation {
            file: file.to_string(),
            start_line: 1,
            start_col: 1,
            end_line: 1,
            end_col: 1,
        },
        language: None,
    }
}

fn make_edge(id: u64, from: u64, to: u64, kind: EdgeKind) -> Edge {
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
fn test_scc_and_cycles_on_dag() {
    // A -> B -> C, A -> C (DAG / diamond half)
    let nodes = vec![
        make_node(1, "A", "a.rs"),
        make_node(2, "B", "b.rs"),
        make_node(3, "C", "c.rs"),
    ];
    let edges = vec![
        make_edge(10, 1, 2, EdgeKind::Calls),
        make_edge(11, 2, 3, EdgeKind::Calls),
        make_edge(12, 1, 3, EdgeKind::Calls),
    ];
    let graph = Graph::new(nodes, edges);
    let projected = project_graph(&graph, AnalysisLevel::Symbol, None);
    let adj = projected.to_adjacency();

    let sccs = tarjan_scc(&adj);
    // All SCCs in DAG should be size 1 and trivial
    assert_eq!(sccs.len(), 3);
    assert!(sccs.iter().all(|s| s.is_trivial && s.size == 1));

    let cycles = find_cycles(&adj, CycleConfig::default());
    assert_eq!(cycles.len(), 0);
}

#[test]
fn test_scc_and_cycles_on_simple_cycle() {
    // A -> B -> C -> A
    let nodes = vec![
        make_node(1, "A", "a.rs"),
        make_node(2, "B", "b.rs"),
        make_node(3, "C", "c.rs"),
    ];
    let edges = vec![
        make_edge(10, 1, 2, EdgeKind::Calls),
        make_edge(11, 2, 3, EdgeKind::Calls),
        make_edge(12, 3, 1, EdgeKind::Calls),
    ];
    let graph = Graph::new(nodes, edges);
    let projected = project_graph(&graph, AnalysisLevel::Symbol, None);
    let adj = projected.to_adjacency();

    let sccs = tarjan_scc(&adj);
    assert_eq!(sccs.len(), 1);
    assert_eq!(sccs[0].size, 3);
    assert!(!sccs[0].is_trivial);

    let cycles = find_cycles(&adj, CycleConfig::default());
    assert_eq!(cycles.len(), 1);
    assert_eq!(cycles[0].length, 3);
    assert_eq!(cycles[0].path, vec!["a.rs::A", "b.rs::B", "c.rs::C"]);
}

#[test]
fn test_multiple_disjoint_sccs_and_self_loops() {
    // SCC 1: A <-> B
    // SCC 2: C <-> D
    // E with self loop
    let nodes = vec![
        make_node(1, "A", "a.rs"),
        make_node(2, "B", "b.rs"),
        make_node(3, "C", "c.rs"),
        make_node(4, "D", "d.rs"),
        make_node(5, "E", "e.rs"),
    ];
    let edges = vec![
        make_edge(10, 1, 2, EdgeKind::Calls),
        make_edge(11, 2, 1, EdgeKind::Calls),
        make_edge(12, 3, 4, EdgeKind::Calls),
        make_edge(13, 4, 3, EdgeKind::Calls),
        make_edge(14, 5, 5, EdgeKind::Calls),
    ];
    let graph = Graph::new(nodes, edges);
    let projected = project_graph(&graph, AnalysisLevel::Symbol, None);
    let adj = projected.to_adjacency();

    let sccs = tarjan_scc(&adj);
    assert_eq!(sccs.len(), 3);
    assert_eq!(sccs[0].size, 2);
    assert_eq!(sccs[1].size, 2);
    assert_eq!(sccs[2].size, 1);
    assert!(!sccs[2].is_trivial); // E has self-loop, so non-trivial

    let cycles_no_self = find_cycles(
        &adj,
        CycleConfig {
            include_self_loops: false,
            ..Default::default()
        },
    );
    assert_eq!(cycles_no_self.len(), 2);

    let cycles_with_self = find_cycles(
        &adj,
        CycleConfig {
            include_self_loops: true,
            ..Default::default()
        },
    );
    assert_eq!(cycles_with_self.len(), 3);
}

#[test]
fn test_centrality_and_pagerank_on_star_graph() {
    // Center Hub (C) with 4 spokes pointing to it: S1 -> C, S2 -> C, S3 -> C, S4 -> C
    let nodes = vec![
        make_node(1, "C", "c.rs"),
        make_node(2, "S1", "s1.rs"),
        make_node(3, "S2", "s2.rs"),
        make_node(4, "S3", "s3.rs"),
        make_node(5, "S4", "s4.rs"),
    ];
    let edges = vec![
        make_edge(10, 2, 1, EdgeKind::Calls),
        make_edge(11, 3, 1, EdgeKind::Calls),
        make_edge(12, 4, 1, EdgeKind::Calls),
        make_edge(13, 5, 1, EdgeKind::Calls),
    ];
    let graph = Graph::new(nodes, edges);
    let projected = project_graph(&graph, AnalysisLevel::Symbol, None);
    let adj = projected.to_adjacency();

    let centrality = compute_centrality(&adj, PageRankConfig::default());
    // Hub C should have highest in_degree (4) and highest PageRank
    assert_eq!(centrality[0].id, "c.rs::C");
    assert_eq!(centrality[0].in_degree, 4);
    assert_eq!(centrality[0].out_degree, 0);

    for s in &centrality[1..] {
        assert_eq!(s.in_degree, 0);
        assert_eq!(s.out_degree, 1);
        assert!(s.pagerank < centrality[0].pagerank);
    }
}

#[test]
fn test_coupling_and_instability_metrics() {
    // Service (S): depends on DB and Logger (Ce = 2, Ca = 0) -> I = 2 / (2+0) = 1.0 (maximally unstable)
    // DB: depended on by Service and Auth (Ce = 0, Ca = 2) -> I = 0 / (2+0) = 0.0 (maximally stable)
    // Isolated (X): Ce = 0, Ca = 0 -> I = 0.0 (guarded)
    let nodes = vec![
        make_node(1, "Service", "service.rs"),
        make_node(2, "DB", "db.rs"),
        make_node(3, "Logger", "logger.rs"),
        make_node(4, "Auth", "auth.rs"),
        make_node(5, "Isolated", "iso.rs"),
    ];
    let edges = vec![
        make_edge(10, 1, 2, EdgeKind::Calls),
        make_edge(11, 1, 3, EdgeKind::Calls),
        make_edge(12, 4, 2, EdgeKind::Calls),
    ];
    let graph = Graph::new(nodes, edges);
    let projected = project_graph(&graph, AnalysisLevel::Symbol, None);
    let adj = projected.to_adjacency();

    let metrics = compute_coupling(&adj);
    let mut metric_map = std::collections::HashMap::new();
    for m in metrics {
        metric_map.insert(m.id.clone(), m);
    }

    let s = &metric_map["service.rs::Service"];
    assert_eq!(s.afferent_coupling, 0);
    assert_eq!(s.efferent_coupling, 2);
    assert!((s.instability - 1.0).abs() < 1e-6);

    let db = &metric_map["db.rs::DB"];
    assert_eq!(db.afferent_coupling, 2);
    assert_eq!(db.efferent_coupling, 0);
    assert!((db.instability - 0.0).abs() < 1e-6);

    let iso = &metric_map["iso.rs::Isolated"];
    assert_eq!(iso.afferent_coupling, 0);
    assert_eq!(iso.efferent_coupling, 0);
    assert_eq!(iso.instability, 0.0);
}

#[test]
fn test_hotspot_scoring() {
    // Critical core node in cycle with high in-degree vs leaf node
    let nodes = vec![
        make_node(1, "CoreA", "core.rs"),
        make_node(2, "CoreB", "core.rs"),
        make_node(3, "Caller1", "c1.rs"),
        make_node(4, "Caller2", "c2.rs"),
        make_node(5, "Leaf", "leaf.rs"),
    ];
    let edges = vec![
        make_edge(10, 1, 2, EdgeKind::Calls),
        make_edge(11, 2, 1, EdgeKind::Calls),
        make_edge(12, 3, 1, EdgeKind::Calls),
        make_edge(13, 4, 1, EdgeKind::Calls),
        make_edge(14, 1, 5, EdgeKind::Calls),
    ];
    let graph = Graph::new(nodes, edges);
    let projected = project_graph(&graph, AnalysisLevel::Symbol, None);
    let adj = projected.to_adjacency();

    let hotspots = compute_hotspots(&adj);
    // CoreA is in SCC and has in_degree = 3 (CoreB, Caller1, Caller2) -> Highest Hotspot
    assert_eq!(hotspots[0].id, "core.rs::CoreA");
    assert!(hotspots[0].in_scc);
    assert!(hotspots[0].score > hotspots.last().unwrap().score);
}

#[test]
fn test_deterministic_community_detection() {
    // Two tight clusters connected by a bridge edge:
    // Cluster 1: {1, 2, 3} all connected
    // Cluster 2: {4, 5, 6} all connected
    // Bridge: 3 -> 4
    let nodes = vec![
        make_node(1, "C1_A", "c1.rs"),
        make_node(2, "C1_B", "c1.rs"),
        make_node(3, "C1_C", "c1.rs"),
        make_node(4, "C2_A", "c2.rs"),
        make_node(5, "C2_B", "c2.rs"),
        make_node(6, "C2_C", "c2.rs"),
    ];
    let edges = vec![
        // Cluster 1
        make_edge(10, 1, 2, EdgeKind::Calls),
        make_edge(11, 2, 3, EdgeKind::Calls),
        make_edge(12, 3, 1, EdgeKind::Calls),
        // Cluster 2
        make_edge(20, 4, 5, EdgeKind::Calls),
        make_edge(21, 5, 6, EdgeKind::Calls),
        make_edge(22, 6, 4, EdgeKind::Calls),
        // Bridge
        make_edge(30, 3, 4, EdgeKind::Calls),
    ];
    let graph = Graph::new(nodes, edges);
    let projected = project_graph(&graph, AnalysisLevel::Symbol, None);
    let adj = projected.to_adjacency();

    let comms = detect_communities(&adj, CommunityConfig::default());
    assert_eq!(comms.len(), 2);
    assert_eq!(comms[0].size, 3);
    assert_eq!(comms[1].size, 3);

    // Verify determinism over 10 repeated runs
    for _ in 0..10 {
        let repeated = detect_communities(&adj, CommunityConfig::default());
        assert_eq!(comms, repeated);
    }
}

#[test]
fn test_projection_levels_symbol_file_module() {
    let nodes = vec![
        make_node(1, "func_a", "core/sub_a/file_a.rs"),
        make_node(2, "func_b", "core/sub_a/file_a.rs"),
        make_node(3, "func_c", "core/sub_b/file_b.rs"),
        make_node(4, "func_d", "infra/net/file_c.rs"),
    ];
    let edges = vec![
        make_edge(10, 1, 2, EdgeKind::Calls),
        make_edge(11, 2, 3, EdgeKind::Calls),
        make_edge(12, 3, 4, EdgeKind::Calls),
    ];
    let graph = Graph::new(nodes, edges);

    // Symbol Level
    let sym_proj = project_graph(&graph, AnalysisLevel::Symbol, None);
    assert_eq!(sym_proj.nodes.len(), 4);
    assert_eq!(sym_proj.edges.len(), 3);

    // File Level
    let file_proj = project_graph(&graph, AnalysisLevel::File, None);
    assert_eq!(file_proj.nodes.len(), 3); // file_a.rs, file_b.rs, file_c.rs

    // Module Level
    let mod_proj = project_graph(&graph, AnalysisLevel::Module, None);
    assert_eq!(mod_proj.nodes.len(), 3); // core/sub_a, core/sub_b, infra/net
}

#[test]
fn test_orchestrator_run_analysis_and_serialization() {
    let nodes = vec![make_node(1, "A", "pkg/a.rs"), make_node(2, "B", "pkg/b.rs")];
    let edges = vec![
        make_edge(10, 1, 2, EdgeKind::Calls),
        make_edge(11, 2, 1, EdgeKind::Calls),
    ];
    let graph = Graph::new(nodes, edges);

    let report = run_analysis(
        &graph,
        AnalysisOptions {
            level: AnalysisLevel::File,
            edge_filter: Some(EdgeKind::Calls),
            limit: Some(10),
        },
    );

    assert_eq!(report.analysis_version, 1);
    assert_eq!(report.node_count, 2);
    assert_eq!(report.cycles.len(), 1);

    // Verify JSON round-trip
    let json_str = serde_json::to_string(&report).expect("serialize report");
    let deserialized: graphia::analysis::AnalysisReport =
        serde_json::from_str(&json_str).expect("deserialize report");
    assert_eq!(report.analysis_version, deserialized.analysis_version);
    assert_eq!(report.level, deserialized.level);
    assert_eq!(report.node_count, deserialized.node_count);
    assert_eq!(report.edge_count, deserialized.edge_count);
    assert_eq!(report.sccs, deserialized.sccs);
    assert_eq!(report.cycles, deserialized.cycles);
    assert_eq!(report.communities, deserialized.communities);
    assert_eq!(report.centrality.len(), deserialized.centrality.len());
    for (a, b) in report.centrality.iter().zip(deserialized.centrality.iter()) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.in_degree, b.in_degree);
        assert_eq!(a.out_degree, b.out_degree);
        assert!((a.pagerank - b.pagerank).abs() < 1e-6);
    }
    for (a, b) in report.hotspots.iter().zip(deserialized.hotspots.iter()) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.fan_in, b.fan_in);
        assert_eq!(a.fan_out, b.fan_out);
        assert!((a.score - b.score).abs() < 1e-6);
    }
}
