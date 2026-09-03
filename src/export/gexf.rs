use std::fmt::Write;

use crate::export::graphml::escape_xml;
use crate::graph::Graph;

/// Export graph to GEXF (Gephi Exchange XML Format).
#[must_use]
pub fn export_gexf(graph: &Graph) -> String {
    let mut out = String::with_capacity(2048 + graph.nodes.len() * 128 + graph.edges.len() * 96);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<gexf xmlns=\"http://www.gexf.net/1.2draft\" version=\"1.2\">\n");
    out.push_str("  <meta>\n");
    out.push_str("    <creator>Graphia</creator>\n");
    out.push_str("    <description>Code Knowledge Graph</description>\n");
    out.push_str("  </meta>\n");
    out.push_str("  <graph mode=\"static\" defaultedgetype=\"directed\">\n");
    out.push_str("    <attributes class=\"node\">\n");
    out.push_str("      <attribute id=\"0\" title=\"kind\" type=\"string\"/>\n");
    out.push_str("      <attribute id=\"1\" title=\"file\" type=\"string\"/>\n");
    out.push_str("      <attribute id=\"2\" title=\"language\" type=\"string\"/>\n");
    out.push_str("    </attributes>\n");

    out.push_str("    <nodes>\n");
    for node in &graph.nodes {
        let _ = writeln!(
            out,
            "      <node id=\"{}\" label=\"{}\">",
            node.id.0,
            escape_xml(&node.name)
        );
        out.push_str("        <attvalues>\n");
        let _ = writeln!(
            out,
            "          <attvalue for=\"0\" value=\"{}\"/>",
            escape_xml(node.kind.as_str())
        );
        let _ = writeln!(
            out,
            "          <attvalue for=\"1\" value=\"{}\"/>",
            escape_xml(&node.file)
        );
        if let Some(lang) = node.language {
            let _ = writeln!(
                out,
                "          <attvalue for=\"2\" value=\"{}\"/>",
                escape_xml(lang.as_str())
            );
        }
        out.push_str("        </attvalues>\n");
        out.push_str("      </node>\n");
    }
    out.push_str("    </nodes>\n");

    out.push_str("    <edges>\n");
    for edge in &graph.edges {
        let _ = writeln!(
            out,
            "      <edge id=\"{}\" source=\"{}\" target=\"{}\" label=\"{}\"/>",
            edge.id.0,
            edge.from.0,
            edge.to.0,
            escape_xml(edge.kind.as_str())
        );
    }
    out.push_str("    </edges>\n");

    out.push_str("  </graph>\n");
    out.push_str("</gexf>\n");
    out
}
