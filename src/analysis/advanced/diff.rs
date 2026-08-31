use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::graph::Graph;
use crate::model::{Edge, EdgeKind, Node, NodeKind, SemanticNodeKey, Visibility};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphDiffSummary {
    pub added_nodes: Vec<Node>,
    pub removed_nodes: Vec<Node>,
    pub modified_nodes: Vec<NodeModification>,
    pub added_edges: Vec<Edge>,
    pub removed_edges: Vec<Edge>,
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
    pub old_signature: Option<String>,
    pub new_signature: Option<String>,
    pub old_visibility: Visibility,
    pub new_visibility: Visibility,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiDiffSummary {
    pub added_public_symbols: Vec<Node>,
    pub removed_public_symbols: Vec<Node>,
    pub modified_signatures: Vec<ModifiedSignatureRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModifiedSignatureRecord {
    pub symbol: String,
    pub old_signature: Option<String>,
    pub new_signature: Option<String>,
    pub old_visibility: Visibility,
    pub new_visibility: Visibility,
    pub breaking_candidate: bool,
}

#[must_use]
pub fn diff_graphs(old_graph: &Graph, new_graph: &Graph) -> GraphDiffSummary {
    let old_map: HashMap<SemanticNodeKey, &Node> = old_graph
        .nodes
        .iter()
        .map(|n| (SemanticNodeKey::from_node(n), n))
        .collect();

    let new_map: HashMap<SemanticNodeKey, &Node> = new_graph
        .nodes
        .iter()
        .map(|n| (SemanticNodeKey::from_node(n), n))
        .collect();

    let mut added_nodes = Vec::new();
    let mut removed_nodes = Vec::new();
    let mut modified_nodes = Vec::new();

    for (key, n_node) in &new_map {
        if let Some(o_node) = old_map.get(key) {
            if o_node.location != n_node.location
                || o_node.signature != n_node.signature
                || o_node.visibility != n_node.visibility
            {
                modified_nodes.push(NodeModification {
                    qualified_name: n_node.qualified_name.clone(),
                    old_file: o_node.location.file.clone(),
                    new_file: n_node.location.file.clone(),
                    old_line: o_node.location.start_line,
                    new_line: n_node.location.start_line,
                    old_signature: o_node.signature.clone(),
                    new_signature: n_node.signature.clone(),
                    old_visibility: o_node.visibility,
                    new_visibility: n_node.visibility,
                });
            }
        } else {
            added_nodes.push((*n_node).clone());
        }
    }

    for (key, o_node) in &old_map {
        if !new_map.contains_key(key) {
            removed_nodes.push((*o_node).clone());
        }
    }

    added_nodes.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
    removed_nodes.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
    modified_nodes.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));

    let old_node_map: HashMap<crate::model::NodeId, SemanticNodeKey> = old_graph
        .nodes
        .iter()
        .map(|n| (n.id, SemanticNodeKey::from_node(n)))
        .collect();
    let new_node_map: HashMap<crate::model::NodeId, SemanticNodeKey> = new_graph
        .nodes
        .iter()
        .map(|n| (n.id, SemanticNodeKey::from_node(n)))
        .collect();

    type EdgeSemanticTuple = (SemanticNodeKey, SemanticNodeKey, EdgeKind, Option<String>);

    let old_edges_set: HashSet<EdgeSemanticTuple> = old_graph
        .edges
        .iter()
        .filter_map(|e| {
            let from_key = old_node_map.get(&e.from)?.clone();
            let to_key = old_node_map.get(&e.to)?.clone();
            Some((from_key, to_key, e.kind, e.label.clone()))
        })
        .collect();

    let new_edges_set: HashSet<EdgeSemanticTuple> = new_graph
        .edges
        .iter()
        .filter_map(|e| {
            let from_key = new_node_map.get(&e.from)?.clone();
            let to_key = new_node_map.get(&e.to)?.clone();
            Some((from_key, to_key, e.kind, e.label.clone()))
        })
        .collect();

    let mut added_edges = Vec::new();
    for e in &new_graph.edges {
        if let (Some(from_key), Some(to_key)) = (new_node_map.get(&e.from), new_node_map.get(&e.to))
        {
            let tuple = (from_key.clone(), to_key.clone(), e.kind, e.label.clone());
            if !old_edges_set.contains(&tuple) {
                added_edges.push(e.clone());
            }
        }
    }

    let mut removed_edges = Vec::new();
    for e in &old_graph.edges {
        if let (Some(from_key), Some(to_key)) = (old_node_map.get(&e.from), old_node_map.get(&e.to))
        {
            let tuple = (from_key.clone(), to_key.clone(), e.kind, e.label.clone());
            if !new_edges_set.contains(&tuple) {
                removed_edges.push(e.clone());
            }
        }
    }

    let added_edges_count = added_edges.len();
    let removed_edges_count = removed_edges.len();

    GraphDiffSummary {
        added_nodes,
        removed_nodes,
        modified_nodes,
        added_edges,
        removed_edges,
        added_edges_count,
        removed_edges_count,
    }
}

#[must_use]
pub fn diff_public_api(old_graph: &Graph, new_graph: &Graph) -> ApiDiffSummary {
    let is_public = |n: &Node| -> bool {
        if matches!(
            n.kind,
            NodeKind::File | NodeKind::Module | NodeKind::Package | NodeKind::Namespace
        ) {
            return false;
        }
        match n.visibility {
            Visibility::Public => true,
            Visibility::Private
            | Visibility::Protected
            | Visibility::Internal
            | Visibility::Package => false,
            Visibility::Unknown => {
                !n.name.starts_with('_') && !n.file.contains("test") && !n.file.contains("internal")
            }
        }
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
    let mut modified_signatures = Vec::new();

    for (qname, n_node) in &new_pub {
        if let Some(o_node) = old_pub.get(qname) {
            if o_node.signature != n_node.signature || o_node.visibility != n_node.visibility {
                let breaking = o_node.signature != n_node.signature
                    || (o_node.visibility == Visibility::Public
                        && n_node.visibility != Visibility::Public);
                modified_signatures.push(ModifiedSignatureRecord {
                    symbol: qname.clone(),
                    old_signature: o_node.signature.clone(),
                    new_signature: n_node.signature.clone(),
                    old_visibility: o_node.visibility,
                    new_visibility: n_node.visibility,
                    breaking_candidate: breaking,
                });
            }
        } else {
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
    modified_signatures.sort_by(|a, b| a.symbol.cmp(&b.symbol));

    ApiDiffSummary {
        added_public_symbols,
        removed_public_symbols,
        modified_signatures,
    }
}
