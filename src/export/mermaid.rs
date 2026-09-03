use std::collections::{BTreeMap, HashSet};
use std::fmt::Write;

use crate::graph::Graph;
use crate::model::{EdgeKind, NodeId, NodeKind};

/// Export graph to Mermaid flowchart format.
/// If `max_nodes` is specified and the graph exceeds it, prioritize highest-degree nodes.
#[must_use]
pub fn export_mermaid(graph: &Graph, max_nodes: Option<usize>) -> String {
    let active_node_ids: HashSet<NodeId> = if let Some(limit) = max_nodes {
        if graph.nodes.len() > limit {
            // Count degree for each node
            let mut degree: BTreeMap<NodeId, usize> = BTreeMap::new();
            for edge in &graph.edges {
                *degree.entry(edge.from).or_insert(0) += 1;
                *degree.entry(edge.to).or_insert(0) += 1;
            }
            let mut node_ranks: Vec<_> = graph
                .nodes
                .iter()
                .map(|n| (degree.get(&n.id).copied().unwrap_or(0), n.id))
                .collect();
            node_ranks.sort_by_key(|a| std::cmp::Reverse(a.0));
            node_ranks
                .into_iter()
                .take(limit)
                .map(|(_, id)| id)
                .collect()
        } else {
            graph.nodes.iter().map(|n| n.id).collect()
        }
    } else {
        graph.nodes.iter().map(|n| n.id).collect()
    };

    let mut out = String::with_capacity(1024 + active_node_ids.len() * 64);
    out.push_str("flowchart TD\n");

    // Group nodes by file
    let mut file_groups: BTreeMap<&str, Vec<&crate::model::Node>> = BTreeMap::new();
    for node in &graph.nodes {
        if active_node_ids.contains(&node.id) {
            file_groups.entry(&node.file).or_default().push(node);
        }
    }

    let mut subgraph_idx = 0;
    for (file, nodes) in file_groups {
        subgraph_idx += 1;
        let escaped_file = escape_mermaid_label(file);
        let _ = writeln!(out, "  subgraph sub_{subgraph_idx} [\"{escaped_file}\"]");

        for node in nodes {
            let label = format!(
                "{}: {}",
                node.kind.as_str(),
                escape_mermaid_label(&node.name)
            );
            let shape = match node.kind {
                NodeKind::Struct | NodeKind::Class => format!("[[\"{label}\"]]"),
                NodeKind::Trait | NodeKind::Interface => format!("{{\"{label}\"}}"),
                NodeKind::Enum => format!("[\\\"{label}\"/]"),
                NodeKind::Function | NodeKind::Method | NodeKind::Constructor => {
                    format!("([\"{label}\"])")
                }
                NodeKind::Module | NodeKind::Package | NodeKind::Namespace => {
                    format!("[/\"{label}\"/]")
                }
                _ => format!("[\"{label}\"]"),
            };
            let _ = writeln!(out, "    node_{}{}", node.id.0, shape);
        }

        out.push_str("  end\n\n");
    }

    for edge in &graph.edges {
        if active_node_ids.contains(&edge.from) && active_node_ids.contains(&edge.to) {
            let arrow = match edge.kind {
                EdgeKind::Calls => "-->|calls|",
                EdgeKind::Implements => "==>|implements|",
                EdgeKind::Inherits => "==>|inherits|",
                EdgeKind::Imports => "-.->|imports|",
                EdgeKind::Contains => "-->|contains|",
                EdgeKind::References | EdgeKind::TypeReferences => "-.->|refs|",
                EdgeKind::Instantiates => "-->|creates|",
                EdgeKind::Exports => "-->|exports|",
            };
            let _ = writeln!(out, "  node_{} {} node_{}", edge.from.0, arrow, edge.to.0);
        }
    }

    out
}

fn escape_mermaid_label(s: &str) -> String {
    s.replace('"', "#quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('[', "&#91;")
        .replace(']', "&#93;")
        .replace('{', "&#123;")
        .replace('}', "&#125;")
        .replace('(', "&#40;")
        .replace(')', "&#41;")
}
