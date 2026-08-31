pub mod budget;
pub mod bundle;
pub mod candidate;
pub mod ranking;
pub mod request;
pub mod slice;

use std::path::Path;

pub use budget::{BudgetConfig, BudgetReport, BudgetedItem, allocate_budget};
pub use bundle::{ContextBundle, ContextFileBundle, ContextSliceEntry, bundle_and_deduplicate};
pub use candidate::{CandidateRole, ContextCandidate, ExpansionOptions, expand_candidates};
pub use ranking::{RankedCandidate, rank_candidates, score_candidate};
pub use request::{BudgetValueType, ContextRequest, resolve_seeds};
pub use slice::{SourceSlice, estimate_approx_tokens, extract_lines, extract_source_slice};

use crate::graph::Graph;

/// Public high-level entrypoint for AI Context generation.
#[must_use]
pub fn generate_context(
    graph: &Graph,
    request: &ContextRequest,
    repo_root: Option<&Path>,
) -> ContextBundle {
    // 1. Resolve seeds
    let seeds = resolve_seeds(graph, request, repo_root);

    // 2. Expand candidates
    let expansion_options = ExpansionOptions {
        max_depth: if request.max_depth == 0 {
            3
        } else {
            request.max_depth
        },
        max_candidates: if request.max_candidates == 0 {
            100
        } else {
            request.max_candidates
        },
    };
    let candidates = expand_candidates(graph, &seeds, &expansion_options);

    // 3. Rank & score candidates
    let ranked = rank_candidates(candidates);

    // 4. Allocate budget
    let budget_limit = request.budget.unwrap_or(8000);
    let budget_config = BudgetConfig {
        limit: budget_limit,
        budget_type: request.budget_type,
    };
    let (budgeted_items, budget_report) = allocate_budget(ranked, &budget_config, repo_root);

    // 5. Bundle & deduplicate slices
    bundle_and_deduplicate(budgeted_items, budget_report)
}
