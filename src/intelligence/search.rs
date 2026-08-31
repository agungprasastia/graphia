use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::relevance::{RelevanceScore, score_relevance};
use crate::analysis::{PageRankConfig, compute_centrality, project_graph};
use crate::graph::Graph;
use crate::model::{Node, NodeKind};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub node: Node,
    pub score: RelevanceScore,
}

#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    pub query: String,
    pub kind_filter: Option<NodeKind>,
    pub file_filter: Option<String>,
    pub limit: Option<usize>,
}

#[must_use]
pub fn search_graph(graph: &Graph, options: &SearchOptions) -> Vec<SearchResult> {
    if options.query.trim().is_empty() {
        return Vec::new();
    }

    let projected = project_graph(graph, crate::analysis::AnalysisLevel::Symbol, None);
    let adj = projected.to_adjacency();
    let centrality_list = compute_centrality(&adj, PageRankConfig::default());
    let centrality_map: HashMap<String, f64> = centrality_list
        .into_iter()
        .map(|c| (c.id, c.pagerank))
        .collect();

    let mut results: Vec<SearchResult> = graph
        .nodes
        .iter()
        .filter(|node| {
            if let Some(kind) = options.kind_filter {
                if node.kind != kind {
                    return false;
                }
            }
            if let Some(ref file) = options.file_filter {
                if !node.file.contains(file) {
                    return false;
                }
            }
            true
        })
        .filter_map(|node| {
            let centrality = centrality_map
                .get(&node.qualified_name)
                .copied()
                .unwrap_or(0.0);
            let score = score_relevance(node, &options.query, centrality);

            // Keep if any lexical or path match was found
            let has_match = score.signals.exact_qualified_match
                || score.signals.exact_name_match
                || score.signals.prefix_name_match
                || score.signals.substring_name_match
                || score.signals.path_match;

            if has_match {
                Some(SearchResult {
                    node: node.clone(),
                    score,
                })
            } else {
                None
            }
        })
        .collect();

    // Deterministic sorting: higher total_score first, then by qualified_name, then by id
    results.sort_by(|a, b| {
        b.score
            .total_score
            .partial_cmp(&a.score.total_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.node.qualified_name.cmp(&b.node.qualified_name))
            .then_with(|| a.node.id.0.cmp(&b.node.id.0))
    });

    if let Some(limit) = options.limit {
        results.truncate(limit);
    }

    results
}
