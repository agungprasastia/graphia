use serde::{Deserialize, Serialize};

use super::projection::AdjacencyGraph;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NodeCentrality {
    pub id: String,
    pub in_degree: usize,
    pub out_degree: usize,
    pub degree: usize,
    pub pagerank: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct PageRankConfig {
    pub damping: f64,
    pub max_iterations: usize,
    pub tolerance: f64,
}

impl Default for PageRankConfig {
    fn default() -> Self {
        Self {
            damping: 0.85,
            max_iterations: 100,
            tolerance: 1e-6,
        }
    }
}

#[must_use]
pub fn compute_centrality(
    graph: &AdjacencyGraph,
    pagerank_config: PageRankConfig,
) -> Vec<NodeCentrality> {
    let n = graph.nodes.len();
    if n == 0 {
        return Vec::new();
    }

    let pr = compute_pagerank(graph, pagerank_config);

    let mut result = Vec::with_capacity(n);
    for (i, &pagerank) in pr.iter().enumerate() {
        let in_deg = graph.incoming[i].len();
        let out_deg = graph.outgoing[i].len();
        result.push(NodeCentrality {
            id: graph.nodes[i].id.clone(),
            in_degree: in_deg,
            out_degree: out_deg,
            degree: in_deg + out_deg,
            pagerank,
        });
    }

    result.sort_by(|a, b| {
        b.pagerank
            .partial_cmp(&a.pagerank)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.degree.cmp(&a.degree))
            .then_with(|| a.id.cmp(&b.id))
    });

    result
}

#[must_use]
pub fn compute_pagerank(graph: &AdjacencyGraph, config: PageRankConfig) -> Vec<f64> {
    let n = graph.nodes.len();
    if n == 0 {
        return Vec::new();
    }

    let inv_n = 1.0 / (n as f64);
    let mut ranks = vec![inv_n; n];
    let mut next_ranks = vec![0.0; n];

    for _ in 0..config.max_iterations {
        // Collect dangling weight (nodes with out_degree = 0)
        let mut dangling_sum = 0.0;
        for (i, &rank) in ranks.iter().enumerate() {
            if graph.outgoing[i].is_empty() {
                dangling_sum += rank;
            }
        }

        let base_score = (1.0 - config.damping) * inv_n + config.damping * (dangling_sum * inv_n);

        for (j, next_rank) in next_ranks.iter_mut().enumerate() {
            let mut sum_in = 0.0;
            for &(source, _) in &graph.incoming[j] {
                let out_deg = graph.outgoing[source].len();
                if out_deg > 0 {
                    sum_in += ranks[source] / (out_deg as f64);
                }
            }
            *next_rank = base_score + config.damping * sum_in;
        }

        // Check L1 convergence
        let mut diff = 0.0;
        for i in 0..n {
            diff += (next_ranks[i] - ranks[i]).abs();
        }

        ranks.copy_from_slice(&next_ranks);

        if diff < config.tolerance {
            break;
        }
    }

    ranks
}
