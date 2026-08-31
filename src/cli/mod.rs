use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use crate::analysis::{
    AnalysisLevel, AnalysisOptions, CommunityConfig, CycleConfig, compute_hotspots,
    detect_communities, find_cycles, project_graph, run_analysis,
};
use crate::context::{BudgetValueType, ContextRequest, generate_context};
use crate::intelligence::{
    NeighborhoodOptions, SearchOptions, analyze_impact, detect_entrypoints, discover_tests,
    get_architecture_overview, get_neighborhood, map_source_to_tests, search_graph,
};
use crate::model::{EdgeKind, NodeKind};

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum CliBudgetType {
    Tokens,
    Bytes,
    Chars,
}

impl From<CliBudgetType> for BudgetValueType {
    fn from(b: CliBudgetType) -> Self {
        match b {
            CliBudgetType::Tokens => BudgetValueType::ApproxTokens,
            CliBudgetType::Bytes => BudgetValueType::Bytes,
            CliBudgetType::Chars => BudgetValueType::Characters,
        }
    }
}

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
pub enum CliNodeKind {
    File,
    Module,
    Function,
    Method,
    Class,
    Struct,
    Trait,
    Interface,
}

impl From<CliNodeKind> for NodeKind {
    fn from(kind: CliNodeKind) -> Self {
        match kind {
            CliNodeKind::File => NodeKind::File,
            CliNodeKind::Module => NodeKind::Module,
            CliNodeKind::Function => NodeKind::Function,
            CliNodeKind::Method => NodeKind::Method,
            CliNodeKind::Class => NodeKind::Class,
            CliNodeKind::Struct => NodeKind::Struct,
            CliNodeKind::Trait => NodeKind::Trait,
            CliNodeKind::Interface => NodeKind::Interface,
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
    Search {
        repo: PathBuf,
        query: String,
        #[arg(long, value_enum)]
        kind: Option<CliNodeKind>,
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long, value_enum, default_value_t = CliFormat::Human)]
        format: CliFormat,
    },
    Neighborhood {
        repo: PathBuf,
        target: String,
        #[arg(long, default_value_t = 1)]
        depth: usize,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long, value_enum, default_value_t = CliFormat::Human)]
        format: CliFormat,
    },
    Impact {
        repo: PathBuf,
        target: String,
        #[arg(long, default_value_t = 3)]
        depth: usize,
        #[arg(long)]
        files: bool,
        #[arg(long, value_enum, default_value_t = CliFormat::Human)]
        format: CliFormat,
    },
    Tests {
        repo: PathBuf,
        #[arg(long)]
        target: Option<String>,
        #[arg(long, value_enum, default_value_t = CliFormat::Human)]
        format: CliFormat,
    },
    Entrypoints {
        repo: PathBuf,
        #[arg(long, value_enum, default_value_t = CliFormat::Human)]
        format: CliFormat,
    },
    Architecture {
        repo: PathBuf,
        #[arg(long, value_enum, default_value_t = CliFormat::Human)]
        format: CliFormat,
    },
    Context {
        repo: PathBuf,
        #[arg(long)]
        symbol: Option<String>,
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        query: Option<String>,
        #[arg(long)]
        changed: bool,
        #[arg(long)]
        token_budget: Option<usize>,
        #[arg(long, value_enum, default_value_t = CliBudgetType::Tokens)]
        budget_type: CliBudgetType,
        #[arg(long, default_value_t = 3)]
        depth: usize,
        #[arg(long, default_value_t = 100)]
        limit: usize,
        #[arg(long, value_enum, default_value_t = CliFormat::Human)]
        format: CliFormat,
    },
    Mcp {
        repo: Option<PathBuf>,
        #[arg(long)]
        auto_index: bool,
    },
    Daemon {
        #[command(subcommand)]
        action: Option<DaemonAction>,
        #[arg(long)]
        repo: Option<PathBuf>,
        #[arg(long)]
        debounce_ms: Option<u64>,
    },
    DaemonStatus {
        #[arg(long)]
        repo: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = CliFormat::Human)]
        format: CliFormat,
    },
    Flow {
        repo: Option<PathBuf>,
        #[arg(long)]
        source: String,
        #[arg(long)]
        sink: String,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long, value_enum, default_value_t = CliFormat::Human)]
        format: CliFormat,
    },
    ArchitectureCheck {
        repo: Option<PathBuf>,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = CliFormat::Human)]
        format: CliFormat,
    },
    History {
        repo: Option<PathBuf>,
        #[arg(long)]
        max_commits: Option<usize>,
        #[arg(long, value_enum, default_value_t = CliFormat::Human)]
        format: CliFormat,
    },
    Cochange {
        repo: Option<PathBuf>,
        #[arg(long)]
        min_support: Option<f64>,
        #[arg(long, value_enum, default_value_t = CliFormat::Human)]
        format: CliFormat,
    },
    Deadcode {
        repo: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = CliFormat::Human)]
        format: CliFormat,
    },
    Diff {
        old_index: PathBuf,
        new_index: PathBuf,
        #[arg(long, value_enum, default_value_t = CliFormat::Human)]
        format: CliFormat,
    },
    ApiDiff {
        old_index: PathBuf,
        new_index: PathBuf,
        #[arg(long, value_enum, default_value_t = CliFormat::Human)]
        format: CliFormat,
    },
}

#[derive(Debug, Subcommand)]
pub enum DaemonAction {
    Status {
        #[arg(long)]
        repo: Option<PathBuf>,
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
        Commands::Search {
            repo,
            query,
            kind,
            file,
            limit,
            format,
        } => {
            let graph = load_or_build(&repo)?;
            let options = SearchOptions {
                query,
                kind_filter: kind.map(Into::into),
                file_filter: file,
                limit,
            };
            let results = search_graph(&graph, &options);

            match format {
                CliFormat::Json => {
                    let json = serde_json::to_string_pretty(&results).map_err(|e| {
                        crate::error::GraphiaError::Storage {
                            message: e.to_string(),
                        }
                    })?;
                    println!("{json}");
                }
                CliFormat::Human => {
                    println!("Found {} search result(s):", results.len());
                    for (i, res) in results.iter().enumerate() {
                        println!(
                            "  {}. [{}] {} (score: {:.2}, file: {}:{}:{})",
                            i + 1,
                            res.node.kind.as_str(),
                            res.node.qualified_name,
                            res.score.total_score,
                            res.node.location.file,
                            res.node.location.start_line,
                            res.node.location.start_col
                        );
                    }
                }
            }
            Ok(())
        }
        Commands::Neighborhood {
            repo,
            target,
            depth,
            limit,
            format,
        } => {
            let graph = load_or_build(&repo)?;
            let options = NeighborhoodOptions {
                target: target.clone(),
                depth,
                limit,
            };
            let Some(neighborhood) = get_neighborhood(&graph, &options) else {
                return Err(crate::error::GraphiaError::InvalidArgument(format!(
                    "target '{target}' not found"
                )));
            };

            match format {
                CliFormat::Json => {
                    let json = serde_json::to_string_pretty(&neighborhood).map_err(|e| {
                        crate::error::GraphiaError::Storage {
                            message: e.to_string(),
                        }
                    })?;
                    println!("{json}");
                }
                CliFormat::Human => {
                    println!(
                        "Structural Neighborhood for [{}] {}:",
                        neighborhood.target.kind.as_str(),
                        neighborhood.target.qualified_name
                    );
                    if let Some(c) = &neighborhood.container {
                        println!("  Container: [{}] {}", c.kind.as_str(), c.qualified_name);
                    }
                    if let Some(m) = &neighborhood.parent_module {
                        println!(
                            "  Parent Module: [{}] {}",
                            m.kind.as_str(),
                            m.qualified_name
                        );
                    }
                    println!("  Children ({}):", neighborhood.children.len());
                    for child in &neighborhood.children {
                        println!("    - [{}] {}", child.kind.as_str(), child.qualified_name);
                    }
                    println!("  Callers ({}):", neighborhood.callers.len());
                    for c in &neighborhood.callers {
                        println!("    - [{}] {}", c.kind.as_str(), c.qualified_name);
                    }
                    println!("  Callees ({}):", neighborhood.callees.len());
                    for c in &neighborhood.callees {
                        println!("    - [{}] {}", c.kind.as_str(), c.qualified_name);
                    }
                    println!("  Imports ({}):", neighborhood.imports.len());
                    for imp in &neighborhood.imports {
                        println!("    - [{}] {}", imp.kind.as_str(), imp.qualified_name);
                    }
                    println!("  Exports ({}):", neighborhood.exports.len());
                    for exp in &neighborhood.exports {
                        println!("    - [{}] {}", exp.kind.as_str(), exp.qualified_name);
                    }
                    println!(
                        "  Referenced Types ({}):",
                        neighborhood.referenced_types.len()
                    );
                    for rt in &neighborhood.referenced_types {
                        println!("    - [{}] {}", rt.kind.as_str(), rt.qualified_name);
                    }
                    println!(
                        "  Trait/Interface Impls ({}):",
                        neighborhood.trait_implementations.len()
                    );
                    for ti in &neighborhood.trait_implementations {
                        println!("    - [{}] {}", ti.kind.as_str(), ti.qualified_name);
                    }
                    println!("  Related Tests ({}):", neighborhood.related_tests.len());
                    for t in &neighborhood.related_tests {
                        println!("    - [{}] {}", t.kind.as_str(), t.qualified_name);
                    }
                }
            }
            Ok(())
        }
        Commands::Impact {
            repo,
            target,
            depth,
            files,
            format,
        } => {
            let graph = load_or_build(&repo)?;
            let Some(analysis) = analyze_impact(&graph, &target, depth) else {
                return Err(crate::error::GraphiaError::InvalidArgument(format!(
                    "target '{target}' not found"
                )));
            };

            match format {
                CliFormat::Json => {
                    let json = serde_json::to_string_pretty(&analysis).map_err(|e| {
                        crate::error::GraphiaError::Storage {
                            message: e.to_string(),
                        }
                    })?;
                    println!("{json}");
                }
                CliFormat::Human => {
                    println!(
                        "Change Surface & Blast Radius for [{}] {}:",
                        analysis.target.kind.as_str(),
                        analysis.target.qualified_name
                    );
                    println!(
                        "  Total Impacted: {} (direct: {}, transitive: {}, possible: {})",
                        analysis.total_impacted,
                        analysis.direct_count,
                        analysis.transitive_count,
                        analysis.possible_count
                    );
                    if files {
                        println!("  Impacted Files ({}):", analysis.impacted_files.len());
                        for f in &analysis.impacted_files {
                            println!("    - {f}");
                        }
                        println!("  Related Tests ({}):", analysis.related_tests.len());
                        for t in &analysis.related_tests {
                            println!("    - {t}");
                        }
                    } else {
                        println!("  Impacted Symbols:");
                        for imp in &analysis.impacted_nodes {
                            println!(
                                "    [{}] [{}] {} (depth: {}, because: {})",
                                imp.kind.as_str(),
                                imp.node.kind.as_str(),
                                imp.node.qualified_name,
                                imp.depth,
                                imp.explanation.because
                            );
                        }
                        println!("  Impacted Files ({}):", analysis.impacted_files.len());
                        for f in &analysis.impacted_files {
                            println!("    - {f}");
                        }
                        println!("  Related Tests ({}):", analysis.related_tests.len());
                        for t in &analysis.related_tests {
                            println!("    - {t}");
                        }
                    }
                }
            }
            Ok(())
        }
        Commands::Tests {
            repo,
            target,
            format,
        } => {
            let graph = load_or_build(&repo)?;

            if let Some(target_spec) = target {
                let tests = map_source_to_tests(&graph, &target_spec);
                match format {
                    CliFormat::Json => {
                        let json = serde_json::to_string_pretty(&tests).map_err(|e| {
                            crate::error::GraphiaError::Storage {
                                message: e.to_string(),
                            }
                        })?;
                        println!("{json}");
                    }
                    CliFormat::Human => {
                        println!("Discovered {} test(s) for '{target_spec}':", tests.len());
                        for (i, t) in tests.iter().enumerate() {
                            let sym = t.test_symbol.as_deref().unwrap_or("<file-level>");
                            println!(
                                "  {}. {} ({}) - reason: {}",
                                i + 1,
                                sym,
                                t.test_file,
                                t.reason
                            );
                        }
                    }
                }
            } else {
                let report = discover_tests(&graph);
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
                        println!("Discovered {} total test mapping(s):", report.total_tests);
                        for mapping in &report.mappings {
                            let src = mapping
                                .source_symbol
                                .as_deref()
                                .unwrap_or(&mapping.source_file);
                            println!("  Source: {src}");
                            for t in &mapping.tests {
                                let sym = t.test_symbol.as_deref().unwrap_or("<file-level>");
                                println!("    -> Test: {} ({}) [{}]", sym, t.test_file, t.reason);
                            }
                        }
                    }
                }
            }
            Ok(())
        }
        Commands::Entrypoints { repo, format } => {
            let graph = load_or_build(&repo)?;
            let entrypoints = detect_entrypoints(&graph);

            match format {
                CliFormat::Json => {
                    let json = serde_json::to_string_pretty(&entrypoints).map_err(|e| {
                        crate::error::GraphiaError::Storage {
                            message: e.to_string(),
                        }
                    })?;
                    println!("{json}");
                }
                CliFormat::Human => {
                    println!("Detected {} entrypoint(s):", entrypoints.len());
                    for (i, ep) in entrypoints.iter().enumerate() {
                        println!(
                            "  {}. [{:?}] {} (file: {}:{}:{}) - {}",
                            i + 1,
                            ep.kind,
                            ep.node.qualified_name,
                            ep.node.location.file,
                            ep.node.location.start_line,
                            ep.node.location.start_col,
                            ep.description
                        );
                    }
                }
            }
            Ok(())
        }
        Commands::Architecture { repo, format } => {
            let graph = load_or_build(&repo)?;
            let overview = get_architecture_overview(&graph);

            match format {
                CliFormat::Json => {
                    let json = serde_json::to_string_pretty(&overview).map_err(|e| {
                        crate::error::GraphiaError::Storage {
                            message: e.to_string(),
                        }
                    })?;
                    println!("{json}");
                }
                CliFormat::Human => {
                    println!("Repository Architectural Overview:");
                    println!(
                        "  Totals: {} nodes (files: {}, symbols: {}), {} edges, {} modules",
                        overview.total_nodes,
                        overview.file_count,
                        overview.symbol_count,
                        overview.total_edges,
                        overview.module_count
                    );
                    println!("  Entrypoints ({}):", overview.entrypoints.len());
                    for ep in &overview.entrypoints {
                        println!("    - [{:?}] {}", ep.kind, ep.node.qualified_name);
                    }
                    println!("  Cycle Count: {}", overview.cycle_count);
                    println!("  Communities: {}", overview.communities.len());
                    println!("  High-Centrality Modules:");
                    for (i, m) in overview.high_centrality_modules.iter().enumerate() {
                        println!(
                            "    {}. {} (pagerank: {:.4}, in: {}, out: {})",
                            i + 1,
                            m.id,
                            m.pagerank,
                            m.in_degree,
                            m.out_degree
                        );
                    }
                    println!("  Primary Dependency Flows:");
                    for flow in overview.dependency_direction.iter().take(10) {
                        println!("    {} -> {} (weight: {})", flow.from, flow.to, flow.weight);
                    }
                }
            }
            Ok(())
        }
        Commands::Context {
            repo,
            symbol,
            file,
            query,
            changed,
            token_budget,
            budget_type,
            depth,
            limit,
            format,
        } => {
            let graph = load_or_build(&repo)?;
            let req = ContextRequest {
                symbol,
                file,
                query,
                changed,
                budget: token_budget,
                budget_type: budget_type.into(),
                max_depth: depth,
                max_candidates: limit,
            };

            let bundle = generate_context(&graph, &req, Some(&repo));

            match format {
                CliFormat::Json => {
                    let json = serde_json::to_string_pretty(&bundle).map_err(|e| {
                        crate::error::GraphiaError::Storage {
                            message: e.to_string(),
                        }
                    })?;
                    println!("{json}");
                }
                CliFormat::Human => {
                    println!(
                        "Context Bundle: {} item(s) across {} file(s) (approx tokens: {}, budget used: {}/{})",
                        bundle.total_items,
                        bundle.files.len(),
                        bundle.total_approx_tokens,
                        bundle.budget.budget_used,
                        bundle.budget.budget_limit
                    );
                    for file_bundle in &bundle.files {
                        println!(
                            "\n--- File: {} (tokens: {}) ---",
                            file_bundle.file, file_bundle.total_approx_tokens
                        );
                        for slice in &file_bundle.slices {
                            println!(
                                "  [{}] {} (lines {}-{}, score: {:.2}, reason: {})",
                                slice.role.as_str(),
                                slice.symbol,
                                slice.start_line,
                                slice.end_line,
                                slice.score,
                                slice.reason
                            );
                            if !slice.content.is_empty() {
                                for line in slice.content.lines() {
                                    println!("    | {line}");
                                }
                            }
                        }
                    }
                }
            }
            Ok(())
        }
        Commands::Mcp { repo, auto_index } => {
            let mut server = crate::mcp::McpServer::new_with_auto_index(repo, auto_index);
            let stdin = std::io::stdin().lock();
            let stdout = std::io::stdout().lock();
            server
                .run_stream(stdin, stdout)
                .map_err(|e| crate::error::GraphiaError::Storage {
                    message: format!("MCP server error: {e}"),
                })?;
            Ok(())
        }
        Commands::Daemon {
            action,
            repo,
            debounce_ms,
        } => {
            if let Some(DaemonAction::Status {
                repo: action_repo,
                format,
            }) = action
            {
                let target_repo = action_repo.or(repo).unwrap_or_else(|| {
                    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                });
                return print_daemon_status(&target_repo, format);
            }

            let repo_root = repo
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            let debounce = debounce_ms
                .map(std::time::Duration::from_millis)
                .unwrap_or_else(|| std::time::Duration::from_millis(100));

            let config = crate::daemon::DaemonConfig {
                repo_root: repo_root.clone(),
                debounce_duration: debounce,
                queue_capacity: 1000,
                persistence_interval: std::time::Duration::from_secs(5),
            };

            let mut server = crate::daemon::DaemonServer::new(config)?;
            println!("Starting live daemon for {}...", repo_root.display());
            server.run()?;
            Ok(())
        }
        Commands::DaemonStatus { repo, format } => {
            let target_repo = repo
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            print_daemon_status(&target_repo, format)
        }
        Commands::Flow {
            repo,
            source,
            sink,
            limit,
            format,
        } => {
            let target_repo = repo
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            let graph = load_or_build(&target_repo)?;
            let report =
                crate::analysis::advanced::find_source_sink_flows(&graph, &source, &sink, limit);
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
                        "Potential data-flow paths from '{}' to '{}': {} path(s) found",
                        report.source_query, report.sink_query, report.paths_found
                    );
                    for (i, p) in report.paths.iter().enumerate() {
                        println!(
                            "  Path #{}: length {} (confidence: {:?})",
                            i + 1,
                            p.length,
                            p.overall_confidence
                        );
                        for step in &p.steps {
                            println!(
                                "    [{}] {} (type: {}, conf: {:?})",
                                step.step_index,
                                step.node.qualified_name,
                                step.edge_type,
                                step.confidence
                            );
                        }
                    }
                }
            }
            Ok(())
        }
        Commands::ArchitectureCheck {
            repo,
            config,
            format,
        } => {
            let target_repo = repo
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            let graph = load_or_build(&target_repo)?;
            let arch_config = if let Some(cfg_path) = config {
                let content = std::fs::read_to_string(&cfg_path).map_err(|e| {
                    crate::error::GraphiaError::Storage {
                        message: e.to_string(),
                    }
                })?;
                serde_json::from_str(&content).map_err(|e| crate::error::GraphiaError::Storage {
                    message: e.to_string(),
                })?
            } else {
                crate::analysis::advanced::ArchitectureRulesConfig::default()
            };
            let report =
                crate::analysis::advanced::check_architecture_boundaries(&graph, &arch_config);
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
                        "Architecture Boundary Check: {}",
                        if report.passed { "PASSED" } else { "FAILED" }
                    );
                    println!(
                        "  Total edges evaluated: {}, Violations: {}",
                        report.total_edges_evaluated, report.violations_count
                    );
                    for v in &report.violations {
                        println!(
                            "    - [{}] {} -> {} : {}",
                            v.edge_kind.as_str(),
                            v.from_file,
                            v.to_file,
                            v.reason
                        );
                    }
                }
            }
            Ok(())
        }
        Commands::History {
            repo,
            max_commits,
            format,
        } => {
            let target_repo = repo
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            let history = crate::analysis::advanced::analyze_git_history(&target_repo, max_commits);
            let summary = match history {
                crate::analysis::advanced::GitHistoryResult::Success(summary) => summary,
                status => {
                    return Err(crate::error::GraphiaError::Io {
                        path: target_repo,
                        message: format!("git history unavailable: {status:?}"),
                    });
                }
            };
            match format {
                CliFormat::Json => {
                    let json = serde_json::to_string_pretty(&summary).map_err(|e| {
                        crate::error::GraphiaError::Storage {
                            message: e.to_string(),
                        }
                    })?;
                    println!("{json}");
                }
                CliFormat::Human => {
                    println!(
                        "Git History Intelligence: {} total commits",
                        summary.total_commits
                    );
                    println!("  Top Churned Files:");
                    for (i, f) in summary.files.iter().take(10).enumerate() {
                        println!(
                            "    {}. {} (commits: {}, authors: {})",
                            i + 1,
                            f.file,
                            f.commit_count,
                            f.authors.join(", ")
                        );
                    }
                }
            }
            Ok(())
        }
        Commands::Cochange {
            repo,
            min_support,
            format,
        } => {
            let target_repo = repo
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            let history =
                match crate::analysis::advanced::analyze_git_history(&target_repo, Some(500)) {
                    crate::analysis::advanced::GitHistoryResult::Success(history) => history,
                    status => {
                        return Err(crate::error::GraphiaError::Io {
                            path: target_repo,
                            message: format!("git history unavailable: {status:?}"),
                        });
                    }
                };
            let report =
                crate::analysis::advanced::compute_change_coupling(&history.commits, min_support);
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
                        "Temporal Co-Change Coupling ({} commits analyzed):",
                        report.total_commits_analyzed
                    );
                    for (i, pair) in report.pairs.iter().take(10).enumerate() {
                        println!(
                            "    {}. {} <-> {} (co-commits: {}, support: {:.2}, conf A->B: {:.2}, conf B->A: {:.2})",
                            i + 1,
                            pair.file_a,
                            pair.file_b,
                            pair.co_commits,
                            pair.support,
                            pair.confidence_a_to_b,
                            pair.confidence_b_to_a
                        );
                    }
                }
            }
            Ok(())
        }
        Commands::Deadcode { repo, format } => {
            let target_repo = repo
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            let graph = load_or_build(&target_repo)?;
            let report = crate::analysis::advanced::detect_dead_code_candidates(&graph);
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
                        "Structural Dead Code Candidates ({} found):",
                        report.candidates_count
                    );
                    for (i, c) in report.candidates.iter().enumerate() {
                        println!(
                            "    {}. [{}] {} ({}:{}) - {}",
                            i + 1,
                            c.node.kind.as_str(),
                            c.node.qualified_name,
                            c.node.location.file,
                            c.node.location.start_line,
                            c.reason
                        );
                    }
                }
            }
            Ok(())
        }
        Commands::Diff {
            old_index,
            new_index,
            format,
        } => {
            let old_graph = load_or_build(&old_index)?;
            let new_graph = load_or_build(&new_index)?;
            let diff = crate::analysis::advanced::diff_graphs(&old_graph, &new_graph);
            match format {
                CliFormat::Json => {
                    let json = serde_json::to_string_pretty(&diff).map_err(|e| {
                        crate::error::GraphiaError::Storage {
                            message: e.to_string(),
                        }
                    })?;
                    println!("{json}");
                }
                CliFormat::Human => {
                    println!(
                        "Graph Diff Summary: +{} / -{} nodes, ~{} modified nodes",
                        diff.added_nodes.len(),
                        diff.removed_nodes.len(),
                        diff.modified_nodes.len()
                    );
                    for n in &diff.added_nodes {
                        println!("  + [{}] {}", n.kind.as_str(), n.qualified_name);
                    }
                    for n in &diff.removed_nodes {
                        println!("  - [{}] {}", n.kind.as_str(), n.qualified_name);
                    }
                    for m in &diff.modified_nodes {
                        println!(
                            "  ~ {} ({}:{} -> {}:{})",
                            m.qualified_name, m.old_file, m.old_line, m.new_file, m.new_line
                        );
                    }
                }
            }
            Ok(())
        }
        Commands::ApiDiff {
            old_index,
            new_index,
            format,
        } => {
            let old_graph = load_or_build(&old_index)?;
            let new_graph = load_or_build(&new_index)?;
            let diff = crate::analysis::advanced::diff_public_api(&old_graph, &new_graph);
            match format {
                CliFormat::Json => {
                    let json = serde_json::to_string_pretty(&diff).map_err(|e| {
                        crate::error::GraphiaError::Storage {
                            message: e.to_string(),
                        }
                    })?;
                    println!("{json}");
                }
                CliFormat::Human => {
                    println!(
                        "Public API Diff Summary: +{} / -{} public symbols",
                        diff.added_public_symbols.len(),
                        diff.removed_public_symbols.len()
                    );
                    for n in &diff.added_public_symbols {
                        println!("  + [{}] {}", n.kind.as_str(), n.qualified_name);
                    }
                    for n in &diff.removed_public_symbols {
                        println!("  - [{}] {}", n.kind.as_str(), n.qualified_name);
                    }
                }
            }
            Ok(())
        }
    }
}

fn print_daemon_status(repo: &std::path::Path, format: CliFormat) -> crate::error::Result<()> {
    let status_opt = crate::daemon::DaemonServer::read_daemon_status(repo)?;
    match format {
        CliFormat::Json => {
            let json = serde_json::to_string_pretty(&status_opt).map_err(|e| {
                crate::error::GraphiaError::Storage {
                    message: e.to_string(),
                }
            })?;
            println!("{json}");
        }
        CliFormat::Human => match status_opt {
            Some(status) => {
                println!("Graphia Daemon Status:");
                println!("  Running: {}", status.running);
                println!("  PID: {}", status.pid);
                println!("  Repository: {}", status.repo_root.display());
                println!("  Graph Generation: {}", status.generation.0);
                println!(
                    "  Graph Nodes: {}, Edges: {}",
                    status.node_count, status.edge_count
                );
                println!("  Pending Events: {}", status.pending_events);
                println!("  State Dirty: {}", status.dirty);
                println!("  Last Update Timestamp (ms): {}", status.last_update_ms);
            }
            None => {
                println!("No active daemon found for repository: {}", repo.display());
            }
        },
    }
    Ok(())
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
