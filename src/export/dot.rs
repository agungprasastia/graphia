use std::fmt::Write;

use crate::graph::Graph;
use crate::model::{EdgeKind, NodeKind};

/// Export graph to Graphviz DOT format.
#[must_use]
pub fn export_dot(graph: &Graph) -> String {
    let mut out = String::with_capacity(1024 + graph.nodes.len() * 64 + graph.edges.len() * 48);
    out.push_str("digraph Graphia {\n");
    out.push_str("  rankdir=LR;\n");
    out.push_str("  compound=true;\n");
    out.push_str("  node [fontsize=10, fontname=\"Helvetica\", shape=ellipse, style=filled, fillcolor=\"#f3f4f6\"];\n");
    out.push_str("  edge [fontsize=8, fontname=\"Helvetica\", color=\"#4b5563\"];\n\n");

    for node in &graph.nodes {
        let (shape, fillcolor, textcolor) = match node.kind {
            NodeKind::Struct | NodeKind::Class => ("box", "#dbeafe", "#1e3a8a"),
            NodeKind::Trait | NodeKind::Interface => ("diamond", "#ede9fe", "#5b21b6"),
            NodeKind::Enum => ("hexagon", "#fef3c7", "#92400e"),
            NodeKind::Function | NodeKind::Method | NodeKind::Constructor => {
                ("ellipse", "#d1fae5", "#065f46")
            }
            NodeKind::Module | NodeKind::Package | NodeKind::Namespace => {
                ("folder", "#ffedd5", "#9a3412")
            }
            NodeKind::File => ("note", "#e5e7eb", "#374151"),
            _ => ("ellipse", "#f3f4f6", "#1f2937"),
        };

        let escaped_name = escape_dot_string(&node.name);
        let escaped_file = escape_dot_string(&node.file);
        let label = format!(
            "{}\\n[{}]\\n{}",
            escaped_name,
            node.kind.as_str(),
            escaped_file
        );

        let _ = writeln!(
            out,
            "  node_{} [label=\"{}\", shape={}, fillcolor=\"{}\", fontcolor=\"{}\"];",
            node.id.0, label, shape, fillcolor, textcolor
        );
    }

    out.push('\n');

    for edge in &graph.edges {
        let (color, style) = match edge.kind {
            EdgeKind::Calls => ("#2563eb", "solid"),
            EdgeKind::Imports => ("#4b5563", "dashed"),
            EdgeKind::Contains => ("#9ca3af", "dotted"),
            EdgeKind::Implements => ("#059669", "bold"),
            EdgeKind::Inherits => ("#7c3aed", "bold"),
            EdgeKind::References | EdgeKind::TypeReferences => ("#d97706", "dashed"),
            EdgeKind::Instantiates => ("#dc2626", "solid"),
            EdgeKind::Exports => ("#0891b2", "solid"),
        };

        let edge_label = edge.label.as_deref().unwrap_or_else(|| edge.kind.as_str());
        let escaped_label = escape_dot_string(edge_label);

        let _ = writeln!(
            out,
            "  node_{} -> node_{} [label=\"{}\", color=\"{}\", style={}];",
            edge.from.0, edge.to.0, escaped_label, color, style
        );
    }

    out.push_str("}\n");
    out
}

fn escape_dot_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}
