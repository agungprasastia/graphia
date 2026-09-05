use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::error::Result;

const CURSOR_GRAPHIA_RULE: &str = r#"---
description: Use Graphia for token-efficient code navigation, symbol relationships, impact analysis, test discovery, and bounded context gathering
globs:
alwaysApply: false
---

When Graphia is available, query its code index before broad repository search.
Start named-symbol work with `graphia explore <symbol> --depth 2 --format json`.
Use `graphia search . <query> --limit 10 --format json` when the exact symbol is unknown.
Use `graphia update .` after ordinary source edits; reserve `graphia build . --clean` for a corrupt or explicitly rebuilt index.
Keep depth, result limits, and context budgets small. Verify exact source before editing.
"#;

/// Result of running `graphia init`.
#[derive(Debug, Default)]
pub struct InitSummary {
    pub repo_root: PathBuf,
    pub gitignore_updated: bool,
    pub configured_targets: Vec<String>,
    pub configured_rules: Vec<String>,
    pub index_nodes: usize,
    pub index_edges: usize,
}

/// Run zero-config initialization for repository and AI coding agents.
///
/// # Errors
///
/// Returns an error if filesystem operations or initial graph build fail.
pub fn run_init(repo: Option<PathBuf>, yes: bool) -> Result<InitSummary> {
    let repo = repo.unwrap_or_else(|| PathBuf::from("."));
    confirm_initialization(&repo, yes)?;
    let mut summary = initialize_repository(Some(repo))?;
    configure_agents(&mut summary)?;
    Ok(summary)
}

pub(crate) fn confirm_initialization(repo: &Path, yes: bool) -> Result<()> {
    use std::io::{IsTerminal, Write};
    println!("Graphia initialization may create or update:");
    for relative in [".graphia/", ".gitignore"] {
        println!("  {}", repo.join(relative).display());
    }
    if repo.join(".claude").exists() || !repo.join(".cursor").exists() {
        println!("  {}", repo.join(".claude/mcp.json").display());
    }
    for (directory, paths) in [
        (
            ".cursor",
            &[".cursor/mcp.json", ".cursor/rules/graphia.mdc"][..],
        ),
        (".vscode", &[".vscode/mcp.json"][..]),
    ] {
        if repo.join(directory).exists() {
            for path in paths {
                println!("  {}", repo.join(path).display());
            }
        }
    }
    let opencode = if repo.join("opencode.jsonc").exists() {
        "opencode.jsonc"
    } else {
        "opencode.json"
    };
    if repo.join(".opencode").exists() || repo.join(opencode).exists() {
        println!(
            "  {} (JSONC comments will be removed when saving)",
            repo.join(opencode).display()
        );
    }
    if yes {
        return Ok(());
    }
    if !std::io::stdin().is_terminal() {
        return Err(crate::error::GraphiaError::InvalidArgument(
            "init requires confirmation; use --yes in non-interactive mode".into(),
        ));
    }
    print!("Apply these changes? [y/N] ");
    std::io::stdout()
        .flush()
        .map_err(|error| crate::error::GraphiaError::InvalidArgument(error.to_string()))?;
    let mut answer = String::new();
    let read = std::io::stdin()
        .read_line(&mut answer)
        .map_err(|error| crate::error::GraphiaError::InvalidArgument(error.to_string()))?;
    if !super::accepts_confirmation(read, &answer) {
        return Err(crate::error::GraphiaError::InvalidArgument(
            "initialization cancelled; no files changed".into(),
        ));
    }
    Ok(())
}

pub fn initialize_repository(repo: Option<PathBuf>) -> Result<InitSummary> {
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

    // 3. Initial index build
    let (graph, _) = crate::storage::build_or_update(&repo_root, false)?;

    summary.index_nodes = graph.node_count();
    summary.index_edges = graph.edge_count();

    Ok(summary)
}

pub fn configure_agents(summary: &mut InitSummary) -> Result<()> {
    let repo_root = summary.repo_root.clone();
    for relative in [
        ".claude/mcp.json",
        ".cursor/mcp.json",
        ".cursor/rules/graphia.mdc",
        ".vscode/mcp.json",
        "opencode.json",
        "opencode.jsonc",
    ] {
        super::skill::verify_containment(&repo_root, &repo_root.join(relative))?;
    }

    // Configure local project IDEs/agents
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
        if configure_cursor_rule(&cursor_dir)? {
            summary
                .configured_rules
                .push("Cursor (.cursor/rules/graphia.mdc)".to_string());
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

    // OpenCode (opencode.json uses a native MCP schema)
    let opencode_config = repo_root.join(if repo_root.join("opencode.jsonc").exists() {
        "opencode.jsonc"
    } else {
        "opencode.json"
    });
    if (repo_root.join(".opencode").exists() || opencode_config.exists())
        && configure_opencode_file(&opencode_config, &repo_root)?
    {
        summary.configured_targets.push(format!(
            "OpenCode ({})",
            opencode_config.file_name().unwrap().to_string_lossy()
        ));
    }

    Ok(())
}

fn update_gitignore(path: &Path) -> Result<bool> {
    let existing = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(crate::error::GraphiaError::Io {
                path: path.to_path_buf(),
                message: error.to_string(),
            });
        }
    };

    let mut additions = Vec::new();
    if !existing.lines().any(|line| {
        matches!(
            line.trim(),
            ".graphia" | ".graphia/" | "/.graphia" | "/.graphia/"
        )
    }) {
        additions.push(".graphia/");
    }
    if !existing
        .lines()
        .any(|line| matches!(line.trim(), "graph.json" | "/graph.json"))
    {
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

    crate::storage::atomic_write(path, new_content.as_bytes())?;

    Ok(true)
}

fn configure_mcp_file(config_path: &Path, repo_root: &Path) -> Result<bool> {
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| crate::error::GraphiaError::Io {
            path: parent.to_path_buf(),
            message: e.to_string(),
        })?;
    }

    let original = if config_path.exists() {
        Some(
            fs::read_to_string(config_path).map_err(|e| crate::error::GraphiaError::Io {
                path: config_path.to_path_buf(),
                message: e.to_string(),
            })?,
        )
    } else {
        None
    };
    let mut root: serde_json::Value = if let Some(content) = &original {
        serde_json::from_str(content).map_err(|e| crate::error::GraphiaError::Parse {
            file: config_path.display().to_string(),
            message: e.to_string(),
        })?
    } else {
        json!({})
    };

    if !root.is_object() {
        return Err(crate::error::GraphiaError::Parse {
            file: config_path.display().to_string(),
            message: "MCP configuration root must be a JSON object".into(),
        });
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
        return Err(crate::error::GraphiaError::Parse {
            file: config_path.display().to_string(),
            message: "mcpServers must be a JSON object".into(),
        });
    }

    let formatted =
        serde_json::to_string_pretty(&root).map_err(|e| crate::error::GraphiaError::Storage {
            message: e.to_string(),
        })?;

    if original.as_deref() == Some(&formatted) {
        return Ok(false);
    }

    crate::storage::atomic_write(config_path, formatted.as_bytes())?;

    Ok(true)
}

fn configure_cursor_rule(cursor_dir: &Path) -> Result<bool> {
    let rule_path = cursor_dir.join("rules/graphia.mdc");
    if rule_path.exists()
        && fs::read_to_string(&rule_path).map_err(|e| crate::error::GraphiaError::Io {
            path: rule_path.clone(),
            message: e.to_string(),
        })? == CURSOR_GRAPHIA_RULE
    {
        return Ok(false);
    }
    let parent = rule_path.parent().expect("Cursor rule has parent");
    fs::create_dir_all(parent).map_err(|e| crate::error::GraphiaError::Io {
        path: parent.to_path_buf(),
        message: e.to_string(),
    })?;
    crate::storage::atomic_write(&rule_path, CURSOR_GRAPHIA_RULE.as_bytes())?;
    Ok(true)
}

fn configure_opencode_file(config_path: &Path, repo_root: &Path) -> Result<bool> {
    let original = if config_path.exists() {
        Some(
            fs::read_to_string(config_path).map_err(|e| crate::error::GraphiaError::Io {
                path: config_path.to_path_buf(),
                message: e.to_string(),
            })?,
        )
    } else {
        None
    };
    let mut root: serde_json::Value = if let Some(content) = &original {
        parse_jsonc(content).map_err(|e| crate::error::GraphiaError::Parse {
            file: config_path.display().to_string(),
            message: e.to_string(),
        })?
    } else {
        json!({})
    };

    let obj = root
        .as_object_mut()
        .ok_or_else(|| crate::error::GraphiaError::Parse {
            file: config_path.display().to_string(),
            message: "OpenCode configuration root must be a JSON object".into(),
        })?;
    let servers = obj.entry("mcp").or_insert_with(|| json!({}));
    let servers = servers
        .as_object_mut()
        .ok_or_else(|| crate::error::GraphiaError::Parse {
            file: config_path.display().to_string(),
            message: "mcp must be a JSON object".into(),
        })?;
    servers.insert(
        "graphia".into(),
        json!({
            "type": "local",
            "command": ["graphia", "mcp", "--auto-index"],
            "cwd": repo_root.to_string_lossy(),
            "enabled": true
        }),
    );

    let formatted =
        serde_json::to_string_pretty(&root).map_err(|e| crate::error::GraphiaError::Storage {
            message: e.to_string(),
        })?;
    if original.as_deref() == Some(&formatted) {
        return Ok(false);
    }
    crate::storage::atomic_write(config_path, formatted.as_bytes())?;
    Ok(true)
}

fn parse_jsonc(content: &str) -> std::result::Result<serde_json::Value, String> {
    let mut bytes = content.as_bytes().to_vec();
    let mut i = 0;
    let mut quoted = false;
    while i < bytes.len() {
        if quoted {
            if bytes[i] == b'\\' {
                i += 2;
                continue;
            }
            if bytes[i] == b'"' {
                quoted = false;
            }
        } else if bytes[i] == b'"' {
            quoted = true;
        } else if bytes[i..].starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                bytes[i] = b' ';
                i += 1;
            }
            continue;
        } else if bytes[i..].starts_with(b"/*") {
            bytes[i] = b' ';
            bytes[i + 1] = b' ';
            i += 2;
            while i + 1 < bytes.len() && !bytes[i..].starts_with(b"*/") {
                if bytes[i] != b'\n' {
                    bytes[i] = b' ';
                }
                i += 1;
            }
            if i + 1 >= bytes.len() {
                return Err("unterminated JSONC comment".into());
            }
            bytes[i] = b' ';
            bytes[i + 1] = b' ';
            i += 2;
            continue;
        }
        i += 1;
    }
    i = 0;
    quoted = false;
    while i < bytes.len() {
        if quoted {
            if bytes[i] == b'\\' {
                i += 2;
                continue;
            }
            if bytes[i] == b'"' {
                quoted = false;
            }
        } else if bytes[i] == b'"' {
            quoted = true;
        } else if bytes[i] == b',' {
            let next = bytes[i + 1..]
                .iter()
                .find(|byte| !byte.is_ascii_whitespace());
            if matches!(next, Some(b'}' | b']')) {
                bytes[i] = b' ';
            }
        }
        i += 1;
    }
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        CURSOR_GRAPHIA_RULE, configure_cursor_rule, configure_mcp_file, configure_opencode_file,
    };
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn opencode_jsonc_handles_comments_strings_and_trailing_commas() {
        let content = r#"{
            // user configuration
            "url": "https://example.test/*literal*/",
            "items": ["comma,]",], /* comment */
        }"#;
        let value = super::parse_jsonc(content).unwrap();
        assert_eq!(value["url"], "https://example.test/*literal*/");
        assert_eq!(value["items"], serde_json::json!(["comma,]"]));
        assert!(super::parse_jsonc("{/* unclosed").is_err());
        assert!(super::parse_jsonc("{bad}").is_err());
        let dir = tempdir().unwrap();
        let config = dir.path().join("opencode.jsonc");
        fs::write(&config, content).unwrap();
        let mut summary = super::InitSummary {
            repo_root: dir.path().canonicalize().unwrap(),
            ..Default::default()
        };
        super::configure_agents(&mut summary).unwrap();
        assert!(!dir.path().join("opencode.json").exists());
        let result = super::parse_jsonc(&fs::read_to_string(config).unwrap()).unwrap();
        assert_eq!(result["url"], value["url"]);
        assert_eq!(result["mcp"]["graphia"]["type"], "local");
    }

    #[cfg(unix)]
    #[test]
    fn agent_configuration_rejects_external_directory_symlink() {
        let repo = tempdir().unwrap();
        let external = tempdir().unwrap();
        std::os::unix::fs::symlink(external.path(), repo.path().join(".cursor")).unwrap();
        let mut summary = super::InitSummary {
            repo_root: repo.path().canonicalize().unwrap(),
            ..Default::default()
        };
        assert!(super::configure_agents(&mut summary).is_err());
        assert!(!external.path().join("mcp.json").exists());
    }

    #[test]
    fn gitignore_preserves_unreadable_content() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".gitignore");
        let original = [0xff, 0xfe, 0x41];
        fs::write(&path, original).unwrap();
        assert!(super::update_gitignore(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn gitignore_comments_do_not_replace_ignore_rules() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".gitignore");
        fs::write(&path, "# .graphia and graph.json\n!graph.json\n").unwrap();
        assert!(super::update_gitignore(&path).unwrap());
        assert!(!super::update_gitignore(&path).unwrap());
        let content = fs::read_to_string(path).unwrap();
        assert!(content.lines().any(|line| line == ".graphia/"));
        assert!(content.lines().any(|line| line == "graph.json"));
    }

    #[test]
    fn malformed_mcp_config_is_not_overwritten() {
        let dir = tempdir().expect("tempdir");
        let config = dir.path().join("mcp.json");
        let original = r#"{"other": "setting"#;
        fs::write(&config, original).expect("write malformed config");

        assert!(configure_mcp_file(&config, dir.path()).is_err());
        assert_eq!(fs::read_to_string(config).expect("read config"), original);
    }

    #[test]
    fn non_object_mcp_config_is_not_overwritten() {
        let dir = tempdir().expect("tempdir");
        let config = dir.path().join("mcp.json");
        fs::write(&config, "[]").expect("write config");

        assert!(configure_mcp_file(&config, dir.path()).is_err());
        assert_eq!(fs::read_to_string(config).expect("read config"), "[]");
    }

    #[test]
    fn non_object_mcp_servers_is_not_overwritten() {
        let dir = tempdir().expect("tempdir");
        let config = dir.path().join("mcp.json");
        let original = r#"{"mcpServers": []}"#;
        fs::write(&config, original).expect("write config");

        assert!(configure_mcp_file(&config, dir.path()).is_err());
        assert_eq!(fs::read_to_string(config).expect("read config"), original);
    }

    #[test]
    fn cursor_rule_install_is_scoped_and_idempotent() {
        let dir = tempdir().expect("tempdir");
        let cursor = dir.path().join(".cursor");
        let rules = cursor.join("rules");
        fs::create_dir_all(&rules).expect("rules");
        let unrelated = rules.join("existing.mdc");
        fs::write(&unrelated, "keep me").expect("unrelated rule");

        assert!(configure_cursor_rule(&cursor).expect("initial install"));
        assert!(!configure_cursor_rule(&cursor).expect("idempotent install"));
        assert_eq!(fs::read_to_string(unrelated).expect("unrelated"), "keep me");
        assert_eq!(
            fs::read_to_string(rules.join("graphia.mdc")).expect("Graphia rule"),
            CURSOR_GRAPHIA_RULE
        );
    }

    #[test]
    fn opencode_config_is_native_scoped_and_idempotent() {
        let dir = tempdir().expect("tempdir");
        let config = dir.path().join("opencode.json");
        fs::write(&config, r#"{"theme":"system"}"#).expect("existing config");

        assert!(configure_opencode_file(&config, dir.path()).expect("initial config"));
        assert!(!configure_opencode_file(&config, dir.path()).expect("idempotent config"));

        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(config).expect("read config"))
                .expect("valid config");
        assert_eq!(value["theme"], "system");
        assert_eq!(value["mcp"]["graphia"]["type"], "local");
        assert_eq!(
            value["mcp"]["graphia"]["command"],
            serde_json::json!(["graphia", "mcp", "--auto-index"])
        );
        assert_eq!(value["mcp"]["graphia"]["enabled"], true);
    }
}
