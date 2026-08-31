use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::graph::Graph;
use crate::intelligence::search::{SearchOptions, search_graph};
use crate::model::Node;
use crate::query::QueryIndex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BudgetValueType {
    #[default]
    ApproxTokens,
    Bytes,
    Characters,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextRequest {
    pub symbol: Option<String>,
    pub file: Option<String>,
    pub query: Option<String>,
    pub changed: bool,
    pub budget: Option<usize>,
    pub budget_type: BudgetValueType,
    pub max_depth: usize,
    pub max_candidates: usize,
}

#[must_use]
pub fn resolve_seeds(
    graph: &Graph,
    request: &ContextRequest,
    repo_root: Option<&Path>,
) -> Vec<Node> {
    let mut seeds = Vec::new();
    let mut seen_ids = HashSet::new();

    let query_index = QueryIndex::new(graph);

    // 1. Resolve --symbol <name>
    if let Some(ref sym) = request.symbol {
        let trimmed = sym.trim();
        if !trimmed.is_empty() {
            let matches = query_index.find(graph, trimmed);
            for node in matches {
                if seen_ids.insert(node.id) {
                    seeds.push(node.clone());
                }
            }
        }
    }

    // 2. Resolve --file <path>
    if let Some(ref file_path) = request.file {
        let normalized = file_path.replace('\\', "/");
        for node in &graph.nodes {
            let node_file = node.file.replace('\\', "/");
            if (node_file == normalized || node_file.ends_with(&normalized))
                && seen_ids.insert(node.id)
            {
                seeds.push(node.clone());
            }
        }
    }

    // 3. Resolve --query "<text>"
    if let Some(ref query_text) = request.query {
        let trimmed = query_text.trim();
        if !trimmed.is_empty() {
            let search_opts = SearchOptions {
                query: trimmed.to_string(),
                kind_filter: None,
                file_filter: None,
                limit: Some(10),
            };
            let search_results = search_graph(graph, &search_opts);
            for res in search_results {
                if seen_ids.insert(res.node.id) {
                    seeds.push(res.node);
                }
            }
        }
    }

    // 4. Resolve --changed
    if request.changed {
        if let Some(root) = repo_root {
            if let Ok(changed_files) = detect_changed_files(root) {
                for node in &graph.nodes {
                    let node_file = node.file.replace('\\', "/");
                    if changed_files.iter().any(|cf| {
                        let cf_norm = cf.replace('\\', "/");
                        node_file == cf_norm || node_file.ends_with(&cf_norm)
                    }) && seen_ids.insert(node.id)
                    {
                        seeds.push(node.clone());
                    }
                }
            }
        }
    }

    // Deterministic sorting of resolved seeds
    seeds.sort_by(|a, b| {
        a.qualified_name
            .cmp(&b.qualified_name)
            .then_with(|| a.id.0.cmp(&b.id.0))
    });

    seeds
}

fn detect_changed_files(root: &Path) -> crate::error::Result<Vec<String>> {
    let scanned = crate::scan::scan_repo(root)?;
    let metadata_path = root.join(".graphia/metadata.json");
    if metadata_path.exists() {
        if let Some(prev_meta) = crate::storage::load_metadata(root)? {
            let changes = crate::incremental::classify_changes(&prev_meta.files, &scanned)?;
            let changed = changes
                .into_iter()
                .filter(|c| c.kind != crate::incremental::ChangeKind::Unchanged)
                .map(|c| c.path)
                .collect();
            return Ok(changed);
        }
    }
    Ok(Vec::new())
}
