use serde::{Deserialize, Serialize};

use super::entrypoints::{Entrypoint, detect_entrypoints};
use crate::analysis::{
    AnalysisLevel, Community, CommunityConfig, Cycle, CycleConfig, NodeCentrality, PageRankConfig,
    compute_centrality, detect_communities, find_cycles, project_graph,
};
use crate::graph::Graph;
use crate::model::NodeKind;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DependencyDirection {
    pub from: String,
    pub to: String,
    pub weight: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArchitectureOverview {
    pub total_nodes: usize,
    pub total_edges: usize,
    pub module_count: usize,
    pub file_count: usize,
    pub symbol_count: usize,
    pub entrypoints: Vec<Entrypoint>,
    pub dependency_direction: Vec<DependencyDirection>,
    pub high_centrality_modules: Vec<NodeCentrality>,
    pub cycle_count: usize,
    pub cycles: Vec<Cycle>,
    pub communities: Vec<Community>,
}

#[must_use]
pub fn get_architecture_overview(graph: &Graph) -> ArchitectureOverview {
    let mut file_count = 0;
    let mut symbol_count = 0;
    for n in &graph.nodes {
        if n.kind == NodeKind::File {
            file_count += 1;
        } else {
            symbol_count += 1;
        }
    }

    let mod_projected = project_graph(graph, AnalysisLevel::Module, None);
    let mod_adj = mod_projected.to_adjacency();
    let module_count = mod_adj.node_count();

    let entrypoints = detect_entrypoints(graph);

    let mut dependency_direction: Vec<DependencyDirection> = mod_projected
        .edges
        .iter()
        .map(|e| DependencyDirection {
            from: e.from.clone(),
            to: e.to.clone(),
            weight: e.weight,
        })
        .collect();

    dependency_direction.sort_by(|a, b| {
        b.weight
            .cmp(&a.weight)
            .then_with(|| a.from.cmp(&b.from))
            .then_with(|| a.to.cmp(&b.to))
    });

    let centrality = compute_centrality(&mod_adj, PageRankConfig::default());
    let mut high_centrality_modules = centrality;
    high_centrality_modules.truncate(10);

    let cycles = find_cycles(&mod_adj, CycleConfig::default());
    let cycle_count = cycles.len();

    let communities = detect_communities(&mod_adj, CommunityConfig::default());

    ArchitectureOverview {
        total_nodes: graph.node_count(),
        total_edges: graph.edge_count(),
        module_count,
        file_count,
        symbol_count,
        entrypoints,
        dependency_direction,
        high_centrality_modules,
        cycle_count,
        cycles,
        communities,
    }
}
