use std::path::Path;

use serde::{Deserialize, Serialize};

use super::candidate::CandidateRole;
use super::ranking::RankedCandidate;
use super::request::BudgetValueType;
use super::slice::{SourceSlice, extract_source_slice};
use crate::model::Node;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetedItem {
    pub node: Node,
    pub role: CandidateRole,
    pub distance: usize,
    pub score: f64,
    pub reason: String,
    pub slice: SourceSlice,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetReport {
    pub budget_type: BudgetValueType,
    pub budget_limit: usize,
    pub budget_used: usize,
    pub items_included: usize,
    pub items_omitted: usize,
}

#[derive(Debug, Clone)]
pub struct BudgetConfig {
    pub limit: usize,
    pub budget_type: BudgetValueType,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            limit: 8000,
            budget_type: BudgetValueType::ApproxTokens,
        }
    }
}

pub fn allocate_budget(
    ranked_candidates: Vec<RankedCandidate>,
    config: &BudgetConfig,
    repo_root: Option<&Path>,
) -> (Vec<BudgetedItem>, BudgetReport) {
    let mut included_items = Vec::new();
    let mut current_usage = 0usize;
    let mut omitted_count = 0usize;

    for ranked in ranked_candidates {
        let node = &ranked.candidate.node;
        // Extract slice for candidate node location
        let slice = match extract_source_slice(repo_root, &node.location) {
            Ok(s) => s,
            Err(_) => {
                // If disk read fails, fallback to empty slice
                SourceSlice {
                    file: node.location.file.clone(),
                    start_line: node.location.start_line,
                    start_col: node.location.start_col,
                    end_line: node.location.end_line,
                    end_col: node.location.end_col,
                    content: String::new(),
                    approx_tokens: 0,
                    bytes: 0,
                    characters: 0,
                }
            }
        };

        let cost = match config.budget_type {
            BudgetValueType::ApproxTokens => slice.approx_tokens,
            BudgetValueType::Bytes => slice.bytes,
            BudgetValueType::Characters => slice.characters,
        };

        // If including this item fits within budget (or if it's the very first seed item)
        if current_usage + cost <= config.limit || included_items.is_empty() {
            current_usage += cost;
            included_items.push(BudgetedItem {
                node: node.clone(),
                role: ranked.candidate.role,
                distance: ranked.candidate.distance,
                score: ranked.score,
                reason: ranked.candidate.reason,
                slice,
            });
        } else {
            omitted_count += 1;
        }
    }

    let report = BudgetReport {
        budget_type: config.budget_type,
        budget_limit: config.limit,
        budget_used: current_usage,
        items_included: included_items.len(),
        items_omitted: omitted_count,
    };

    (included_items, report)
}
