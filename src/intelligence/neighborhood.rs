use std::collections::{HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use super::tests::discover_tests;
use crate::graph::Graph;
use crate::model::{EdgeKind, Node, NodeId, NodeKind};
use crate::query::QueryIndex;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundedNeighborhood {
    pub target: Node,
    pub container: Option<Node>,
    pub parent_module: Option<Node>,
    pub children: Vec<Node>,
    pub callers: Vec<Node>,
    pub callees: Vec<Node>,
    pub imports: Vec<Node>,
    pub exports: Vec<Node>,
    pub referenced_types: Vec<Node>,
    pub trait_implementations: Vec<Node>,
    pub related_tests: Vec<Node>,
}

#[derive(Debug, Clone)]
pub struct NeighborhoodOptions {
    pub target: String,
    pub depth: usize,
    pub limit: usize,
}

impl Default for NeighborhoodOptions {
    fn default() -> Self {
        Self {
            target: String::new(),
            depth: 1,
            limit: 50,
        }
    }
}

#[must_use]
pub fn get_neighborhood(
    graph: &Graph,
    options: &NeighborhoodOptions,
) -> Option<BoundedNeighborhood> {
    get_neighborhood_with_cancel(graph, options, None)
}

pub fn get_neighborhood_with_cancel(
    graph: &Graph,
    options: &NeighborhoodOptions,
    cancelled: Option<&dyn Fn() -> bool>,
) -> Option<BoundedNeighborhood> {
    let index = QueryIndex::new(graph);
    let matches = index.find(graph, &options.target);
    if matches.is_empty() {
        return None;
    }
    let target_node = matches[0].clone();
    let target_id = target_node.id;

    // Direct container: Contains edge pointing TO target
    let mut container = None;
    let mut parent_module = None;

    for edge in &graph.edges {
        if cancelled.is_some_and(|check| check()) {
            return None;
        }
        if edge.kind == EdgeKind::Contains && edge.to == target_id {
            if let Some(n) = graph.nodes.iter().find(|n| n.id == edge.from) {
                if n.kind == NodeKind::Module || n.kind == NodeKind::File {
                    parent_module = Some(n.clone());
                }
                container = Some(n.clone());
            }
        }
    }

    // Children: Contains edge FROM target
    let mut children: Vec<Node> = graph
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Contains && e.from == target_id)
        .filter_map(|e| graph.nodes.iter().find(|n| n.id == e.to).cloned())
        .collect();

    // Callers / Callees with BFS up to depth
    let callers = collect_neighbors_k_hop(
        graph,
        target_id,
        EdgeKind::Calls,
        false,
        options.depth,
        options.limit,
    );
    let callees = collect_neighbors_k_hop(
        graph,
        target_id,
        EdgeKind::Calls,
        true,
        options.depth,
        options.limit,
    );

    // Imports / Exports
    let imports = collect_neighbors_k_hop(
        graph,
        target_id,
        EdgeKind::Imports,
        true,
        options.depth,
        options.limit,
    );
    let exports = collect_neighbors_k_hop(
        graph,
        target_id,
        EdgeKind::Imports,
        false,
        options.depth,
        options.limit,
    );

    // Trait / Interface Implementations (Inherits / Implements)
    let mut trait_implementations: Vec<Node> = graph
        .edges
        .iter()
        .filter(|e| {
            (e.kind == EdgeKind::Implements || e.kind == EdgeKind::Inherits)
                && (e.from == target_id || e.to == target_id)
        })
        .filter_map(|e| {
            let other_id = if e.from == target_id { e.to } else { e.from };
            graph.nodes.iter().find(|n| n.id == other_id).cloned()
        })
        .collect();

    // Referenced Types (Struct, Trait, Interface, Class called/referenced)
    let mut referenced_types: Vec<Node> = callees
        .iter()
        .chain(imports.iter())
        .filter(|n| {
            matches!(
                n.kind,
                NodeKind::Struct | NodeKind::Trait | NodeKind::Interface | NodeKind::Class
            )
        })
        .cloned()
        .collect();

    // Related Tests via deterministic test discovery
    let test_discovery = discover_tests(graph);
    let mut related_test_ids = HashSet::new();

    // Check if target is mapped to tests
    for mapping in &test_discovery.mappings {
        if cancelled.is_some_and(|check| check()) {
            return None;
        }
        if mapping.source_file == target_node.file
            || mapping.source_symbol.as_ref() == Some(&target_node.qualified_name)
            || mapping.source_symbol.as_ref() == Some(&target_node.name)
        {
            for test in &mapping.tests {
                if let Some(id) = test.test_symbol_id {
                    related_test_ids.insert(id);
                } else if let Some(file_node) = graph
                    .nodes
                    .iter()
                    .find(|n| n.file == test.test_file && n.kind == NodeKind::File)
                {
                    related_test_ids.insert(file_node.id);
                }
            }
        }
    }

    // Also check if any caller is in a test file
    for caller in &callers {
        if caller.file.contains("test")
            || caller.name.starts_with("test_")
            || caller.name.ends_with("_test")
        {
            related_test_ids.insert(caller.id);
        }
    }

    let mut related_tests: Vec<Node> = related_test_ids
        .into_iter()
        .filter_map(|id| graph.nodes.iter().find(|n| n.id == id).cloned())
        .collect();

    // Deduplicate and apply limit
    sort_and_truncate(&mut children, options.limit);
    sort_and_truncate(&mut trait_implementations, options.limit);
    sort_and_truncate(&mut referenced_types, options.limit);
    sort_and_truncate(&mut related_tests, options.limit);

    Some(BoundedNeighborhood {
        target: target_node,
        container,
        parent_module,
        children,
        callers,
        callees,
        imports,
        exports,
        referenced_types,
        trait_implementations,
        related_tests,
    })
}

fn collect_neighbors_k_hop(
    graph: &Graph,
    start_id: NodeId,
    kind: EdgeKind,
    outgoing: bool,
    max_depth: usize,
    limit: usize,
) -> Vec<Node> {
    if max_depth == 0 {
        return Vec::new();
    }

    let mut visited = HashSet::new();
    visited.insert(start_id);
    let mut queue = VecDeque::new();
    queue.push_back((start_id, 0));

    let mut result_nodes = Vec::new();

    while let Some((curr, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }

        for edge in &graph.edges {
            if edge.kind != kind {
                continue;
            }
            let next_id = if outgoing && edge.from == curr {
                Some(edge.to)
            } else if !outgoing && edge.to == curr {
                Some(edge.from)
            } else {
                None
            };

            if let Some(nid) = next_id {
                if visited.insert(nid) {
                    if let Some(node) = graph.nodes.iter().find(|n| n.id == nid) {
                        result_nodes.push(node.clone());
                    }
                    queue.push_back((nid, depth + 1));
                }
            }
        }
    }

    sort_and_truncate(&mut result_nodes, limit);
    result_nodes
}

fn sort_and_truncate(nodes: &mut Vec<Node>, limit: usize) {
    nodes.sort_by(|a, b| {
        a.qualified_name
            .cmp(&b.qualified_name)
            .then_with(|| a.id.0.cmp(&b.id.0))
    });
    nodes.dedup_by(|a, b| a.id == b.id);
    if nodes.len() > limit {
        nodes.truncate(limit);
    }
}
