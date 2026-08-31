use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::graph::Graph;
use crate::model::{EdgeKind, Node, NodeKind};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeadCodeCandidate {
    pub node: Node,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeadCodeReport {
    pub candidates_count: usize,
    pub candidates: Vec<DeadCodeCandidate>,
}

#[must_use]
pub fn detect_dead_code_candidates(graph: &Graph) -> DeadCodeReport {
    let mut referenced_nodes = HashSet::new();

    // Collect all nodes referenced via Calls, Inherits, Implements, Imports
    for edge in &graph.edges {
        if edge.kind != EdgeKind::Contains {
            referenced_nodes.insert(edge.to);
        }
    }

    let mut candidates = Vec::new();

    for node in &graph.nodes {
        // Skip files and modules
        if matches!(node.kind, NodeKind::File | NodeKind::Module) {
            continue;
        }

        // Check if node is entrypoint/main or test
        if node.name == "main"
            || node.name.starts_with("test_")
            || node.name.ends_with("_test")
            || node.file.contains("test")
            || node.file.contains("tests")
        {
            continue;
        }

        // If not referenced anywhere
        if !referenced_nodes.contains(&node.id) {
            candidates.push(DeadCodeCandidate {
                node: node.clone(),
                reason: format!(
                    "Non-entrypoint {} '{}' in {} has 0 incoming references",
                    node.kind.as_str(),
                    node.name,
                    node.file
                ),
            });
        }
    }

    candidates.sort_by(|a, b| {
        a.node
            .qualified_name
            .cmp(&b.node.qualified_name)
            .then_with(|| a.node.id.0.cmp(&b.node.id.0))
    });

    let candidates_count = candidates.len();

    DeadCodeReport {
        candidates_count,
        candidates,
    }
}
