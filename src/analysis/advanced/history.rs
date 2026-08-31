use std::collections::{HashMap, HashSet};
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
    pub binary_files: usize,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GitHistoryResult {
    Success(GitHistorySummary),
    EmptyHistory,
    NotGitRepository,
    GitUnavailable,
    CommandFailed(String),
}

#[must_use]
pub fn analyze_git_history(repo_root: &Path, max_commits: Option<usize>) -> GitHistoryResult {
    let limit_arg = format!("-n{}", max_commits.unwrap_or(100));
    let output = Command::new("git")
        .args([
            "log",
            &limit_arg,
            "--numstat",
            "--pretty=format:COMMIT:%H|%an|%at",
        ])
        .current_dir(repo_root)
        .output();

    let mut commits = Vec::new();
    let mut file_map: HashMap<String, (usize, usize, usize, usize, HashSet<String>)> =
        HashMap::new();

    let out = match output {
        Ok(out) => out,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return GitHistoryResult::GitUnavailable;
        }
        Err(error) => return GitHistoryResult::CommandFailed(error.to_string()),
    };
    if !out.status.success() {
        let message = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return if message.contains("not a git repository") {
            GitHistoryResult::NotGitRepository
        } else if message.contains("does not have any commits")
            || message.contains("ambiguous argument 'HEAD'")
        {
            GitHistoryResult::EmptyHistory
        } else {
            GitHistoryResult::CommandFailed(message)
        };
    }
    {
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
                let numstat_parts: Vec<&str> = trimmed.split_whitespace().collect();
                if numstat_parts.len() >= 3 {
                    let is_binary = numstat_parts[0] == "-" || numstat_parts[1] == "-";
                    let adds = numstat_parts[0].parse::<usize>().unwrap_or(0);
                    let dels = numstat_parts[1].parse::<usize>().unwrap_or(0);
                    let file_path = numstat_parts[2..].join(" ");
                    let file_norm = file_path.replace('\\', "/");
                    c.files_changed.push(file_norm.clone());
                    let entry = file_map
                        .entry(file_norm)
                        .or_insert((0, 0, 0, 0, HashSet::new()));
                    entry.0 += 1;
                    entry.1 += adds;
                    entry.2 += dels;
                    entry.3 += usize::from(is_binary);
                    entry.4.insert(c.author.clone());
                } else {
                    let file_norm = trimmed.replace('\\', "/");
                    c.files_changed.push(file_norm.clone());
                    let entry = file_map
                        .entry(file_norm)
                        .or_insert((0, 0, 0, 0, HashSet::new()));
                    entry.0 += 1;
                    entry.4.insert(c.author.clone());
                }
            }
        }
        if let Some(c) = current_commit {
            commits.push(c);
        }
    }

    if commits.is_empty() {
        return GitHistoryResult::EmptyHistory;
    }

    let mut files: Vec<FileChurn> = file_map
        .into_iter()
        .map(|(file, (count, adds, dels, binary_files, authors))| {
            let mut auth_vec: Vec<String> = authors.into_iter().collect();
            auth_vec.sort();
            FileChurn {
                file,
                commit_count: count,
                additions: adds,
                deletions: dels,
                authors: auth_vec,
                binary_files,
            }
        })
        .collect();

    files.sort_by(|a, b| {
        b.commit_count
            .cmp(&a.commit_count)
            .then_with(|| a.file.cmp(&b.file))
    });

    let total_commits = commits.len();

    GitHistoryResult::Success(GitHistorySummary {
        total_commits,
        files,
        commits,
    })
}
