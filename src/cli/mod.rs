use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "graphia", version, about = "Native code graph engine")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Scan {
        repo: PathBuf,
    },
    Build {
        repo: PathBuf,
        #[arg(long)]
        clean: bool,
    },
    Stats {
        repo: PathBuf,
    },
    Query {
        repo: PathBuf,
        symbol: String,
    },
    Path {
        repo: PathBuf,
        from: String,
        to: String,
    },
    Update {
        repo: PathBuf,
    },
    Export {
        repo: PathBuf,
        #[arg(long, default_value = "json")]
        format: String,
    },
    Explain {
        repo: PathBuf,
        symbol: String,
    },
}

/// Execute selected command.
///
/// # Errors
///
/// Returns an error when repository scanning, graph construction, or storage fails.
pub fn run(cli: Cli) -> crate::error::Result<()> {
    match cli.command {
        Commands::Scan { repo } => {
            let files = crate::scan::scan_repo(&repo)?;
            for f in &files {
                let lang = f
                    .language
                    .map_or_else(|| "-".to_string(), |l| l.as_str().to_string());
                println!("{} [{lang}]", f.relative_path);
            }
            eprintln!("scanned {} files", files.len());
            Ok(())
        }
        Commands::Build { repo, clean } => {
            let (graph, changes) = crate::storage::build_or_update(&repo, clean)?;
            let output = repo.join("graph.json");
            crate::storage::save_graph_json(&graph, &output)?;
            crate::storage::save_graph_binary(&graph, &repo.join(".graphia/index.bin"))?;
            println!(
                "built graph: {} nodes, {} edges -> {} ({} changed files)",
                graph.node_count(),
                graph.edge_count(),
                output.display(),
                changes
                    .iter()
                    .filter(|change| !matches!(
                        change.change,
                        crate::storage::FileChange::Unchanged
                    ))
                    .count()
            );
            Ok(())
        }
        Commands::Stats { repo } => {
            let graph_path = repo.join("graph.json");
            let graph = if graph_path.exists() {
                crate::storage::load_graph_json(&graph_path)?
            } else {
                crate::storage::build_graph_from_repo(&repo)?
            };
            println!("nodes: {}", graph.node_count());
            println!("edges: {}", graph.edge_count());
            let mut by_kind = std::collections::BTreeMap::new();
            for n in &graph.nodes {
                *by_kind.entry(n.kind.as_str()).or_insert(0usize) += 1;
            }
            for (k, v) in by_kind {
                println!("  {k}: {v}");
            }
            let mut by_edge = std::collections::BTreeMap::new();
            for e in &graph.edges {
                *by_edge.entry(e.kind.as_str()).or_insert(0usize) += 1;
            }
            for (k, v) in by_edge {
                println!("  edge {k}: {v}");
            }
            Ok(())
        }
        Commands::Query { repo, symbol } => {
            let graph = load_or_build(&repo)?;
            let index = crate::query::QueryIndex::new(&graph);
            for node in index.find(&graph, &symbol) {
                println!("{} {}", node.kind.as_str(), node.qualified_name);
            }
            Ok(())
        }
        Commands::Path { repo, from, to } => {
            let graph = load_or_build(&repo)?;
            let index = crate::query::QueryIndex::new(&graph);
            let starts = index.find(&graph, &from);
            let ends = index.find(&graph, &to);
            if starts.len() != 1 || ends.len() != 1 {
                return Err(crate::error::GraphiaError::InvalidArgument(
                    "path endpoints must each resolve to exactly one symbol".into(),
                ));
            }
            match index.shortest_path(starts[0].id, ends[0].id, 100, 10_000) {
                Ok(Some(path)) => {
                    for id in path {
                        if let Some(node) = graph.nodes.iter().find(|node| node.id == id) {
                            println!("{}", node.qualified_name);
                        }
                    }
                }
                Ok(None) => println!("no path"),
                Err(limit) => {
                    return Err(crate::error::GraphiaError::InvalidArgument(format!(
                        "path traversal limit exceeded after {} nodes",
                        limit.visited
                    )));
                }
            }
            Ok(())
        }

        Commands::Update { repo } => {
            let (graph, changes) = crate::storage::build_or_update(&repo, false)?;
            crate::storage::save_graph_json(&graph, &repo.join("graph.json"))?;
            crate::storage::save_graph_binary(&graph, &repo.join(".graphia/index.bin"))?;
            println!(
                "updated graph: {} nodes, {} edges ({} changed files)",
                graph.node_count(),
                graph.edge_count(),
                changes
                    .iter()
                    .filter(|change| !matches!(
                        change.change,
                        crate::storage::FileChange::Unchanged
                    ))
                    .count()
            );
            Ok(())
        }
        Commands::Export { repo, format } => {
            if format != "json" {
                return Err(crate::error::GraphiaError::InvalidArgument(
                    "only json export is supported".into(),
                ));
            }
            let graph = if repo.join(".graphia/index.bin").exists() {
                crate::storage::load_graph_binary(&repo.join(".graphia/index.bin"))?
            } else {
                crate::storage::load_graph_json(&repo.join("graph.json"))?
            };
            crate::storage::save_graph_json(&graph, &repo.join("graph.json"))?;
            println!("exported json");
            Ok(())
        }
        Commands::Explain { repo, symbol } => {
            let graph = load_or_build(&repo)?;
            let index = crate::query::QueryIndex::new(&graph);
            let matches = index.find(&graph, &symbol);
            if matches.is_empty() {
                return Err(crate::error::GraphiaError::InvalidArgument(
                    "symbol not found".into(),
                ));
            }
            for node in matches {
                println!("{}", index.explain(&graph, node));
            }
            Ok(())
        }
    }
}

fn load_or_build(repo: &std::path::Path) -> crate::error::Result<crate::graph::Graph> {
    if repo.join(".graphia/index.bin").exists() {
        crate::storage::load_graph_binary(&repo.join(".graphia/index.bin"))
    } else if repo.join("graph.json").exists() {
        crate::storage::load_graph_json(&repo.join("graph.json"))
    } else {
        crate::storage::build_graph_from_repo(repo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_build_accepts_clean_flag() {
        let cli = Cli::try_parse_from(["graphia", "build", ".", "--clean"]).expect("parse");
        assert!(matches!(cli.command, Commands::Build { clean: true, .. }));
    }
}
