use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;
use tempfile::tempdir;

fn run_binary_cmd(
    bin: &str,
    args: &[&str],
    current_dir: &std::path::Path,
) -> (bool, String, String) {
    let output = Command::new(bin)
        .args(args)
        .current_dir(current_dir)
        .env("GRAPHIA_INSTALL_HOME", current_dir.join(".test-home"))
        .output()
        .expect("execute graphia binary");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (output.status.success(), stdout, stderr)
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn all_mcp_tools_work_through_binary_stdio() {
    use serde_json::{Value, json};
    let repo = tempdir().unwrap();
    fs::write(repo.path().join("lib.rs"),
        "pub fn checkout() { calculate_total(); }\npub fn calculate_total() { apply_discount(); }\npub fn apply_discount() {}\n#[test]\nfn test_calculate_total() { calculate_total(); }\n").unwrap();
    let cases = [
        (
            "graphia_search_symbol",
            json!({"query":"calculate"}),
            "calculate_total",
        ),
        (
            "graphia_get_symbol",
            json!({"symbol":"calculate_total"}),
            "calculate_total",
        ),
        (
            "graphia_find_callers",
            json!({"symbol":"calculate_total"}),
            "checkout",
        ),
        (
            "graphia_find_callees",
            json!({"symbol":"calculate_total"}),
            "apply_discount",
        ),
        (
            "graphia_find_references",
            json!({"symbol":"calculate_total"}),
            "checkout",
        ),
        (
            "graphia_dependency_path",
            json!({"from":"checkout","to":"apply_discount"}),
            "calculate_total",
        ),
        (
            "graphia_neighborhood",
            json!({"symbol":"calculate_total"}),
            "apply_discount",
        ),
        (
            "graphia_impact",
            json!({"symbol":"apply_discount"}),
            "calculate_total",
        ),
        (
            "graphia_find_tests",
            json!({"symbol":"calculate_total"}),
            "test_calculate_total",
        ),
        ("graphia_architecture", json!({}), "total_nodes"),
        (
            "graphia_context",
            json!({"symbol":"calculate_total","token_budget":500}),
            "calculate_total",
        ),
        (
            "graphia_explore",
            json!({"query":"calculate_total"}),
            "apply_discount",
        ),
    ];
    let mut child = Command::new(env!("CARGO_BIN_EXE_graphia"))
        .args(["mcp", "--auto-index"])
        .current_dir(repo.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    use std::io::{BufRead, BufReader};
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut responses = Vec::<Value>::new();
    writeln!(input, "{}", json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2024-11-05"}})).unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    responses.push(serde_json::from_str(&line).unwrap());
    for (index, (name, arguments, _)) in cases.iter().enumerate() {
        writeln!(input, "{}", json!({"jsonrpc":"2.0","id":index+1,"method":"tools/call","params":{"name":name,"arguments":arguments}})).unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        responses.push(serde_json::from_str(&line).unwrap());
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(responses.len(), cases.len() + 1);
    for (index, (name, _, expected)) in cases.iter().enumerate() {
        let response = responses
            .iter()
            .find(|response| response["id"] == index + 1)
            .unwrap();
        assert!(response["error"].is_null(), "{name}: {response}");
        assert_ne!(response["result"]["isError"], true, "{name}: {response}");
        assert!(
            response["result"].to_string().contains(expected),
            "{name}: {response}"
        );
    }
}

fn http_get(port: u16, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).expect("connect to UI server");
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .expect("send HTTP request");

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("read HTTP response");

    let status_line = response.lines().next().unwrap_or("");
    let status_code: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let body = response.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status_code, body)
}

#[test]
fn test_comprehensive_e2e_workflow() {
    let bin = env!("CARGO_BIN_EXE_graphia");
    let fixture = tempdir().expect("tempdir");
    let root = fixture.path();

    // 1. Create multi-language project fixture
    // Rust file
    fs::write(
        root.join("service.rs"),
        "pub fn authenticate_user(token: &str) -> bool { validate_token(token) }\npub fn validate_token(_: &str) -> bool { true }\n",
    )
    .expect("write service.rs");

    // Python file
    fs::write(
        root.join("worker.py"),
        "def process_job(job_id):\n    return job_id * 2\n",
    )
    .expect("write worker.py");

    // TypeScript file
    fs::write(
        root.join("client.ts"),
        "export function fetchData(): Promise<void> {}\n",
    )
    .expect("write client.ts");

    // Pre-create agent dirs so init configures all detected targets.
    fs::create_dir_all(root.join(".claude")).expect("create .claude");
    fs::create_dir_all(root.join(".cursor")).expect("create .cursor");
    fs::create_dir_all(root.join(".vscode")).expect("create .vscode");
    fs::create_dir_all(root.join(".opencode")).expect("create .opencode");

    // 2. Test `graphia init --yes`
    let (success, stdout, stderr) = run_binary_cmd(bin, &["init", "--yes"], root);
    assert!(success, "init failed: {stdout} {stderr}");
    assert!(stdout.contains("Configured MCP for agents:"));
    assert!(stdout.contains("Configured agent rules:"));
    assert!(stdout.contains("Graphia agent skill: current (user scope, 5 target(s))"));
    assert!(root.join(".graphia").is_dir());
    assert!(root.join(".gitignore").exists());
    assert!(root.join(".claude/mcp.json").exists());
    assert!(root.join(".cursor/mcp.json").exists());
    assert!(root.join(".cursor/rules/graphia.mdc").exists());
    assert!(root.join(".vscode/mcp.json").exists());
    assert!(root.join("opencode.json").exists());
    assert!(root.join(".graphia/index.bin").exists());
    assert!(root.join(".graphia/.gitignore").exists());
    assert!(
        root.join(".test-home/.codex/skills/graphia/SKILL.md")
            .exists()
    );
    assert!(
        root.join(".test-home/.config/opencode/skills/graphia/SKILL.md")
            .exists()
    );
    let (success, stdout, stderr) = run_binary_cmd(bin, &["skill", "status"], root);
    assert!(success, "skill status failed: {stdout} {stderr}");
    assert!(stdout.contains("current (user scope, 5 target(s))"));

    let codex_skill = root.join(".test-home/.codex/skills/graphia/SKILL.md");
    fs::write(&codex_skill, "stale").expect("make skill stale");
    let (success, stdout, stderr) = run_binary_cmd(bin, &["init", "--yes"], root);
    assert!(success, "stale skill update failed: {stdout} {stderr}");
    assert!(stdout.contains("Graphia agent skill: current (user scope, 5 target(s))"));
    assert_eq!(
        fs::read_to_string(codex_skill).expect("updated skill"),
        include_str!("../skills/graphia/SKILL.md")
    );

    // 3. Test `graphia build`
    let (success, stdout, stderr) = run_binary_cmd(bin, &["build", "."], root);
    assert!(success, "build failed: {stdout} {stderr}");
    assert!(root.join(".graphia/graph.json").exists());
    assert!(!root.join("graph.json").exists());

    // 4. Test `graphia explore authenticate_user`
    let (success, stdout, stderr) = run_binary_cmd(bin, &["explore", "authenticate_user"], root);
    assert!(success, "explore failed: {stdout} {stderr}");
    assert!(stdout.contains("authenticate_user"));
    assert!(stdout.contains("Callees"));

    let (success, stdout, stderr) = run_binary_cmd(
        bin,
        &["explore", "authenticate_user", "--format", "json"],
        root,
    );
    assert!(success, "explore json failed: {stdout} {stderr}");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid explore JSON");
    assert_eq!(parsed["target"]["name"], "authenticate_user");

    // 5. Test `graphia report`
    let report_path = root.join("GRAPH_REPORT.md");
    let (success, stdout, stderr) = run_binary_cmd(
        bin,
        &["report", "--output", report_path.to_str().unwrap()],
        root,
    );
    assert!(success, "report failed: {stdout} {stderr}");
    assert!(report_path.exists());
    let report_content = fs::read_to_string(&report_path).expect("read report");
    assert!(report_content.contains("# Graphia Architectural Audit Report"));

    // 6. Test `graphia export` in all formats
    // DOT
    let dot_file = root.join("architecture.dot");
    let (success, stdout, stderr) = run_binary_cmd(
        bin,
        &[
            "export",
            ".",
            "--format",
            "dot",
            "--output",
            dot_file.to_str().unwrap(),
        ],
        root,
    );
    assert!(success, "export dot failed: {stdout} {stderr}");
    assert!(dot_file.exists());
    assert!(
        fs::read_to_string(&dot_file)
            .unwrap()
            .contains("digraph Graphia")
    );

    // Mermaid
    let mmd_file = root.join("graph.mmd");
    let (success, stdout, stderr) = run_binary_cmd(
        bin,
        &[
            "export",
            ".",
            "--format",
            "mermaid",
            "--output",
            mmd_file.to_str().unwrap(),
        ],
        root,
    );
    assert!(success, "export mermaid failed: {stdout} {stderr}");
    assert!(mmd_file.exists());
    assert!(
        fs::read_to_string(&mmd_file)
            .unwrap()
            .contains("flowchart TD")
    );

    // GraphML
    let graphml_file = root.join("graph.graphml");
    let (success, stdout, stderr) = run_binary_cmd(
        bin,
        &[
            "export",
            ".",
            "--format",
            "graphml",
            "--output",
            graphml_file.to_str().unwrap(),
        ],
        root,
    );
    assert!(success, "export graphml failed: {stdout} {stderr}");
    assert!(graphml_file.exists());
    assert!(
        fs::read_to_string(&graphml_file)
            .unwrap()
            .contains("<graphml")
    );

    // GEXF
    let gexf_file = root.join("graph.gexf");
    let (success, stdout, stderr) = run_binary_cmd(
        bin,
        &[
            "export",
            ".",
            "--format",
            "gexf",
            "--output",
            gexf_file.to_str().unwrap(),
        ],
        root,
    );
    assert!(success, "export gexf failed: {stdout} {stderr}");
    assert!(gexf_file.exists());
    assert!(fs::read_to_string(&gexf_file).unwrap().contains("<gexf"));

    // Cytoscape
    let cyto_file = root.join("cytoscape.json");
    let (success, stdout, stderr) = run_binary_cmd(
        bin,
        &[
            "export",
            ".",
            "--format",
            "cytoscape",
            "--output",
            cyto_file.to_str().unwrap(),
        ],
        root,
    );
    assert!(success, "export cytoscape failed: {stdout} {stderr}");
    assert!(cyto_file.exists());
    let cyto_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&cyto_file).unwrap()).unwrap();
    assert!(cyto_json["elements"]["nodes"].is_array());

    // Obsidian Vault
    let vault_dir = root.join("obsidian-vault");
    let (success, stdout, stderr) = run_binary_cmd(
        bin,
        &[
            "export",
            ".",
            "--format",
            "obsidian",
            "--output",
            vault_dir.to_str().unwrap(),
        ],
        root,
    );
    assert!(success, "export obsidian failed: {stdout} {stderr}");
    assert!(vault_dir.join(".obsidian/graph.json").exists());
    assert!(vault_dir.join("00-Index.md").exists());
    assert!(vault_dir.join("01-Files.md").exists());

    // 7. Test `graphia ui` live HTTP server
    let port = 49152 + (std::process::id() as u16 % 5000);
    let ui_cmd = Command::new(bin)
        .args([
            "ui",
            "--repo",
            root.to_str().unwrap(),
            "--port",
            &port.to_string(),
            "--no-open",
        ])
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn graphia ui");

    // Wait for server to bind
    let mut server_up = false;
    for _ in 0..50 {
        if TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
            server_up = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(server_up, "UI server failed to start within 2.5s");

    let _guard = ChildGuard(ui_cmd);

    // Test GET /
    let (status, html) = http_get(port, "/");
    assert_eq!(status, 200);
    assert!(html.contains("<!DOCTYPE html>"));
    assert!(html.contains("Graphia"));

    // Test GET /api/stats
    let (status, stats_body) = http_get(port, "/api/stats");
    assert_eq!(status, 200);
    let stats: serde_json::Value = serde_json::from_str(&stats_body).expect("valid stats json");
    assert!(stats["node_count"].as_u64().unwrap() >= 3);

    // Test GET /api/search?q=auth
    let (status, search_body) = http_get(port, "/api/search?q=auth");
    assert_eq!(status, 200);
    let search_results: serde_json::Value =
        serde_json::from_str(&search_body).expect("valid search json");
    assert!(!search_results.as_array().unwrap().is_empty());

    // Test GET /api/explore?symbol=authenticate_user
    let (status, explore_body) = http_get(port, "/api/explore?symbol=authenticate_user");
    assert_eq!(status, 200);
    let explore_json: serde_json::Value =
        serde_json::from_str(&explore_body).expect("valid explore json");
    assert_eq!(explore_json["target"]["name"], "authenticate_user");

    // Test GET /api/neighborhood?symbol=authenticate_user
    let (status, nh_body) = http_get(port, "/api/neighborhood?symbol=authenticate_user");
    assert_eq!(status, 200);
    let nh_json: serde_json::Value =
        serde_json::from_str(&nh_body).expect("valid neighborhood json");
    assert_eq!(nh_json["target"]["name"], "authenticate_user");
    assert!(nh_json["callees"].is_array());

    // Test GET /api/hotspots
    let (status, hotspots_body) = http_get(port, "/api/hotspots");
    assert_eq!(status, 200);
    assert!(!hotspots_body.is_empty());

    // Test GET /api/cycles
    let (status, cycles_body) = http_get(port, "/api/cycles");
    assert_eq!(status, 200);
    assert!(!cycles_body.is_empty());
}

#[test]
fn test_init_skill_skip_paths_are_non_interactive_and_home_independent() {
    let bin = env!("CARGO_BIN_EXE_graphia");

    let skipped = tempdir().expect("skip repo");
    fs::write(skipped.path().join("main.rs"), "fn main() {}\n").expect("source");
    let (success, stdout, stderr) =
        run_binary_cmd(bin, &["init", "--yes", "--no-skill"], skipped.path());
    assert!(success, "--no-skill init failed: {stdout} {stderr}");
    assert!(stdout.contains("Graphia agent skill: skipped (--no-skill)"));
    assert!(stdout.contains("Graphia initialization may create or update:"));
    assert!(stdout.contains(".gitignore"));
    assert!(
        stdout.find("may create or update").unwrap() < stdout.find("Initialized Graphia").unwrap()
    );
    assert!(!skipped.path().join(".test-home").exists());

    let explicit_user = tempdir().expect("explicit user repo");
    fs::write(explicit_user.path().join("main.rs"), "fn main() {}\n").expect("source");
    let (success, stdout, stderr) = run_binary_cmd(
        bin,
        &["init", "--skill-scope", "user"],
        explicit_user.path(),
    );
    assert!(!success, "unconfirmed init succeeded: {stdout} {stderr}");
    assert!(stderr.contains("use --yes"));
    assert!(!explicit_user.path().join(".graphia").exists());
    assert!(!explicit_user.path().join(".gitignore").exists());
    assert!(!explicit_user.path().join(".claude").exists());
    assert!(!explicit_user.path().join(".test-home").exists());

    let missing_home = tempdir().expect("missing home repo");
    fs::write(missing_home.path().join("main.rs"), "fn main() {}\n").expect("source");
    let output = Command::new(bin)
        .arg("init")
        .current_dir(missing_home.path())
        .env_remove("GRAPHIA_INSTALL_HOME")
        .env_remove("HOME")
        .env_remove("USERPROFILE")
        .stdin(Stdio::null())
        .output()
        .expect("run without home");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "unconfirmed missing-home init succeeded: {stdout} {stderr}"
    );
    assert!(stderr.contains("use --yes"));
    assert!(!missing_home.path().join(".graphia").exists());
}

#[test]
fn export_without_output_and_unconfirmed_init_do_not_write() {
    let bin = env!("CARGO_BIN_EXE_graphia");
    for args in [
        vec!["export", "."],
        vec!["init"],
        vec!["init", "--no-skill"],
        vec!["init", "--skill-scope", "project"],
    ] {
        let repo = tempdir().unwrap();
        fs::write(repo.path().join("main.rs"), "fn main() {}\n").unwrap();
        fs::write(repo.path().join(".gitignore"), "keep-me\n").unwrap();
        let (success, _, stderr) = run_binary_cmd(bin, &args, repo.path());
        assert!(!success, "unexpected success: {args:?}");
        assert!(
            stderr.contains("--output") || stderr.contains("--yes"),
            "{stderr}"
        );
        assert_eq!(
            fs::read_to_string(repo.path().join(".gitignore")).unwrap(),
            "keep-me\n"
        );
        assert_eq!(
            fs::read_dir(repo.path()).unwrap().count(),
            2,
            "unexpected write: {args:?}"
        );
    }
}
