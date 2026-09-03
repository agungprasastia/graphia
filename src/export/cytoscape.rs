use serde_json::json;

use crate::graph::Graph;

/// Export graph to Cytoscape.js elements JSON format.
#[must_use]
pub fn export_cytoscape(graph: &Graph) -> String {
    let nodes: Vec<_> = graph
        .nodes
        .iter()
        .map(|node| {
            json!({
                "data": {
                    "id": node.id.0.to_string(),
                    "name": node.name,
                    "qualified_name": node.qualified_name,
                    "kind": node.kind.as_str(),
                    "file": node.file,
                    "language": node.language.map(|l| l.as_str()),
                    "signature": node.signature,
                    "container": node.container,
                }
            })
        })
        .collect();

    let edges: Vec<_> = graph
        .edges
        .iter()
        .map(|edge| {
            json!({
                "data": {
                    "id": format!("e_{}", edge.id.0),
                    "source": edge.from.0.to_string(),
                    "target": edge.to.0.to_string(),
                    "kind": edge.kind.as_str(),
                    "confidence": edge.confidence.as_str(),
                    "label": edge.label,
                }
            })
        })
        .collect();

    let payload = json!({
        "elements": {
            "nodes": nodes,
            "edges": edges,
        }
    });

    serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
}
