use serde::{Deserialize, Serialize};

use super::centrality::{PageRankConfig, compute_centrality};
use super::coupling::compute_coupling;
use super::projection::AdjacencyGraph;
use super::scc::tarjan_scc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Hotspot {
    pub id: String,
    pub name: String,
    pub score: f64,
    pub fan_in: usize,
    pub fan_out: usize,
    pub pagerank: f64,
    pub in_scc: bool,
    pub scc_size: usize,
}

#[must_use]
pub fn compute_hotspots(graph: &AdjacencyGraph) -> Vec<Hotspot> {
    let n = graph.nodes.len();
    if n == 0 {
        return Vec::new();
    }

    let centrality = compute_centrality(graph, PageRankConfig::default());
    let coupling = compute_coupling(graph);
    let sccs = tarjan_scc(graph);

    let mut centrality_map = std::collections::HashMap::new();
    for c in centrality {
        centrality_map.insert(c.id.clone(), c);
    }

    let mut coupling_map = std::collections::HashMap::new();
    for cp in coupling {
        coupling_map.insert(cp.id.clone(), cp);
    }

    let mut scc_info = std::collections::HashMap::new();
    for scc in &sccs {
        let is_non_trivial = !scc.is_trivial;
        for member in &scc.members {
            scc_info.insert(member.clone(), (is_non_trivial, scc.size));
        }
    }

    let mut hotspots = Vec::with_capacity(n);

    for node in &graph.nodes {
        let c =
            centrality_map
                .get(&node.id)
                .cloned()
                .unwrap_or(super::centrality::NodeCentrality {
                    id: node.id.clone(),
                    in_degree: 0,
                    out_degree: 0,
                    degree: 0,
                    pagerank: 0.0,
                });
        let cp = coupling_map
            .get(&node.id)
            .cloned()
            .unwrap_or(super::coupling::CouplingMetrics {
                id: node.id.clone(),
                afferent_coupling: 0,
                efferent_coupling: 0,
                fan_in: 0,
                fan_out: 0,
                instability: 0.0,
            });
        let (in_scc, scc_size) = scc_info.get(&node.id).copied().unwrap_or((false, 1));

        // Hotspot score formula:
        // Combined centrality, fan-in/fan-out stress, and cycle penalty
        // score = (fan_in * 2.0 + fan_out * 1.0) * (1.0 + pagerank * 10.0) * (if in_scc { 1.5 + (scc_size as f64) * 0.1 } else { 1.0 })
        let base_structural = (cp.fan_in as f64 * 2.0) + (cp.fan_out as f64 * 1.0);
        let pr_multiplier = 1.0 + (c.pagerank * 10.0);
        let scc_multiplier = if in_scc {
            1.5 + (scc_size as f64 * 0.1)
        } else {
            1.0
        };

        let score = base_structural * pr_multiplier * scc_multiplier;

        hotspots.push(Hotspot {
            id: node.id.clone(),
            name: node.name.clone(),
            score,
            fan_in: cp.fan_in,
            fan_out: cp.fan_out,
            pagerank: c.pagerank,
            in_scc,
            scc_size,
        });
    }

    hotspots.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.fan_in.cmp(&a.fan_in))
            .then_with(|| b.fan_out.cmp(&a.fan_out))
            .then_with(|| a.id.cmp(&b.id))
    });

    hotspots
}
