use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::history::GitCommitRecord;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CoChangePair {
    pub file_a: String,
    pub file_b: String,
    pub co_commits: usize,
    pub commits_a: usize,
    pub commits_b: usize,
    pub support: f64,
    pub confidence_a_to_b: f64,
    pub confidence_b_to_a: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChangeCouplingReport {
    pub total_commits_analyzed: usize,
    pub pairs: Vec<CoChangePair>,
}

#[must_use]
pub fn compute_change_coupling(
    commits: &[GitCommitRecord],
    min_support: Option<f64>,
) -> ChangeCouplingReport {
    let mut file_commit_counts: HashMap<String, usize> = HashMap::new();
    let mut pair_counts: HashMap<(String, String), usize> = HashMap::new();

    let total_commits = commits.len();
    if total_commits == 0 {
        return ChangeCouplingReport {
            total_commits_analyzed: 0,
            pairs: Vec::new(),
        };
    }

    for commit in commits {
        let mut unique_files = commit.files_changed.clone();
        unique_files.sort();
        unique_files.dedup();

        for f in &unique_files {
            *file_commit_counts.entry(f.clone()).or_default() += 1;
        }

        for i in 0..unique_files.len() {
            for j in (i + 1)..unique_files.len() {
                let pair = (unique_files[i].clone(), unique_files[j].clone());
                *pair_counts.entry(pair).or_default() += 1;
            }
        }
    }

    let min_sup = min_support.unwrap_or(0.0);
    let mut pairs = Vec::new();

    for ((file_a, file_b), co_commits) in pair_counts {
        let commits_a = *file_commit_counts.get(&file_a).unwrap_or(&0);
        let commits_b = *file_commit_counts.get(&file_b).unwrap_or(&0);

        let support = co_commits as f64 / total_commits as f64;
        if support < min_sup {
            continue;
        }

        let confidence_a_to_b = if commits_a > 0 {
            co_commits as f64 / commits_a as f64
        } else {
            0.0
        };

        let confidence_b_to_a = if commits_b > 0 {
            co_commits as f64 / commits_b as f64
        } else {
            0.0
        };

        pairs.push(CoChangePair {
            file_a,
            file_b,
            co_commits,
            commits_a,
            commits_b,
            support,
            confidence_a_to_b,
            confidence_b_to_a,
        });
    }

    pairs.sort_by(|a, b| {
        b.co_commits
            .cmp(&a.co_commits)
            .then_with(|| a.file_a.cmp(&b.file_a))
    });

    ChangeCouplingReport {
        total_commits_analyzed: total_commits,
        pairs,
    }
}
