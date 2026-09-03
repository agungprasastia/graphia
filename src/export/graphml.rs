use std::fmt::Write;

use crate::graph::Graph;

/// Export graph to GraphML format (supported by Gephi, Cytoscape, yEd).
#[must_use]
pub fn export_graphml(graph: &Graph) -> String {
    let mut out = String::with_capacity(2048 + graph.nodes.len() * 128 + graph.edges.len() * 96);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<graphml xmlns=\"http://graphml.graphdrawing.org/xmlns\"\n");
    out.push_str("         xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"\n");
    out.push_str("         xsi:schemaLocation=\"http://graphml.graphdrawing.org/xmlns\n");
    out.push_str("         http://graphml.graphdrawing.org/xmlns/1.0/graphml.xsd\">\n");

    // Key declarations
    out.push_str("  <key id=\"d_name\" for=\"node\" attr.name=\"name\" attr.type=\"string\"/>\n");
    out.push_str("  <key id=\"d_kind\" for=\"node\" attr.name=\"kind\" attr.type=\"string\"/>\n");
    out.push_str("  <key id=\"d_file\" for=\"node\" attr.name=\"file\" attr.type=\"string\"/>\n");
    out.push_str(
        "  <key id=\"d_lang\" for=\"node\" attr.name=\"language\" attr.type=\"string\"/>\n",
    );
    out.push_str(
        "  <key id=\"d_sig\" for=\"node\" attr.name=\"signature\" attr.type=\"string\"/>\n",
    );
    out.push_str("  <key id=\"e_kind\" for=\"edge\" attr.name=\"kind\" attr.type=\"string\"/>\n");
    out.push_str(
        "  <key id=\"e_conf\" for=\"edge\" attr.name=\"confidence\" attr.type=\"string\"/>\n",
    );
    out.push_str("  <key id=\"e_label\" for=\"edge\" attr.name=\"label\" attr.type=\"string\"/>\n");

    out.push_str("  <graph id=\"Graphia\" edgedefault=\"directed\">\n");

    for node in &graph.nodes {
        let _ = writeln!(out, "    <node id=\"n{}\">", node.id.0);
        let _ = writeln!(
            out,
            "      <data key=\"d_name\">{}</data>",
            escape_xml(&node.name)
        );
        let _ = writeln!(
            out,
            "      <data key=\"d_kind\">{}</data>",
            escape_xml(node.kind.as_str())
        );
        let _ = writeln!(
            out,
            "      <data key=\"d_file\">{}</data>",
            escape_xml(&node.file)
        );
        if let Some(lang) = node.language {
            let _ = writeln!(
                out,
                "      <data key=\"d_lang\">{}</data>",
                escape_xml(lang.as_str())
            );
        }
        if let Some(sig) = &node.signature {
            let _ = writeln!(out, "      <data key=\"d_sig\">{}</data>", escape_xml(sig));
        }
        out.push_str("    </node>\n");
    }

    for edge in &graph.edges {
        let _ = writeln!(
            out,
            "    <edge id=\"e{}\" source=\"n{}\" target=\"n{}\">",
            edge.id.0, edge.from.0, edge.to.0
        );
        let _ = writeln!(
            out,
            "      <data key=\"e_kind\">{}</data>",
            escape_xml(edge.kind.as_str())
        );
        let _ = writeln!(
            out,
            "      <data key=\"e_conf\">{}</data>",
            escape_xml(edge.confidence.as_str())
        );
        if let Some(label) = &edge.label {
            let _ = writeln!(
                out,
                "      <data key=\"e_label\">{}</data>",
                escape_xml(label)
            );
        }
        out.push_str("    </edge>\n");
    }

    out.push_str("  </graph>\n");
    out.push_str("</graphml>\n");
    out
}

pub(crate) fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
