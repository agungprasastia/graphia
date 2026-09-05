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
            "-z",
            "--pretty=format:COMMIT:%H|%an|%at%x00",
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
        let mut current_commit: Option<GitCommitRecord> = None;
        let fields: Vec<&[u8]> = out.stdout.split(|byte| *byte == 0).collect();
        let mut field_index = 0;

        while field_index < fields.len() {
            let field = String::from_utf8_lossy(fields[field_index]);
            field_index += 1;
            let record = field.trim_start_matches(['\r', '\n']);
            if record.is_empty() {
                continue;
            }
            if let Some(rest) = record.strip_prefix("COMMIT:") {
                if let Some(c) = current_commit.take() {
                    commits.push(c);
                }
                let parts: Vec<&str> = rest.split('|').collect();
                if parts.len() >= 3 {
                    current_commit = Some(GitCommitRecord {
                        commit_hash: parts[0].to_string(),
                        author: parts[1].to_string(),
                        timestamp: parts[2].parse::<u64>().unwrap_or(0),
                        files_changed: Vec::new(),
                    });
                }
            } else if let Some(ref mut commit) = current_commit
                && let Some((additions, deletions, mut file_path)) = parse_numstat_record(record)
            {
                let renamed_path;
                if file_path.is_empty() && field_index + 1 < fields.len() {
                    renamed_path = String::from_utf8_lossy(fields[field_index + 1]);
                    field_index += 2;
                    file_path = renamed_path.as_ref();
                }
                if file_path.is_empty() {
                    continue;
                }

                let is_binary = additions == "-" || deletions == "-";
                let adds = additions.parse::<usize>().unwrap_or(0);
                let dels = deletions.parse::<usize>().unwrap_or(0);
                let file_norm = file_path.replace('\\', "/");
                commit.files_changed.push(file_norm.clone());
                let entry = file_map
                    .entry(file_norm)
                    .or_insert((0, 0, 0, 0, HashSet::new()));
                entry.0 += 1;
                entry.1 += adds;
                entry.2 += dels;
                entry.3 += usize::from(is_binary);
                entry.4.insert(commit.author.clone());
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

fn parse_numstat_record(record: &str) -> Option<(&str, &str, &str)> {
    let mut parts = record.splitn(3, '\t');
    Some((parts.next()?, parts.next()?, parts.next()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn numstat_preserves_filename_whitespace() {
        let repo = tempdir().expect("repo");
        assert!(
            Command::new("git")
                .arg("init")
                .current_dir(repo.path())
                .status()
                .expect("git init")
                .success()
        );
        assert!(
            Command::new("git")
                .args(["config", "user.email", "test@example.com"])
                .current_dir(repo.path())
                .status()
                .expect("git config")
                .success()
        );
        assert!(
            Command::new("git")
                .args(["config", "user.name", "Test User"])
                .current_dir(repo.path())
                .status()
                .expect("git config")
                .success()
        );
        let filename = "two  spaces.rs";
        fs::write(repo.path().join(filename), "fn main() {}\n").expect("file");
        assert!(
            Command::new("git")
                .args(["add", "--", filename])
                .current_dir(repo.path())
                .status()
                .expect("git add")
                .success()
        );
        assert!(
            Command::new("git")
                .args(["commit", "-m", "initial"])
                .current_dir(repo.path())
                .status()
                .expect("git commit")
                .success()
        );

        let GitHistoryResult::Success(summary) = analyze_git_history(repo.path(), None) else {
            panic!("history should succeed");
        };
        assert_eq!(summary.commits[0].files_changed, [filename]);
        assert_eq!(summary.files[0].file, filename);
    }

    #[test]
    fn numstat_parser_preserves_tabs_inside_filename() {
        assert_eq!(
            parse_numstat_record("1\t2\tdir/name\twith-tab.rs"),
            Some(("1", "2", "dir/name\twith-tab.rs"))
        );
    }
}
