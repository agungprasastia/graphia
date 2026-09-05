use std::fs;

use graphia::cli::{Cli, CliFormat, Commands, run};
use graphia::graph::Graph;
use graphia::intelligence::explore::{explore_symbol, format_explore_markdown};
use graphia::model::{
    Confidence, Edge, EdgeIdentity, EdgeKind, Language, Node, NodeId, NodeIdentity, NodeKind,
    SourceLocation, Visibility,
};
use graphia::storage;
use tempfile::tempdir;

fn make_test_node(name: &str, file: &str, kind: NodeKind) -> Node {
    let loc = SourceLocation {
        file: file.to_string(),
        start_line: 1,
        start_col: 1,
        end_line: 4,
        end_col: 1,
    };
    let qualified_name = format!("{file}::{name}");
    let id = graphia::graph::stable_node_id(&NodeIdentity::new(
        Some(Language::Rust),
        file,
        kind,
        &qualified_name,
        None,
        None,
    ));
    Node {
        id,
        kind,
        name: name.to_string(),
        qualified_name,
        file: file.to_string(),
        location: loc,
        language: Some(Language::Rust),
        visibility: Visibility::Public,
        signature: None,
        container: None,
    }
}

fn make_test_edge(from: NodeId, to: NodeId, kind: EdgeKind) -> Edge {
    let id = graphia::graph::stable_edge_id(&EdgeIdentity::new(
        from,
        to,
        kind,
        Confidence::Extracted,
        None,
    ));
    Edge {
        id,
        kind,
        from,
        to,
        confidence: Confidence::Extracted,
        label: None,
    }
}

#[test]
fn test_explore_symbol_and_formatting() {
    let repo = tempdir().expect("tempdir");
    let src_dir = repo.path().join("src");
    fs::create_dir_all(&src_dir).expect("create src");

    let service_file = src_dir.join("service.rs");
    fs::write(
        &service_file,
        "pub fn process_payment() {\n    call_gateway();\n}\n",
    )
    .expect("write service");

    let n1 = make_test_node("controller", "src/api.rs", NodeKind::Function);
    let n2 = make_test_node("process_payment", "src/service.rs", NodeKind::Function);
    let n3 = make_test_node("call_gateway", "src/gateway.rs", NodeKind::Function);

    // controller -> process_payment -> call_gateway
    let e1 = make_test_edge(n1.id, n2.id, EdgeKind::Calls);
    let e2 = make_test_edge(n2.id, n3.id, EdgeKind::Calls);

    let graph = Graph::new(vec![n1, n2, n3], vec![e1, e2]);

    let res = explore_symbol(
        &graph,
        "src/service.rs::process_payment",
        2,
        Some(repo.path()),
    )
    .expect("explore result");

    assert_eq!(res.target.name, "process_payment");
    assert_eq!(res.callers.len(), 1);
    assert_eq!(res.callers[0].name, "controller");
    assert_eq!(res.callees.len(), 1);
    assert_eq!(res.callees[0].name, "call_gateway");
    assert!(res.source_code.is_some());
    assert!(
        res.source_code
            .as_ref()
            .unwrap()
            .contains("process_payment")
    );

    let markdown = format_explore_markdown(&res);
    assert!(markdown.contains("### [Function] src/service.rs::process_payment"));
    assert!(markdown.contains("Callers (1)"));
    assert!(markdown.contains("Callees (1)"));
    assert!(markdown.contains("Blast Radius"));
}

#[test]
fn test_cli_explore_command() {
    let repo = tempdir().expect("tempdir");
    let n = make_test_node("handle_request", "src/main.rs", NodeKind::Function);
    let graph = Graph::new(vec![n], vec![]);
    storage::save_graph_json(&graph, &repo.path().join("graph.json")).expect("save");

    // Human format
    run(Cli {
        command: Commands::Explore {
            repo: Some(repo.path().to_path_buf()),
            symbol: "src/main.rs::handle_request".to_string(),
            depth: 2,
            format: CliFormat::Human,
        },
    })
    .expect("cli explore human");

    // Json format
    run(Cli {
        command: Commands::Explore {
            repo: Some(repo.path().to_path_buf()),
            symbol: "src/main.rs::handle_request".to_string(),
            depth: 2,
            format: CliFormat::Json,
        },
    })
    .expect("cli explore json");
}

#[test]
fn test_cli_init_command() {
    let repo = tempdir().expect("tempdir");
    let src_dir = repo.path().join("src");
    fs::create_dir_all(&src_dir).expect("src dir");
    fs::write(
        src_dir.join("lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
    )
    .expect("lib.rs");

    // Pre-create a dummy .gitignore and .cursor folder
    fs::write(repo.path().join(".gitignore"), "target/\n").expect("gitignore");
    fs::create_dir_all(repo.path().join(".cursor")).expect(".cursor");
    fs::create_dir_all(repo.path().join(".opencode")).expect(".opencode");

    // First init
    run(Cli {
        command: Commands::Init {
            repo: Some(repo.path().to_path_buf()),
            yes: true,
            no_skill: false,
            skill_scope: Some(graphia::cli::CliSkillScope::Project),
        },
    })
    .expect("run init");

    assert!(repo.path().join(".graphia").exists());
    assert!(repo.path().join(".graphia/index.bin").exists());
    assert!(repo.path().join(".graphia/graph.json").exists());
    assert!(!repo.path().join("graph.json").exists());
    assert!(repo.path().join(".graphia/.gitignore").exists());
    let internal_gi = fs::read_to_string(repo.path().join(".graphia/.gitignore")).unwrap();
    assert!(internal_gi.contains("*\n!.gitignore"));

    // Verify .gitignore updated
    let gitignore = fs::read_to_string(repo.path().join(".gitignore")).expect("read gitignore");
    assert!(gitignore.contains(".graphia/"));
    assert!(gitignore.contains("graph.json"));

    // Verify Cursor mcp config created
    let cursor_mcp = repo.path().join(".cursor/mcp.json");
    assert!(cursor_mcp.exists());
    let cursor_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&cursor_mcp).unwrap()).unwrap();
    assert!(cursor_json["mcpServers"]["graphia"].is_object());
    let cursor_rule = repo.path().join(".cursor/rules/graphia.mdc");
    assert!(cursor_rule.exists());
    assert!(
        fs::read_to_string(cursor_rule)
            .expect("read Cursor rule")
            .contains("graphia explore")
    );

    // Verify OpenCode receives its native MCP shape.
    let opencode_config = repo.path().join("opencode.json");
    let opencode_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(opencode_config).unwrap()).unwrap();
    assert_eq!(opencode_json["mcp"]["graphia"]["type"], "local");
    assert_eq!(
        opencode_json["mcp"]["graphia"]["command"],
        serde_json::json!(["graphia", "mcp", "--auto-index"])
    );
    assert!(repo.path().join(".agents/skills/graphia/SKILL.md").exists());

    // Second init is idempotent
    run(Cli {
        command: Commands::Init {
            repo: Some(repo.path().to_path_buf()),
            yes: true,
            no_skill: false,
            skill_scope: Some(graphia::cli::CliSkillScope::Project),
        },
    })
    .expect("run init second time");
}

#[test]
fn test_cli_report_command() {
    let repo = tempdir().expect("tempdir");
    let n1 = make_test_node("core_engine", "src/core.rs", NodeKind::Function);
    let n2 = make_test_node("api_layer", "src/api.rs", NodeKind::Function);
    let e1 = make_test_edge(n2.id, n1.id, EdgeKind::Calls);
    let graph = Graph::new(vec![n1, n2], vec![e1]);
    storage::save_graph_json(&graph, &repo.path().join("graph.json")).expect("save");

    // Run report command (default output: GRAPH_REPORT.md)
    run(Cli {
        command: Commands::Report {
            repo: Some(repo.path().to_path_buf()),
            output: None,
            format: CliFormat::Human,
        },
    })
    .expect("cli report human");

    let report_file = repo.path().join("GRAPH_REPORT.md");
    assert!(report_file.exists());
    let report_content = fs::read_to_string(&report_file).expect("read report");

    assert!(report_content.contains("# Graphia Architectural Audit Report"));
    assert!(report_content.contains("1. Executive Summary"));
    assert!(report_content.contains("2. God Nodes & Critical Hotspots"));
    assert!(report_content.contains("6. AI Agent Guidelines & Safety Guardrails"));

    // Custom output path
    let custom_out = repo.path().join("CUSTOM_AUDIT.md");
    run(Cli {
        command: Commands::Report {
            repo: Some(repo.path().to_path_buf()),
            output: Some(custom_out.clone()),
            format: CliFormat::Human,
        },
    })
    .expect("cli report custom path");

    assert!(custom_out.exists());

    // JSON format
    run(Cli {
        command: Commands::Report {
            repo: Some(repo.path().to_path_buf()),
            output: None,
            format: CliFormat::Json,
        },
    })
    .expect("cli report json");
}
