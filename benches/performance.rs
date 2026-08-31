#[path = "generator.rs"]
mod generator;
#[path = "rss.rs"]
mod rss;

use std::fs;
use std::hint::black_box;
use std::io::Write;
use std::time::Instant;

use generator::{Scale, generate};
use graphia::context::{BudgetValueType, ContextRequest, generate_context};
use graphia::graph::Graph;
use graphia::intelligence::{NeighborhoodOptions, analyze_impact, get_neighborhood};
use graphia::mcp::call_tool;
use graphia::query::{QueryIndex, TraversalLimits};
use graphia::storage::{build_graph_from_repo, build_or_update, load_graph_binary};

fn main() {
    println!("dataset,files,source_bytes,nodes,edges,stage,latency_ns,peak_rss_bytes");
    for scale in [Scale::Small, Scale::Medium] {
        run(scale);
    }
    if std::env::var("GRAPHIA_BENCH_LARGE").as_deref() == Ok("1") {
        run(Scale::Large);
    }
}

fn run(scale: Scale) {
    let dataset = generate(scale);
    let root = dataset.root.path();
    let metadata = &dataset.metadata;
    let binary = root.join(".graphia/index.bin");
    let clean = timed(|| build_graph_from_repo(root).expect("clean build"));
    emit(scale, metadata, "clean_build", clean.0, rss::measure());
    build_or_update(root, true).expect("write native index");
    let load = timed(|| load_graph_binary(&binary).expect("native load"));
    emit(scale, metadata, "native_index_load", load.0, rss::measure());
    let graph = load.1;
    emit_query_stages(scale, metadata, root, &graph);
    let target = root.join("src/rust/module_0000.rs");
    let original = fs::read_to_string(&target).expect("read edit target");
    fs::write(
        &target,
        format!("{original}\npub fn changed() -> usize {{ 1 }}\n"),
    )
    .expect("edit target");
    let incremental = timed(|| build_or_update(root, false).expect("incremental update"));
    emit(
        scale,
        metadata,
        "incremental_single_file",
        incremental.0,
        rss::measure(),
    );
    fs::write(&target, original).expect("restore edit target");
    for count in [10, 100] {
        let paths = source_paths(root, count);
        for path in &paths {
            fs::OpenOptions::new()
                .append(true)
                .open(path)
                .expect("open burst file")
                .write_all(b"\n")
                .expect("write burst file");
        }
        let result = timed(|| build_or_update(root, false).expect("burst update"));
        emit(
            scale,
            metadata,
            &format!("incremental_burst_{count}"),
            result.0,
            rss::measure(),
        );
    }
    let fallback = timed(|| build_or_update(root, true).expect("fallback reconcile"));
    emit(
        scale,
        metadata,
        "fallback_reconcile",
        fallback.0,
        rss::measure(),
    );
}

fn emit_query_stages(
    scale: Scale,
    metadata: &generator::DatasetMetadata,
    root: &std::path::Path,
    graph: &Graph,
) {
    let query = QueryIndex::new(graph);
    let ids = graph.nodes.first().zip(graph.nodes.last());
    let bfs = timed(|| {
        ids.map(|(from, to)| {
            query.shortest_path(from.id, to.id, TraversalLimits::new(100, graph.nodes.len()))
        })
    });
    emit(scale, metadata, "bfs_path", bfs.0, rss::measure());
    let symbol = graph
        .nodes
        .first()
        .map_or("function_0", |node| node.name.as_str());
    let impact = timed(|| analyze_impact(graph, symbol, 3));
    emit(scale, metadata, "impact_analysis", impact.0, rss::measure());
    let neighborhood = timed(|| {
        get_neighborhood(
            graph,
            &NeighborhoodOptions {
                target: symbol.into(),
                depth: 2,
                limit: 20,
            },
        )
    });
    emit(
        scale,
        metadata,
        "neighborhood",
        neighborhood.0,
        rss::measure(),
    );
    let context = timed(|| {
        generate_context(
            graph,
            &ContextRequest {
                symbol: Some(symbol.into()),
                file: None,
                query: None,
                changed: false,
                budget: Some(2_000),
                budget_type: BudgetValueType::ApproxTokens,
                max_depth: 3,
                max_candidates: 50,
            },
            Some(root),
        )
    });
    emit(
        scale,
        metadata,
        "context_generation",
        context.0,
        rss::measure(),
    );
    let mcp_arguments = serde_json::json!({"query": symbol}).as_object().cloned();
    let mcp = timed(|| {
        call_tool(
            graph,
            Some(root),
            "graphia_search_symbol",
            mcp_arguments.as_ref(),
        )
    });
    emit(scale, metadata, "mcp_invocation", mcp.0, rss::measure());
}

fn source_paths(root: &std::path::Path, count: usize) -> Vec<std::path::PathBuf> {
    (0..count)
        .map(|i| root.join(format!("src/rust/module_{i:04}.rs")))
        .collect()
}
fn timed<T>(operation: impl FnOnce() -> T) -> (u128, T) {
    let start = Instant::now();
    let value = black_box(operation());
    (start.elapsed().as_nanos(), value)
}
fn emit(
    scale: Scale,
    metadata: &generator::DatasetMetadata,
    stage: &str,
    latency: u128,
    measurement: rss::RssMeasurement,
) {
    println!(
        "{scale:?},{},{},{},{},{stage},{latency},{}",
        metadata.files,
        metadata.source_bytes,
        metadata.symbols,
        metadata.edges,
        measurement
            .peak_rss_bytes
            .map_or_else(|| "UNAVAILABLE".into(), |value| value.to_string())
    );
}
