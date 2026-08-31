use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use super::tests::{discover_tests, is_test_file, is_test_symbol};
use crate::graph::Graph;
use crate::model::{EdgeKind, Node, NodeId};
use crate::query::QueryIndex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImpactKind {
    DirectImpact,
    TransitiveImpact,
    PossibleImpact,
}

impl ImpactKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DirectImpact => "direct_impact",
            Self::TransitiveImpact => "transitive_impact",
            Self::PossibleImpact => "possible_impact",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactExplanation {
    pub target: String,
    pub because: String,
    pub path: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactedNode {
    pub node: Node,
    pub kind: ImpactKind,
    pub depth: usize,
    pub explanation: ImpactExplanation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactAnalysis {
    pub target: Node,
    pub total_impacted: usize,
    pub direct_count: usize,
    pub transitive_count: usize,
    pub possible_count: usize,
    pub impacted_nodes: Vec<ImpactedNode>,
    pub impacted_files: Vec<String>,
    pub related_tests: Vec<String>,
}

#[must_use]
pub fn analyze_impact(
    graph: &Graph,
    target_symbol_or_file: &str,
    max_depth: usize,
) -> Option<ImpactAnalysis> {
    analyze_impact_with_cancel(graph, target_symbol_or_file, max_depth, None)
}

pub fn analyze_impact_with_cancel(
    graph: &Graph,
    target_symbol_or_file: &str,
    max_depth: usize,
    cancelled: Option<&dyn Fn() -> bool>,
) -> Option<ImpactAnalysis> {
    let index = QueryIndex::new(graph);
    let matches = index.find(graph, target_symbol_or_file);
    if matches.is_empty() {
        return None;
    }
    let target_node = matches[0].clone();
    let target_id = target_node.id;

    let mut impacted = Vec::new();
    let mut visited = HashSet::new();
    visited.insert(target_id);

    // Track path and explanations for each node
    // Queue stores (NodeId, current_depth, path_of_qualified_names, last_edge_kind)
    let mut queue = VecDeque::new();
    queue.push_back((
        target_id,
        0usize,
        vec![target_node.qualified_name.clone()],
        None,
    ));

    let mut node_map: HashMap<NodeId, &Node> = HashMap::new();
    for n in &graph.nodes {
        node_map.insert(n.id, n);
    }

    while let Some((curr_id, depth, path, _)) = queue.pop_front() {
        if cancelled.is_some_and(|check| check()) {
            return None;
        }
        if depth >= max_depth {
            continue;
        }

        // Look for upstream callers / dependents (edges pointing TO curr_id: from -> to)
        for edge in &graph.edges {
            if edge.to != curr_id {
                continue;
            }

            // Exclude Contains edges unless exploring upwards to container
            if edge.kind == EdgeKind::Contains {
                continue;
            }

            let next_id = edge.from;
            let Some(&from_node) = node_map.get(&next_id) else {
                continue;
            };

            let is_new = visited.insert(next_id);
            if is_new {
                let next_depth = depth + 1;
                let mut next_path = path.clone();
                next_path.push(from_node.qualified_name.clone());

                let impact_kind = if next_depth == 1 {
                    if edge.kind == EdgeKind::Calls || edge.kind == EdgeKind::Imports {
                        ImpactKind::DirectImpact
                    } else {
                        ImpactKind::PossibleImpact
                    }
                } else if edge.kind == EdgeKind::Calls || edge.kind == EdgeKind::Imports {
                    ImpactKind::TransitiveImpact
                } else {
                    ImpactKind::PossibleImpact
                };

                let curr_node_name = node_map
                    .get(&curr_id)
                    .map(|n| n.name.as_str())
                    .unwrap_or("target");

                let because = format!(
                    "{} -> {} -> {}",
                    from_node.name,
                    edge.kind.as_str().to_lowercase(),
                    curr_node_name
                );

                impacted.push(ImpactedNode {
                    node: from_node.clone(),
                    kind: impact_kind,
                    depth: next_depth,
                    explanation: ImpactExplanation {
                        target: target_node.qualified_name.clone(),
                        because,
                        path: next_path.clone(),
                    },
                });

                queue.push_back((next_id, next_depth, next_path, Some(edge.kind)));
            }
        }
    }

    // Sort impacted nodes: depth asc, kind asc, qualified_name asc
    impacted.sort_by(|a, b| {
        a.depth
            .cmp(&b.depth)
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| a.node.qualified_name.cmp(&b.node.qualified_name))
    });

    let mut direct_count = 0;
    let mut transitive_count = 0;
    let mut possible_count = 0;
    let mut files_set = BTreeSet::new();
    let mut tests_set = BTreeSet::new();

    // Include target's own file
    files_set.insert(target_node.file.clone());

    for item in &impacted {
        match item.kind {
            ImpactKind::DirectImpact => direct_count += 1,
            ImpactKind::TransitiveImpact => transitive_count += 1,
            ImpactKind::PossibleImpact => possible_count += 1,
        }

        files_set.insert(item.node.file.clone());
        if is_test_file(&item.node.file) || is_test_symbol(&item.node) {
            tests_set.insert(item.node.file.clone());
        }
    }

    // Also link tests mapped to target
    let test_discovery = discover_tests(graph);
    for mapping in test_discovery.mappings {
        if mapping.source_file == target_node.file
            || mapping.source_symbol.as_deref() == Some(&target_node.qualified_name)
        {
            for t in mapping.tests {
                tests_set.insert(t.test_file);
            }
        }
    }

    Some(ImpactAnalysis {
        target: target_node,
        total_impacted: impacted.len(),
        direct_count,
        transitive_count,
        possible_count,
        impacted_nodes: impacted,
        impacted_files: files_set.into_iter().collect(),
        related_tests: tests_set.into_iter().collect(),
    })
}
