use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileChurn {
    pub file: String,
    pub commit_count: usize,
    pub additions: usize,
    pub deletions: usize,
    pub authors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitCommitRecord {
    pub commit_hash: String,
    pub author: String,
    pub timestamp: u64,
    pub files_changed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitHistorySummary {
    pub total_commits: usize,
    pub files: Vec<FileChurn>,
    pub commits: Vec<GitCommitRecord>,
}

#[must_use]
pub fn analyze_git_history(repo_root: &Path, max_commits: Option<usize>) -> GitHistorySummary {
    let limit_arg = format!("-n{}", max_commits.unwrap_or(100));
    let output = Command::new("git")
        .args([
            "log",
            &limit_arg,
            "--name-only",
            "--pretty=format:COMMIT:%H|%an|%at",
        ])
        .current_dir(repo_root)
        .output();

    let mut commits = Vec::new();
    let mut file_map: HashMap<String, (usize, HashSet<String>)> = HashMap::new();

    if let Ok(out) = output {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let mut current_commit: Option<GitCommitRecord> = None;

            for line in stdout.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Some(rest) = trimmed.strip_prefix("COMMIT:") {
                    if let Some(c) = current_commit.take() {
                        commits.push(c);
                    }
                    let parts: Vec<&str> = rest.split('|').collect();
                    if parts.len() >= 3 {
                        let hash = parts[0].to_string();
                        let author = parts[1].to_string();
                        let ts = parts[2].parse::<u64>().unwrap_or(0);
                        current_commit = Some(GitCommitRecord {
                            commit_hash: hash,
                            author,
                            timestamp: ts,
                            files_changed: Vec::new(),
                        });
                    }
                } else if let Some(ref mut c) = current_commit {
                    let file_norm = trimmed.replace('\\', "/");
                    c.files_changed.push(file_norm.clone());
                    let entry = file_map.entry(file_norm).or_insert((0, HashSet::new()));
                    entry.0 += 1;
                    entry.1.insert(c.author.clone());
                }
            }
            if let Some(c) = current_commit {
                commits.push(c);
            }
        }
    }

    let mut files: Vec<FileChurn> = file_map
        .into_iter()
        .map(|(file, (count, authors))| {
            let mut auth_vec: Vec<String> = authors.into_iter().collect();
            auth_vec.sort();
            FileChurn {
                file,
                commit_count: count,
                additions: 0,
                deletions: 0,
                authors: auth_vec,
            }
        })
        .collect();

    files.sort_by(|a, b| {
        b.commit_count
            .cmp(&a.commit_count)
            .then_with(|| a.file.cmp(&b.file))
    });

    let total_commits = commits.len();

    GitHistorySummary {
        total_commits,
        files,
        commits,
    }
}

use std::collections::HashSet;
