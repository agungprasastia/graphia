use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::graph::Graph;
use crate::model::{Node, NodeKind};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphDiffSummary {
    pub added_nodes: Vec<Node>,
    pub removed_nodes: Vec<Node>,
    pub modified_nodes: Vec<NodeModification>,
    pub added_edges_count: usize,
    pub removed_edges_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeModification {
    pub qualified_name: String,
    pub old_file: String,
    pub new_file: String,
    pub old_line: u32,
    pub new_line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiDiffSummary {
    pub added_public_symbols: Vec<Node>,
    pub removed_public_symbols: Vec<Node>,
    pub modified_signatures: Vec<String>,
}

#[must_use]
pub fn diff_graphs(old_graph: &Graph, new_graph: &Graph) -> GraphDiffSummary {
    let old_map: HashMap<String, &Node> = old_graph
        .nodes
        .iter()
        .map(|n| (n.qualified_name.clone(), n))
        .collect();

    let new_map: HashMap<String, &Node> = new_graph
        .nodes
        .iter()
        .map(|n| (n.qualified_name.clone(), n))
        .collect();

    let mut added_nodes = Vec::new();
    let mut removed_nodes = Vec::new();
    let mut modified_nodes = Vec::new();

    for (qname, n_node) in &new_map {
        if let Some(o_node) = old_map.get(qname) {
            if o_node.location != n_node.location {
                modified_nodes.push(NodeModification {
                    qualified_name: qname.clone(),
                    old_file: o_node.location.file.clone(),
                    new_file: n_node.location.file.clone(),
                    old_line: o_node.location.start_line,
                    new_line: n_node.location.start_line,
                });
            }
        } else {
            added_nodes.push((*n_node).clone());
        }
    }

    for (qname, o_node) in &old_map {
        if !new_map.contains_key(qname) {
            removed_nodes.push((*o_node).clone());
        }
    }

    added_nodes.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
    removed_nodes.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
    modified_nodes.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));

    let added_edges_count = new_graph.edges.len().saturating_sub(old_graph.edges.len());
    let removed_edges_count = old_graph.edges.len().saturating_sub(new_graph.edges.len());

    GraphDiffSummary {
        added_nodes,
        removed_nodes,
        modified_nodes,
        added_edges_count,
        removed_edges_count,
    }
}

#[must_use]
pub fn diff_public_api(old_graph: &Graph, new_graph: &Graph) -> ApiDiffSummary {
    let is_public = |n: &Node| -> bool {
        !matches!(n.kind, NodeKind::File | NodeKind::Module)
            && !n.name.starts_with('_')
            && !n.file.contains("test")
            && !n.file.contains("internal")
    };

    let old_pub: HashMap<String, &Node> = old_graph
        .nodes
        .iter()
        .filter(|n| is_public(n))
        .map(|n| (n.qualified_name.clone(), n))
        .collect();

    let new_pub: HashMap<String, &Node> = new_graph
        .nodes
        .iter()
        .filter(|n| is_public(n))
        .map(|n| (n.qualified_name.clone(), n))
        .collect();

    let mut added_public_symbols = Vec::new();
    let mut removed_public_symbols = Vec::new();

    for (qname, n_node) in &new_pub {
        if !old_pub.contains_key(qname) {
            added_public_symbols.push((*n_node).clone());
        }
    }

    for (qname, o_node) in &old_pub {
        if !new_pub.contains_key(qname) {
            removed_public_symbols.push((*o_node).clone());
        }
    }

    added_public_symbols.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
    removed_public_symbols.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));

    ApiDiffSummary {
        added_public_symbols,
        removed_public_symbols,
        modified_signatures: Vec::new(),
    }
}
