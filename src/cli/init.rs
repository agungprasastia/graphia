use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::error::Result;

/// Result of running `graphia init`.
#[derive(Debug, Default)]
pub struct InitSummary {
    pub repo_root: PathBuf,
    pub gitignore_updated: bool,
    pub configured_targets: Vec<String>,
    pub index_nodes: usize,
    pub index_edges: usize,
}

/// Run zero-config initialization for repository and AI coding agents.
///
/// # Errors
///
/// Returns an error if filesystem operations or initial graph build fail.
pub fn run_init(repo: Option<PathBuf>, _yes: bool) -> Result<InitSummary> {
    let raw_repo =
        repo.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let repo_root = raw_repo.canonicalize().unwrap_or(raw_repo);

    let mut summary = InitSummary {
        repo_root: repo_root.clone(),
        ..Default::default()
    };

    // 1. Ensure .graphia directory exists
    let graphia_dir = repo_root.join(".graphia");
    fs::create_dir_all(&graphia_dir).map_err(|e| crate::error::GraphiaError::Io {
        path: graphia_dir.clone(),
        message: e.to_string(),
    })?;
    crate::storage::ensure_graphia_gitignore(&graphia_dir);

    // 2. Update .gitignore
    let gitignore_path = repo_root.join(".gitignore");
    if update_gitignore(&gitignore_path)? {
        summary.gitignore_updated = true;
    }

    // 3. Configure local project IDEs/agents
    // Claude Code (.claude/mcp.json)
    let claude_dir = repo_root.join(".claude");
    let claude_mcp = claude_dir.join("mcp.json");
    if (claude_dir.exists() || !repo_root.join(".cursor").exists())
        && configure_mcp_file(&claude_mcp, &repo_root)?
    {
        summary
            .configured_targets
            .push("Claude Code (.claude/mcp.json)".to_string());
    }

    // Cursor (.cursor/mcp.json)
    let cursor_dir = repo_root.join(".cursor");
    if cursor_dir.exists() {
        let cursor_mcp = cursor_dir.join("mcp.json");
        if configure_mcp_file(&cursor_mcp, &repo_root)? {
            summary
                .configured_targets
                .push("Cursor (.cursor/mcp.json)".to_string());
        }
    }

    // VS Code (.vscode/mcp.json)
    let vscode_dir = repo_root.join(".vscode");
    if vscode_dir.exists() {
        let vscode_mcp = vscode_dir.join("mcp.json");
        if configure_mcp_file(&vscode_mcp, &repo_root)? {
            summary
                .configured_targets
                .push("VS Code (.vscode/mcp.json)".to_string());
        }
    }

    // 4. Configure global Claude Desktop if installed
    if let Some(claude_desktop_path) = find_claude_desktop_config()
        && configure_mcp_file(&claude_desktop_path, &repo_root)?
    {
        summary
            .configured_targets
            .push("Claude Desktop".to_string());
    }

    // 5. Initial index build
    let (graph, _) = crate::storage::build_or_update(&repo_root, false)?;
    let bin_path = repo_root.join(".graphia/index.bin");
    crate::storage::save_graph_binary(&graph, &bin_path)?;
    let json_path = repo_root.join("graph.json");
    crate::storage::save_graph_json(&graph, &json_path)?;

    summary.index_nodes = graph.node_count();
    summary.index_edges = graph.edge_count();

    Ok(summary)
}

fn update_gitignore(path: &Path) -> Result<bool> {
    let existing = if path.exists() {
        fs::read_to_string(path).unwrap_or_default()
    } else {
        String::new()
    };

    let mut additions = Vec::new();
    if !existing.contains(".graphia") {
        additions.push(".graphia/");
    }
    if !existing.contains("graph.json") {
        additions.push("graph.json");
    }

    if additions.is_empty() {
        return Ok(false);
    }

    let mut new_content = existing;
    if !new_content.is_empty() && !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push_str("\n# Graphia index artifacts\n");
    for item in additions {
        new_content.push_str(item);
        new_content.push('\n');
    }

    fs::write(path, new_content).map_err(|e| crate::error::GraphiaError::Io {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;

    Ok(true)
}

fn configure_mcp_file(config_path: &Path, repo_root: &Path) -> Result<bool> {
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| crate::error::GraphiaError::Io {
            path: parent.to_path_buf(),
            message: e.to_string(),
        })?;
    }

    let mut root: serde_json::Value = if config_path.exists() {
        let content =
            fs::read_to_string(config_path).map_err(|e| crate::error::GraphiaError::Io {
                path: config_path.to_path_buf(),
                message: e.to_string(),
            })?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    if !root.is_object() {
        root = json!({});
    }

    let obj = root.as_object_mut().unwrap();
    let servers = obj
        .entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut();

    if let Some(servers_map) = servers {
        servers_map.insert(
            "graphia".to_string(),
            json!({
                "command": "graphia",
                "args": [
                    "mcp",
                    "--auto-index"
                ],
                "cwd": repo_root.to_string_lossy()
            }),
        );
    } else {
        return Ok(false);
    }

    let formatted =
        serde_json::to_string_pretty(&root).map_err(|e| crate::error::GraphiaError::Storage {
            message: e.to_string(),
        })?;

    fs::write(config_path, formatted).map_err(|e| crate::error::GraphiaError::Io {
        path: config_path.to_path_buf(),
        message: e.to_string(),
    })?;

    Ok(true)
}

fn find_claude_desktop_config() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|p| p.join("Claude").join("claude_desktop_config.json"))
            .filter(|p| p.parent().is_some_and(Path::exists))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|p| {
                p.join("Library")
                    .join("Application Support")
                    .join("Claude")
                    .join("claude_desktop_config.json")
            })
            .filter(|p| p.parent().is_some_and(Path::exists))
    }
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|p| {
                p.join(".config")
                    .join("Claude")
                    .join("claude_desktop_config.json")
            })
            .filter(|p| p.parent().is_some_and(Path::exists))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        None
    }
}
