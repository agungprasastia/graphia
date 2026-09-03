pub mod cytoscape;
pub mod dot;
pub mod gexf;
pub mod graphml;
pub mod mermaid;
pub mod obsidian;

use std::fs;
use std::path::{Path, PathBuf};

pub use cytoscape::export_cytoscape;
pub use dot::export_dot;
pub use gexf::export_gexf;
pub use graphml::export_graphml;
pub use mermaid::export_mermaid;
pub use obsidian::export_obsidian;

use crate::error::{GraphiaError, Result};
use crate::graph::Graph;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Json,
    Dot,
    Mermaid,
    Graphml,
    Gexf,
    Cytoscape,
    Obsidian,
}

impl ExportFormat {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "json" => Some(Self::Json),
            "dot" | "graphviz" => Some(Self::Dot),
            "mermaid" | "mmd" => Some(Self::Mermaid),
            "graphml" => Some(Self::Graphml),
            "gexf" | "gephi" => Some(Self::Gexf),
            "cytoscape" | "cyto" => Some(Self::Cytoscape),
            "obsidian" | "vault" => Some(Self::Obsidian),
            _ => None,
        }
    }

    #[must_use]
    pub fn default_extension(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Dot => "dot",
            Self::Mermaid => "mmd",
            Self::Graphml => "graphml",
            Self::Gexf => "gexf",
            Self::Cytoscape => "cyto.json",
            Self::Obsidian => "obsidian-vault",
        }
    }
}

pub(crate) fn io_err<P: AsRef<Path>, E: ToString>(path: P, err: E) -> GraphiaError {
    GraphiaError::Io {
        path: path.as_ref().to_path_buf(),
        message: err.to_string(),
    }
}

/// Export a graph to a string in the specified format (for text-based formats).
pub fn export_to_string(graph: &Graph, format: ExportFormat) -> Result<String> {
    match format {
        ExportFormat::Json => crate::storage::graph_to_json_string(graph),
        ExportFormat::Dot => Ok(export_dot(graph)),
        ExportFormat::Mermaid => Ok(export_mermaid(graph, None)),
        ExportFormat::Graphml => Ok(export_graphml(graph)),
        ExportFormat::Gexf => Ok(export_gexf(graph)),
        ExportFormat::Cytoscape => Ok(export_cytoscape(graph)),
        ExportFormat::Obsidian => Err(GraphiaError::InvalidArgument(
            "obsidian format requires a directory output path".into(),
        )),
    }
}

/// Export a graph to a file or directory depending on format.
pub fn export_graph(
    graph: &Graph,
    format_str: &str,
    output: Option<&Path>,
    repo: &Path,
) -> Result<PathBuf> {
    let format = ExportFormat::parse(format_str).ok_or_else(|| {
        GraphiaError::InvalidArgument(format!(
            "unsupported export format '{format_str}'. Supported: json, dot, mermaid, graphml, gexf, cytoscape, obsidian"
        ))
    })?;

    let destination = match output {
        Some(path) => path.to_path_buf(),
        None => match format {
            ExportFormat::Obsidian => repo.join("graphia-vault"),
            _ => repo.join(format!("graph.{}", format.default_extension())),
        },
    };

    match format {
        ExportFormat::Obsidian => {
            export_obsidian(graph, &destination)?;
        }
        ExportFormat::Json => {
            crate::storage::save_graph_json(graph, &destination)?;
        }
        _ => {
            let content = export_to_string(graph, format)?;
            if let Some(parent) = destination.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent).map_err(|e| io_err(parent, e))?;
            }
            fs::write(&destination, content).map_err(|e| io_err(&destination, e))?;
        }
    }

    Ok(destination)
}
