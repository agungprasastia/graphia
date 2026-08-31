#[path = "generator.rs"]
mod generator;
#[path = "rss.rs"]
mod rss;

use std::fs;
use std::hint::black_box;
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use generator::{Scale, generate};
use graphia::context::{BudgetValueType, ContextRequest, generate_context};
use graphia::daemon::debounce::SemanticAction;
use graphia::graph::Graph;
use graphia::incremental::IncrementalWorkspace;
use graphia::intelligence::{NeighborhoodOptions, analyze_impact, get_neighborhood};
use graphia::mcp::call_tool;
use graphia::query::{QueryIndex, TraversalLimits};
use graphia::storage::{build_graph_from_repo, build_or_update, load_graph_binary};

fn main() {
    if std::env::var("GRAPHIA_RUN_BENCH").as_deref() != Ok("1") {
        return;
    }
    println!(
        "dataset,files,source_bytes,nodes,edges,stage,latency_ns,peak_rss_bytes,files_reparsed,files_affected,generation_delta,persisted_generation,persistence_latency_ns"
    );
    let args = std::env::args().collect::<Vec<_>>();
    if let Some(stage) = args
        .iter()
        .position(|arg| arg == "--stage")
        .and_then(|i| args.get(i + 1))
    {
        let scale = args
            .iter()
            .position(|arg| arg == "--scale")
            .and_then(|i| args.get(i + 1))
            .map_or(Scale::Small, |s| match s.as_str() {
                "Medium" => Scale::Medium,
                "Large" => Scale::Large,
                _ => Scale::Small,
            });
        run_stage(scale, stage);
        return;
    }
    let mut scales = vec![Scale::Small, Scale::Medium];
    if std::env::var("GRAPHIA_BENCH_LARGE").as_deref() == Ok("1") {
        scales.push(Scale::Large);
    }
    let stages = [
        "clean_build",
        "native_index_load",
        "incremental_single_file",
        "incremental_burst_10",
        "incremental_burst_100",
        "daemon_idle",
        "daemon_idle_rss",
        "daemon_action_to_generation_1",
        "daemon_action_to_generation_10",
        "daemon_action_to_generation_100",
        "daemon_update_peak_rss_1",
        "context_generation",
    ];
    if std::env::var("GRAPHIA_BENCH_IN_PROCESS").as_deref() == Ok("1") {
        for scale in scales {
            run(scale);
        }
        return;
    }
    for scale in scales {
        for stage in stages {
            let scale_name = format!("{scale:?}");
            let Ok(output) = std::env::current_exe().and_then(|exe| {
                Command::new(exe)
                    .args(["--stage", stage, "--scale", &scale_name])
                    .output()
            }) else {
                continue;
            };
            if output.status.success() {
                print!(
                    "{}",
                    String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .skip(1)
                        .map(|line| format!("{line}\n"))
                        .collect::<String>()
                );
            }
        }
    }
}

fn run_stage(scale: Scale, stage: &str) {
    let dataset = generate(scale);
    let root = dataset.root.path();
    let metadata = &dataset.metadata;
    let target = root.join("src/rust/module_0000.rs");
    match stage {
        "clean_build" => {
            let result = timed(|| build_graph_from_repo(root).expect("clean build"));
            emit(scale, metadata, stage, result.0, rss::measure());
        }
        "native_index_load" => {
            build_or_update(root, true).expect("index");
            let result =
                timed(|| load_graph_binary(&root.join(".graphia/index.bin")).expect("load"));
            emit(scale, metadata, stage, result.0, rss::measure());
        }
        "incremental_single_file" => {
            let mut ws = IncrementalWorkspace::new(root.to_path_buf()).expect("workspace");
            let original = fs::read_to_string(&target).expect("target");
            fs::write(
                &target,
                format!("{original}\npub fn changed() -> usize {{ 1 }}\n"),
            )
            .expect("edit");
            let result = timed(|| {
                ws.apply_changes_selective(&[SemanticAction::Modified(target.clone())])
                    .expect("update")
            });
            emit(scale, metadata, stage, result.0, rss::measure());
        }
        "incremental_burst_10" | "incremental_burst_100" => {
            let mut ws = IncrementalWorkspace::new(root.to_path_buf()).expect("workspace");
            let count = if stage.ends_with("10") { 10 } else { 100 };
            let paths = source_paths(root, count);
            for path in &paths {
                fs::OpenOptions::new()
                    .append(true)
                    .open(path)
                    .expect("open")
                    .write_all(b"\n")
                    .expect("edit");
            }
            let actions = paths
                .into_iter()
                .map(SemanticAction::Modified)
                .collect::<Vec<_>>();
            let result = timed(|| ws.apply_changes_selective(&actions).expect("burst"));
            emit(scale, metadata, stage, result.0, rss::measure());
        }
        "daemon_idle" | "daemon_idle_rss" => {
            let (child, _status) = start_daemon(root);
            let _ = wait_healthy(&_status);
            let measurement = rss::measure_process(child.id());
            emit(scale, metadata, "daemon_idle_rss", 0, measurement);
            stop_daemon(child);
        }
        "daemon_action_to_generation_1"
        | "daemon_action_to_generation_10"
        | "daemon_action_to_generation_100"
        | "daemon_update_peak_rss_1" => {
            let count = stage.rsplit('_').next().unwrap().parse::<usize>().unwrap();
            let (child, status_path) = start_daemon(root);
            let before = wait_healthy(&status_path).generation;
            let paths = source_paths(root, count);
            let start = Instant::now();
            for path in &paths {
                fs::OpenOptions::new()
                    .append(true)
                    .open(path)
                    .expect("daemon benchmark edit")
                    .write_all(b"\n")
                    .expect("daemon benchmark write");
            }
            let after = wait_generation(&status_path, before);
            let persist_start = Instant::now();
            let persisted = wait_persisted(&status_path, after.generation);
            let persistence_latency = persist_start.elapsed().as_nanos();
            let measurement = rss::measure_process(child.id());
            println!(
                "{scale:?},{},{},{},{},{stage},{},{},files_reparsed={},files_affected={},generation_delta={},persisted_generation={},persistence_latency_ns={}",
                metadata.files,
                metadata.source_bytes,
                metadata.symbols,
                metadata.edges,
                start.elapsed().as_nanos(),
                measurement
                    .peak_rss_bytes
                    .map_or_else(|| "UNAVAILABLE".into(), |v| v.to_string()),
                after.files_reparsed,
                after.affected_files,
                after.generation.0.saturating_sub(before.0),
                persisted.last_persisted_generation.0,
                persistence_latency,
            );
            stop_daemon(child);
        }
        "context_generation" => {
            let graph = build_graph_from_repo(root).expect("graph");
            let symbol = graph
                .nodes
                .first()
                .map_or("function_0", |node| node.name.as_str());
            let result = timed(|| {
                generate_context(
                    &graph,
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
            emit(scale, metadata, stage, result.0, rss::measure());
        }
        _ => eprintln!("unknown benchmark stage: {stage}"),
    }
}

fn daemon_binary() -> std::path::PathBuf {
    let current = std::env::current_exe().expect("benchmark executable path");
    let name = if cfg!(windows) {
        "graphia.exe"
    } else {
        "graphia"
    };
    current
        .parent()
        .and_then(|path| path.parent())
        .map_or_else(|| std::path::PathBuf::from(name), |path| path.join(name))
}

fn start_daemon(root: &std::path::Path) -> (Child, std::path::PathBuf) {
    fs::create_dir_all(root.join(".graphia")).expect("create daemon state directory");
    let status = root.join(".graphia/daemon.json");
    let _ = fs::remove_file(&status);
    let stderr = std::fs::File::create(root.join(".graphia/daemon.stderr"))
        .expect("create daemon stderr log");
    let stdout = std::fs::File::create(root.join(".graphia/daemon.stdout"))
        .expect("create daemon stdout log");
    let child = Command::new(daemon_binary())
        .args(["daemon", "--repo"])
        .arg(root)
        .args(["--debounce-ms", "10"])
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .expect("spawn graphia daemon");
    (child, status)
}

fn wait_healthy(status: &std::path::Path) -> graphia::daemon::DaemonStatusInfo {
    for _ in 0..4800 {
        if let Ok(Some(value)) = graphia::daemon::DaemonServer::read_daemon_status(
            status
                .parent()
                .and_then(std::path::Path::parent)
                .unwrap_or_else(|| std::path::Path::new(".")),
        ) && matches!(value.health, graphia::daemon::DaemonHealth::Healthy)
        {
            return value;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!(
        "daemon did not become healthy: {} stderr={} stdout={}",
        status.display(),
        fs::read_to_string(status.with_file_name("daemon.stderr")).unwrap_or_default(),
        fs::read_to_string(status.with_file_name("daemon.stdout")).unwrap_or_default()
    )
}

fn wait_generation(
    status: &std::path::Path,
    before: graphia::daemon::GraphGeneration,
) -> graphia::daemon::DaemonStatusInfo {
    for _ in 0..400 {
        let value = wait_healthy(status);
        if value.generation > before {
            return value;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("daemon generation did not advance")
}

fn wait_persisted(
    status: &std::path::Path,
    generation: graphia::daemon::GraphGeneration,
) -> graphia::daemon::DaemonStatusInfo {
    for _ in 0..400 {
        let value = wait_healthy(status);
        if value.last_persisted_generation >= generation {
            return value;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("daemon persistence did not complete")
}

fn stop_daemon(mut child: Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status();
        let _ = child.wait();
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
        let _ = child.wait();
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
    let languages = [
        ("rust", "rs"),
        ("python", "py"),
        ("typescript", "ts"),
        ("go", "go"),
    ];
    (0..count)
        .map(|i| {
            let (language, extension) = languages[i % languages.len()];
            root.join(format!("src/{language}/module_{i:04}.{extension}"))
        })
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
        "{scale:?},{},{},{},{},{stage},{latency},{},,,,",
        metadata.files,
        metadata.source_bytes,
        metadata.symbols,
        metadata.edges,
        measurement
            .peak_rss_bytes
            .map_or_else(|| "UNAVAILABLE".into(), |value| value.to_string()),
    );
}
