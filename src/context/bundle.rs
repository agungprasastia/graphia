use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::budget::{BudgetReport, BudgetedItem};
use super::candidate::CandidateRole;
use crate::model::NodeKind;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextSliceEntry {
    pub file: String,
    pub symbol: String,
    pub kind: NodeKind,
    pub role: CandidateRole,
    pub distance: usize,
    pub score: f64,
    pub reason: String,
    pub start_line: u32,
    pub end_line: u32,
    pub content: String,
    pub approx_tokens: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextFileBundle {
    pub file: String,
    pub slices: Vec<ContextSliceEntry>,
    pub total_approx_tokens: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextBundle {
    pub schema_version: u32,
    pub files: Vec<ContextFileBundle>,
    pub total_items: usize,
    pub total_approx_tokens: usize,
    pub budget: BudgetReport,
}

#[must_use]
pub fn bundle_and_deduplicate(items: Vec<BudgetedItem>, budget: BudgetReport) -> ContextBundle {
    let mut files_map: HashMap<String, Vec<ContextSliceEntry>> = HashMap::new();

    for item in items {
        let entry = ContextSliceEntry {
            file: item.node.file.clone(),
            symbol: item.node.qualified_name.clone(),
            kind: item.node.kind,
            role: item.role,
            distance: item.distance,
            score: item.score,
            reason: item.reason,
            start_line: item.slice.start_line,
            end_line: item.slice.end_line,
            content: item.slice.content,
            approx_tokens: item.slice.approx_tokens,
        };
        files_map
            .entry(item.node.file.clone())
            .or_default()
            .push(entry);
    }

    let mut file_bundles = Vec::new();
    let mut total_items = 0usize;
    let mut total_tokens = 0usize;

    // Deterministic ordering of files
    let mut file_keys: Vec<String> = files_map.keys().cloned().collect();
    file_keys.sort();

    for file in file_keys {
        if let Some(mut slices) = files_map.remove(&file) {
            // Sort slices by line range
            slices.sort_by(|a, b| {
                a.start_line
                    .cmp(&b.start_line)
                    .then_with(|| b.end_line.cmp(&a.end_line))
                    .then_with(|| a.symbol.cmp(&b.symbol))
            });

            // Deduplicate overlapping or nested slices
            let mut deduplicated: Vec<ContextSliceEntry> = Vec::new();
            for slice in slices {
                let is_enclosed = deduplicated.iter().any(|existing| {
                    existing.start_line <= slice.start_line && existing.end_line >= slice.end_line
                });

                if !is_enclosed {
                    // Check if current slice completely encloses previously added smaller slices
                    deduplicated.retain(|existing| {
                        !(slice.start_line <= existing.start_line
                            && slice.end_line >= existing.end_line)
                    });
                    deduplicated.push(slice);
                }
            }

            // Re-sort after deduplication
            deduplicated.sort_by_key(|s| s.start_line);

            let bundle_tokens: usize = deduplicated.iter().map(|s| s.approx_tokens).sum();
            total_items += deduplicated.len();
            total_tokens += bundle_tokens;

            file_bundles.push(ContextFileBundle {
                file,
                slices: deduplicated,
                total_approx_tokens: bundle_tokens,
            });
        }
    }

    ContextBundle {
        schema_version: 1,
        files: file_bundles,
        total_items,
        total_approx_tokens: total_tokens,
        budget,
    }
}
