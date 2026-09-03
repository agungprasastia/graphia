use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::json;

use crate::error::Result;
use crate::export::io_err;
use crate::graph::Graph;
use crate::model::NodeId;

/// Export graph as an interactive Obsidian markdown knowledge vault.
pub fn export_obsidian(graph: &Graph, output_dir: &Path) -> Result<()> {
    fs::create_dir_all(output_dir).map_err(|e| io_err(output_dir, e))?;
    let obsidian_config_dir = output_dir.join(".obsidian");
    fs::create_dir_all(&obsidian_config_dir).map_err(|e| io_err(&obsidian_config_dir, e))?;

    // 1. Write .obsidian/graph.json with color groups
    let graph_config = json!({
        "collapse-filter": true,
        "search": "",
        "localJumps": 1,
        "colorGroups": [
            { "query": "tag:#code/struct", "color": { "a": 1, "rgb": 3901614 } },       // Blue
            { "query": "tag:#code/class", "color": { "a": 1, "rgb": 3901614 } },        // Blue
            { "query": "tag:#code/function", "color": { "a": 1, "rgb": 2013289 } },     // Emerald
            { "query": "tag:#code/method", "color": { "a": 1, "rgb": 2013289 } },       // Emerald
            { "query": "tag:#code/trait", "color": { "a": 1, "rgb": 9324798 } },        // Purple
            { "query": "tag:#code/interface", "color": { "a": 1, "rgb": 9324798 } },    // Purple
            { "query": "tag:#code/file", "color": { "a": 1, "rgb": 15631410 } }         // Amber
        ]
    });
    let graph_config_file = obsidian_config_dir.join("graph.json");
    fs::write(
        &graph_config_file,
        serde_json::to_string_pretty(&graph_config).unwrap_or_default(),
    )
    .map_err(|e| io_err(&graph_config_file, e))?;

    // Map node id to node for quick lookups
    let node_map: BTreeMap<NodeId, &crate::model::Node> =
        graph.nodes.iter().map(|n| (n.id, n)).collect();

    // Group edges by from (callees/references) and to (callers/referrers)
    let mut outbound: BTreeMap<NodeId, Vec<(crate::model::EdgeKind, NodeId)>> = BTreeMap::new();
    let mut inbound: BTreeMap<NodeId, Vec<(crate::model::EdgeKind, NodeId)>> = BTreeMap::new();

    for edge in &graph.edges {
        outbound
            .entry(edge.from)
            .or_default()
            .push((edge.kind, edge.to));
        inbound
            .entry(edge.to)
            .or_default()
            .push((edge.kind, edge.from));
    }

    // 2. Write 00-Index.md
    let mut index_content = String::new();
    index_content.push_str("# Codebase Knowledge Graph (Obsidian Vault)\n\n");
    index_content.push_str("> Generated automatically by **Graphia**.\n\n");
    index_content.push_str("## Repository Summary\n\n");
    index_content.push_str(&format!(
        "- **Total Symbols (Nodes)**: {}\n",
        graph.nodes.len()
    ));
    index_content.push_str(&format!(
        "- **Total Dependencies (Edges)**: {}\n",
        graph.edges.len()
    ));
    index_content.push_str("\n## Node Kinds\n\n");

    let mut kind_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for node in &graph.nodes {
        *kind_counts.entry(node.kind.as_str()).or_insert(0) += 1;
    }
    for (kind, count) in kind_counts {
        index_content.push_str(&format!("- **{}**: {}\n", kind, count));
    }

    index_content.push_str("\n## Quick Links\n\n");
    index_content.push_str("- [[01-Files|All Indexed Files]]\n");
    index_content.push_str("\n### Key Entrypoints & Hotspots\n\n");

    for node in graph.nodes.iter().take(20) {
        let note_name = sanitize_filename(&node.name);
        index_content.push_str(&format!(
            "- [[symbols/{}/{}|{}]] (`{}` in `{}`)\n",
            node.kind.as_str(),
            note_name,
            node.name,
            node.kind.as_str(),
            node.file
        ));
    }

    let index_file = output_dir.join("00-Index.md");
    fs::write(&index_file, index_content).map_err(|e| io_err(&index_file, e))?;

    // 3. Write symbols/<Kind>/<Name>.md
    for node in &graph.nodes {
        let kind_dir = output_dir.join("symbols").join(node.kind.as_str());
        fs::create_dir_all(&kind_dir).map_err(|e| io_err(&kind_dir, e))?;

        let note_filename = format!("{}.md", sanitize_filename(&node.name));
        let mut note = String::new();

        // Frontmatter
        note.push_str("---\n");
        note.push_str(&format!("name: \"{}\"\n", node.name.replace('"', "\\\"")));
        note.push_str(&format!("kind: {}\n", node.kind.as_str()));
        note.push_str(&format!("file: \"{}\"\n", node.file.replace('"', "\\\"")));
        if let Some(lang) = node.language {
            note.push_str(&format!("language: {}\n", lang.as_str()));
        }
        note.push_str("tags:\n");
        note.push_str("  - code\n");
        note.push_str(&format!("  - code/{}\n", node.kind.as_str().to_lowercase()));
        if let Some(lang) = node.language {
            note.push_str(&format!("  - lang/{}\n", lang.as_str()));
        }
        note.push_str("---\n\n");

        // Body
        note.push_str(&format!("# {}\n\n", node.name));
        note.push_str(&format!("- **Kind**: `{}`\n", node.kind.as_str()));
        let file_note_name = sanitize_filename(&node.file);
        note.push_str(&format!(
            "- **File**: [[files/{}|{}]] (lines {}-{})\n",
            file_note_name, node.file, node.location.start_line, node.location.end_line
        ));

        if let Some(container) = &node.container {
            note.push_str(&format!("- **Container**: `{container}`\n"));
        }
        if let Some(sig) = &node.signature {
            note.push_str(&format!("- **Signature**: `{sig}`\n"));
        }

        note.push_str("\n---\n\n");

        // Inbound references (who calls/uses this)
        note.push_str("## Inbound References (Callers & Users)\n\n");
        if let Some(edges) = inbound.get(&node.id) {
            for (kind, from_id) in edges {
                if let Some(from_node) = node_map.get(from_id) {
                    let from_note = sanitize_filename(&from_node.name);
                    note.push_str(&format!(
                        "- `{}` by [[symbols/{}/{}|{}]] (`{}`)\n",
                        kind.as_str(),
                        from_node.kind.as_str(),
                        from_note,
                        from_node.name,
                        from_node.file
                    ));
                }
            }
        } else {
            note.push_str("*No inbound callers recorded.*\n");
        }

        // Outbound dependencies (who this calls/uses)
        note.push_str("\n## Outbound Dependencies (Callees & Uses)\n\n");
        if let Some(edges) = outbound.get(&node.id) {
            for (kind, to_id) in edges {
                if let Some(to_node) = node_map.get(to_id) {
                    let to_note = sanitize_filename(&to_node.name);
                    note.push_str(&format!(
                        "- `{}` -> [[symbols/{}/{}|{}]] (`{}`)\n",
                        kind.as_str(),
                        to_node.kind.as_str(),
                        to_note,
                        to_node.name,
                        to_node.file
                    ));
                }
            }
        } else {
            note.push_str("*No outbound dependencies recorded.*\n");
        }

        let symbol_file = kind_dir.join(note_filename);
        fs::write(&symbol_file, note).map_err(|e| io_err(&symbol_file, e))?;
    }

    // 4. Write files/<filename>.md and 01-Files.md
    let files_dir = output_dir.join("files");
    fs::create_dir_all(&files_dir).map_err(|e| io_err(&files_dir, e))?;

    let mut files_index = String::new();
    files_index.push_str("# Indexed Files\n\n");

    let mut file_to_nodes: BTreeMap<&str, Vec<&crate::model::Node>> = BTreeMap::new();
    for node in &graph.nodes {
        file_to_nodes.entry(&node.file).or_default().push(node);
    }

    for (file, nodes) in file_to_nodes {
        let file_note_name = sanitize_filename(file);
        files_index.push_str(&format!(
            "- [[files/{}|{}]] ({} symbols)\n",
            file_note_name,
            file,
            nodes.len()
        ));

        let mut file_note = String::new();
        file_note.push_str("---\n");
        file_note.push_str(&format!("file: \"{}\"\n", file.replace('"', "\\\"")));
        file_note.push_str("tags:\n  - code/file\n---\n\n");
        file_note.push_str(&format!("# {}\n\n", file));
        file_note.push_str("## Contained Symbols\n\n");

        for n in nodes {
            let note_name = sanitize_filename(&n.name);
            file_note.push_str(&format!(
                "- [[symbols/{}/{}|{}]] (`{}`)\n",
                n.kind.as_str(),
                note_name,
                n.name,
                n.kind.as_str()
            ));
        }

        let file_path = files_dir.join(format!("{file_note_name}.md"));
        fs::write(&file_path, file_note).map_err(|e| io_err(&file_path, e))?;
    }

    let all_files_index = output_dir.join("01-Files.md");
    fs::write(&all_files_index, files_index).map_err(|e| io_err(&all_files_index, e))?;

    Ok(())
}

fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect()
}
