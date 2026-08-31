use serde::{Deserialize, Serialize};

use crate::model::Node;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelevanceSignals {
    pub exact_qualified_match: bool,
    pub exact_name_match: bool,
    pub prefix_name_match: bool,
    pub substring_name_match: bool,
    pub path_match: bool,
    pub centrality: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelevanceScore {
    pub total_score: f64,
    pub signals: RelevanceSignals,
}

#[must_use]
pub fn score_relevance(node: &Node, query: &str, centrality: f64) -> RelevanceScore {
    let query_lower = query.to_lowercase();
    let name_lower = node.name.to_lowercase();
    let qualified_lower = node.qualified_name.to_lowercase();
    let file_lower = node.file.to_lowercase();

    let exact_qualified_match = qualified_lower == query_lower;
    let exact_name_match = name_lower == query_lower;
    let prefix_name_match = name_lower.starts_with(&query_lower);
    let substring_name_match = name_lower.contains(&query_lower);
    let path_match = file_lower.contains(&query_lower);

    let mut score = 0.0;

    if exact_qualified_match {
        score += 100.0;
    } else if exact_name_match {
        score += 80.0;
    } else if prefix_name_match {
        score += 50.0;
    } else if substring_name_match {
        score += 30.0;
    }

    if path_match {
        score += 15.0;
    }

    // Boost score with centrality (PageRank or normalized degree, scaled)
    score += centrality * 10.0;

    let signals = RelevanceSignals {
        exact_qualified_match,
        exact_name_match,
        prefix_name_match,
        substring_name_match,
        path_match,
        centrality,
    };

    RelevanceScore {
        total_score: score,
        signals,
    }
}
