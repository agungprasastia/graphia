use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use crate::analysis::{
    AnalysisLevel, AnalysisOptions, CommunityConfig, CycleConfig, compute_hotspots,
    detect_communities, find_cycles, project_graph, run_analysis,
};
use crate::model::EdgeKind;

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum CliLevel {
    Symbol,
    File,
    Module,
}

impl From<CliLevel> for AnalysisLevel {
    fn from(level: CliLevel) -> Self {
        match level {
            CliLevel::Symbol => AnalysisLevel::Symbol,
            CliLevel::File => AnalysisLevel::File,
            CliLevel::Module => AnalysisLevel::Module,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum CliEdgeKind {
    Contains,
    Imports,
    Calls,
    Inherits,
    Implements,
}

impl From<CliEdgeKind> for EdgeKind {
    fn from(kind: CliEdgeKind) -> Self {
        match kind {
            CliEdgeKind::Contains => EdgeKind::Contains,
            CliEdgeKind::Imports => EdgeKind::Imports,
            CliEdgeKind::Calls => EdgeKind::Calls,
            CliEdgeKind::Inherits => EdgeKind::Inherits,
            CliEdgeKind::Implements => EdgeKind::Implements,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum CliFormat {
    Human,
    Json,
}

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
    Load {
        repo: PathBuf,
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
    Analyze {
        repo: PathBuf,
        #[arg(long, value_enum, default_value_t = CliLevel::File)]
        level: CliLevel,
        #[arg(long, value_enum)]
        edge: Option<CliEdgeKind>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long, value_enum, default_value_t = CliFormat::Human)]
        format: CliFormat,
    },
    Cycles {
        repo: PathBuf,
        #[arg(long, value_enum, default_value_t = CliLevel::File)]
        level: CliLevel,
        #[arg(long, value_enum)]
        edge: Option<CliEdgeKind>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long, value_enum, default_value_t = CliFormat::Human)]
        format: CliFormat,
    },
    Hotspots {
        repo: PathBuf,
        #[arg(long, value_enum, default_value_t = CliLevel::File)]
        level: CliLevel,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long, value_enum, default_value_t = CliFormat::Human)]
        format: CliFormat,
    },
    Communities {
        repo: PathBuf,
        #[arg(long, value_enum, default_value_t = CliLevel::File)]
        level: CliLevel,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long, value_enum, default_value_t = CliFormat::Human)]
        format: CliFormat,
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
            let counts = change_counts(&changes);
            println!(
                "built graph: {} nodes, {} edges -> {} ({} changed files)",
                graph.node_count(),
                graph.edge_count(),
                output.display(),
                counts.changed
            );
            println!("{}", format_change_summary(&counts));
            Ok(())
        }
        Commands::Load { repo } => {
            let graph = load_or_build(&repo)?;
            println!(
                "loaded graph: {} nodes, {} edges",
                graph.node_count(),
                graph.edge_count()
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
            let matches = index.find(&graph, &symbol);
            if matches.is_empty() {
                return Err(crate::error::GraphiaError::InvalidArgument(
                    "symbol not found".into(),
                ));
            }
            for node in matches {
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
            match index.shortest_path(
                starts[0].id,
                ends[0].id,
                crate::query::TraversalLimits::new(100, 10_000),
            ) {
                Ok(Some(path)) => {
                    println!("{}", starts[0].qualified_name);
                    for edge_id in path {
                        if let Some(edge) = graph.edges.iter().find(|edge| edge.id == edge_id) {
                            if let Some(node) = graph.nodes.iter().find(|node| node.id == edge.to) {
                                println!("{}", node.qualified_name);
                            }
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
            let counts = change_counts(&changes);
            println!(
                "updated graph: {} nodes, {} edges ({} changed files)",
                graph.node_count(),
                graph.edge_count(),
                counts.changed
            );
            println!("{}", format_change_summary(&counts));
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
                let explanation = index.explain(&graph, node.id)?;
                println!(
                    "{} {}\nlocation: {}\nparent: {:?}\nincoming: {:?}\noutgoing: {:?}\ncallers: {:?}\ncallees: {:?}\nimports: {:?}",
                    explanation.kind.as_str(),
                    node.qualified_name,
                    explanation.location,
                    explanation.parent,
                    explanation.incoming,
                    explanation.outgoing,
                    explanation.callers,
                    explanation.callees,
                    explanation.imports
                );
            }
            Ok(())
        }
        Commands::Analyze {
            repo,
            level,
            edge,
            limit,
            format,
        } => {
            let graph = load_or_build(&repo)?;
            let options = AnalysisOptions {
                level: level.into(),
                edge_filter: edge.map(Into::into),
                limit,
            };
            let report = run_analysis(&graph, options);

            match format {
                CliFormat::Json => {
                    let json = serde_json::to_string_pretty(&report).map_err(|e| {
                        crate::error::GraphiaError::Storage {
                            message: e.to_string(),
                        }
                    })?;
                    println!("{json}");
                }
                CliFormat::Human => {
                    println!(
                        "Analysis Summary (level={}, nodes={}, edges={}):",
                        report.level.as_str(),
                        report.node_count,
                        report.edge_count
                    );
                    println!(
                        "  SCCs: {} (non-trivial: {})",
                        report.sccs.len(),
                        report.sccs.iter().filter(|s| !s.is_trivial).count()
                    );
                    println!("  Cycles: {}", report.cycles.len());
                    println!("  Communities: {}", report.communities.len());
                    if !report.hotspots.is_empty() {
                        println!("  Top Hotspots:");
                        for (i, h) in report.hotspots.iter().take(5).enumerate() {
                            println!(
                                "    {}. {} (score={:.2}, in={}, out={}, scc={})",
                                i + 1,
                                h.id,
                                h.score,
                                h.fan_in,
                                h.fan_out,
                                h.in_scc
                            );
                        }
                    }
                }
            }
            Ok(())
        }
        Commands::Cycles {
            repo,
            level,
            edge,
            limit,
            format,
        } => {
            let graph = load_or_build(&repo)?;
            let projected = project_graph(&graph, level.into(), edge.map(Into::into));
            let adj = projected.to_adjacency();
            let mut cycles = find_cycles(&adj, CycleConfig::default());
            if let Some(limit) = limit {
                cycles.truncate(limit);
            }

            match format {
                CliFormat::Json => {
                    #[derive(serde::Serialize)]
                    struct CyclesJsonOutput {
                        analysis_version: u32,
                        level: &'static str,
                        cycle_count: usize,
                        cycles: Vec<crate::analysis::Cycle>,
                    }
                    let out = CyclesJsonOutput {
                        analysis_version: 1,
                        level: AnalysisLevel::from(level).as_str(),
                        cycle_count: cycles.len(),
                        cycles,
                    };
                    let json = serde_json::to_string_pretty(&out).map_err(|e| {
                        crate::error::GraphiaError::Storage {
                            message: e.to_string(),
                        }
                    })?;
                    println!("{json}");
                }
                CliFormat::Human => {
                    println!(
                        "Found {} cycle(s) at {} level:",
                        cycles.len(),
                        AnalysisLevel::from(level).as_str()
                    );
                    for (i, cycle) in cycles.iter().enumerate() {
                        println!("  Cycle #{}: length {}", i + 1, cycle.length);
                        for (step, node) in cycle.path.iter().enumerate() {
                            let next_node = &cycle.path[(step + 1) % cycle.length];
                            println!("    {} -> {}", node, next_node);
                        }
                    }
                }
            }
            Ok(())
        }
        Commands::Hotspots {
            repo,
            level,
            limit,
            format,
        } => {
            let graph = load_or_build(&repo)?;
            let projected = project_graph(&graph, level.into(), None);
            let adj = projected.to_adjacency();
            let mut hotspots = compute_hotspots(&adj);
            if let Some(limit) = limit {
                hotspots.truncate(limit);
            }

            match format {
                CliFormat::Json => {
                    #[derive(serde::Serialize)]
                    struct HotspotsJsonOutput {
                        analysis_version: u32,
                        level: &'static str,
                        hotspot_count: usize,
                        hotspots: Vec<crate::analysis::Hotspot>,
                    }
                    let out = HotspotsJsonOutput {
                        analysis_version: 1,
                        level: AnalysisLevel::from(level).as_str(),
                        hotspot_count: hotspots.len(),
                        hotspots,
                    };
                    let json = serde_json::to_string_pretty(&out).map_err(|e| {
                        crate::error::GraphiaError::Storage {
                            message: e.to_string(),
                        }
                    })?;
                    println!("{json}");
                }
                CliFormat::Human => {
                    println!(
                        "Top {} hotspot(s) at {} level:",
                        hotspots.len(),
                        AnalysisLevel::from(level).as_str()
                    );
                    for (i, h) in hotspots.iter().enumerate() {
                        println!(
                            "  {}. {} (score={:.2}, fan_in={}, fan_out={}, pagerank={:.4}, in_scc={})",
                            i + 1,
                            h.id,
                            h.score,
                            h.fan_in,
                            h.fan_out,
                            h.pagerank,
                            h.in_scc
                        );
                    }
                }
            }
            Ok(())
        }
        Commands::Communities {
            repo,
            level,
            limit,
            format,
        } => {
            let graph = load_or_build(&repo)?;
            let projected = project_graph(&graph, level.into(), None);
            let adj = projected.to_adjacency();
            let mut communities = detect_communities(&adj, CommunityConfig::default());
            if let Some(limit) = limit {
                communities.truncate(limit);
            }

            match format {
                CliFormat::Json => {
                    #[derive(serde::Serialize)]
                    struct CommunitiesJsonOutput {
                        analysis_version: u32,
                        level: &'static str,
                        community_count: usize,
                        communities: Vec<crate::analysis::Community>,
                    }
                    let out = CommunitiesJsonOutput {
                        analysis_version: 1,
                        level: AnalysisLevel::from(level).as_str(),
                        community_count: communities.len(),
                        communities,
                    };
                    let json = serde_json::to_string_pretty(&out).map_err(|e| {
                        crate::error::GraphiaError::Storage {
                            message: e.to_string(),
                        }
                    })?;
                    println!("{json}");
                }
                CliFormat::Human => {
                    println!(
                        "Detected {} community(ies) at {} level:",
                        communities.len(),
                        AnalysisLevel::from(level).as_str()
                    );
                    for (i, c) in communities.iter().enumerate() {
                        println!(
                            "  Community #{} (size={}, internal_edges={}, external_edges={}):",
                            i + 1,
                            c.size,
                            c.internal_edges,
                            c.external_edges
                        );
                        for member in &c.members {
                            println!("    - {}", member);
                        }
                    }
                }
            }
            Ok(())
        }
    }
}

#[derive(Debug, Default)]
struct ChangeCounts {
    added: usize,
    modified: usize,
    deleted: usize,
    unchanged: usize,
    changed: usize,
}

fn change_counts(changes: &[crate::storage::FileChangeRecord]) -> ChangeCounts {
    let mut counts = ChangeCounts::default();
    for change in changes {
        match change.change {
            crate::storage::FileChange::Added => counts.added += 1,
            crate::storage::FileChange::Modified => counts.modified += 1,
            crate::storage::FileChange::Deleted => counts.deleted += 1,
            crate::storage::FileChange::Unchanged => counts.unchanged += 1,
        }
    }
    counts.changed = counts.added + counts.modified + counts.deleted;
    counts
}

fn format_change_summary(counts: &ChangeCounts) -> String {
    format!(
        "changes: added={}, modified={}, deleted={}, unchanged={}",
        counts.added, counts.modified, counts.deleted, counts.unchanged
    )
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
    use tempfile::tempdir;

    #[test]
    fn change_summary_is_deterministic() {
        let counts = ChangeCounts {
            added: 1,
            modified: 2,
            deleted: 3,
            unchanged: 4,
            changed: 6,
        };
        assert_eq!(
            format_change_summary(&counts),
            "changes: added=1, modified=2, deleted=3, unchanged=4"
        );
    }

    #[test]
    fn cli_build_accepts_clean_flag() {
        let cli = Cli::try_parse_from(["graphia", "build", ".", "--clean"]).expect("parse");
        assert!(matches!(cli.command, Commands::Build { clean: true, .. }));
    }

    #[test]
    fn cli_load_accepts_repository() {
        let cli = Cli::try_parse_from(["graphia", "load", "."]).expect("parse");
        assert!(matches!(cli.command, Commands::Load { .. }));
    }

    #[test]
    fn cli_load_reads_existing_repository() {
        let repo = tempdir().expect("temporary repository");
        let graph = crate::graph::Graph::new(vec![], vec![]);
        crate::storage::save_graph_binary(&graph, &repo.path().join(".graphia/index.bin"))
            .expect("save index");
        run(Cli {
            command: Commands::Load {
                repo: repo.path().to_path_buf(),
            },
        })
        .expect("load graph");
    }

    #[test]
    fn cli_query_reports_missing_symbol() {
        let repo = tempdir().expect("temporary repository");
        let graph = crate::graph::Graph::new(vec![], vec![]);
        crate::storage::save_graph_json(&graph, &repo.path().join("graph.json"))
            .expect("save graph");
        let result = run(Cli {
            command: Commands::Query {
                repo: repo.path().to_path_buf(),
                symbol: "missing".to_string(),
            },
        });
        assert!(matches!(
            result,
            Err(crate::error::GraphiaError::InvalidArgument(message)) if message == "symbol not found"
        ));
    }

    #[test]
    fn cli_export_reads_binary_index_and_writes_json() {
        let repo = tempdir().expect("temporary repository");
        let graph = crate::graph::Graph::new(vec![], vec![]);
        crate::storage::save_graph_binary(&graph, &repo.path().join(".graphia/index.bin"))
            .expect("save index");
        run(Cli {
            command: Commands::Export {
                repo: repo.path().to_path_buf(),
                format: "json".to_string(),
            },
        })
        .expect("export graph");
        assert!(repo.path().join("graph.json").exists());
        assert_eq!(
            crate::storage::load_graph_json(&repo.path().join("graph.json")).expect("load json"),
            graph
        );
    }

    #[test]
    fn cli_analyze_subcommand_json_and_human() {
        let repo = tempdir().expect("temporary repository");
        let graph = crate::graph::Graph::new(vec![], vec![]);
        crate::storage::save_graph_json(&graph, &repo.path().join("graph.json"))
            .expect("save graph");

        run(Cli {
            command: Commands::Analyze {
                repo: repo.path().to_path_buf(),
                level: CliLevel::File,
                edge: None,
                limit: Some(10),
                format: CliFormat::Human,
            },
        })
        .expect("run analyze human");

        run(Cli {
            command: Commands::Analyze {
                repo: repo.path().to_path_buf(),
                level: CliLevel::Module,
                edge: Some(CliEdgeKind::Imports),
                limit: Some(5),
                format: CliFormat::Json,
            },
        })
        .expect("run analyze json");
    }

    #[test]
    fn cli_cycles_subcommand_options() {
        let repo = tempdir().expect("temporary repository");
        let graph = crate::graph::Graph::new(vec![], vec![]);
        crate::storage::save_graph_json(&graph, &repo.path().join("graph.json"))
            .expect("save graph");

        run(Cli {
            command: Commands::Cycles {
                repo: repo.path().to_path_buf(),
                level: CliLevel::File,
                edge: None,
                limit: Some(10),
                format: CliFormat::Json,
            },
        })
        .expect("run cycles");
    }

    #[test]
    fn cli_hotspots_and_communities_subcommands() {
        let repo = tempdir().expect("temporary repository");
        let graph = crate::graph::Graph::new(vec![], vec![]);
        crate::storage::save_graph_json(&graph, &repo.path().join("graph.json"))
            .expect("save graph");

        run(Cli {
            command: Commands::Hotspots {
                repo: repo.path().to_path_buf(),
                level: CliLevel::File,
                limit: Some(5),
                format: CliFormat::Json,
            },
        })
        .expect("run hotspots");

        run(Cli {
            command: Commands::Communities {
                repo: repo.path().to_path_buf(),
                level: CliLevel::Module,
                limit: Some(5),
                format: CliFormat::Json,
            },
        })
        .expect("run communities");
    }
}
