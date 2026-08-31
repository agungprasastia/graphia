pub mod advanced;
pub mod centrality;
pub mod communities;
pub mod coupling;
pub mod cycles;
pub mod hotspots;
pub mod projection;
pub mod scc;

use serde::{Deserialize, Serialize};

pub use centrality::{NodeCentrality, PageRankConfig, compute_centrality};
pub use communities::{Community, CommunityConfig, detect_communities};
pub use coupling::{CouplingMetrics, compute_coupling};
pub use cycles::{Cycle, CycleConfig, find_cycles};
pub use hotspots::{Hotspot, compute_hotspots};
pub use projection::{
    AdjacencyGraph, AnalysisLevel, ProjectedEdge, ProjectedGraph, ProjectedNode, project_graph,
};
pub use scc::{StronglyConnectedComponent, tarjan_scc};

use crate::graph::Graph;
use crate::model::EdgeKind;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnalysisReport {
    pub analysis_version: u32,
    pub level: AnalysisLevel,
    pub node_count: usize,
    pub edge_count: usize,
    pub sccs: Vec<StronglyConnectedComponent>,
    pub cycles: Vec<Cycle>,
    pub centrality: Vec<NodeCentrality>,
    pub coupling: Vec<CouplingMetrics>,
    pub hotspots: Vec<Hotspot>,
    pub communities: Vec<Community>,
}

#[derive(Debug, Clone, Copy)]
pub struct AnalysisOptions {
    pub level: AnalysisLevel,
    pub edge_filter: Option<EdgeKind>,
    pub limit: Option<usize>,
}

impl Default for AnalysisOptions {
    fn default() -> Self {
        Self {
            level: AnalysisLevel::File,
            edge_filter: None,
            limit: None,
        }
    }
}

#[must_use]
pub fn run_analysis(graph: &Graph, options: AnalysisOptions) -> AnalysisReport {
    let projected = project_graph(graph, options.level, options.edge_filter);
    let adj = projected.to_adjacency();

    let mut sccs = tarjan_scc(&adj);
    let mut cycles = find_cycles(&adj, CycleConfig::default());
    let mut centrality = compute_centrality(&adj, PageRankConfig::default());
    let mut coupling = compute_coupling(&adj);
    let mut hotspots = compute_hotspots(&adj);
    let mut communities = detect_communities(&adj, CommunityConfig::default());

    if let Some(limit) = options.limit {
        sccs.truncate(limit);
        cycles.truncate(limit);
        centrality.truncate(limit);
        coupling.truncate(limit);
        hotspots.truncate(limit);
        communities.truncate(limit);
    }

    AnalysisReport {
        analysis_version: 1,
        level: options.level,
        node_count: adj.node_count(),
        edge_count: adj.edge_count(),
        sccs,
        cycles,
        centrality,
        coupling,
        hotspots,
        communities,
    }
}
