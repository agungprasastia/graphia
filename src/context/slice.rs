use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::SourceLocation;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSlice {
    pub file: String,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub content: String,
    pub approx_tokens: usize,
    pub bytes: usize,
    pub characters: usize,
}

#[must_use]
pub fn estimate_approx_tokens(text: &str) -> usize {
    // Standard ~4 chars / token heuristic, with a minimum of 1 token for non-empty string
    let chars = text.chars().count();
    if chars == 0 { 0 } else { chars.div_ceil(4) }
}

/// Extract source slice from file system or provided file text.
///
/// # Errors
///
/// Returns an error if the file cannot be read from disk.
pub fn extract_source_slice(
    repo_root: Option<&Path>,
    location: &SourceLocation,
) -> crate::error::Result<SourceSlice> {
    let file_path = if let Some(root) = repo_root {
        let mut relative = PathBuf::new();
        for component in Path::new(&location.file).components() {
            match component {
                Component::Normal(part) => relative.push(part),
                Component::CurDir => {}
                Component::ParentDir if relative.pop() => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(crate::error::GraphiaError::InvalidArgument(format!(
                        "source path escapes repository root: {}",
                        location.file
                    )));
                }
            }
        }
        let canonical_root = root
            .canonicalize()
            .map_err(|e| crate::error::GraphiaError::Io {
                path: root.to_path_buf(),
                message: e.to_string(),
            })?;
        let candidate = canonical_root.join(relative);
        if candidate.exists() {
            let canonical_file =
                candidate
                    .canonicalize()
                    .map_err(|e| crate::error::GraphiaError::Io {
                        path: candidate.clone(),
                        message: e.to_string(),
                    })?;
            if !canonical_file.starts_with(&canonical_root) {
                return Err(crate::error::GraphiaError::InvalidArgument(format!(
                    "source path escapes repository root: {}",
                    location.file
                )));
            }
            canonical_file
        } else {
            candidate
        }
    } else {
        Path::new(&location.file).to_path_buf()
    };

    let content = if file_path.exists() {
        let full_text =
            fs::read_to_string(&file_path).map_err(|e| crate::error::GraphiaError::Io {
                path: file_path.clone(),
                message: e.to_string(),
            })?;
        extract_lines(
            &full_text,
            location.start_line,
            location.start_col,
            location.end_line,
            location.end_col,
        )
    } else {
        // Placeholder / empty if file not on disk
        String::new()
    };

    let approx_tokens = estimate_approx_tokens(&content);
    let bytes = content.len();
    let characters = content.chars().count();

    Ok(SourceSlice {
        file: location.file.clone(),
        start_line: location.start_line,
        start_col: location.start_col,
        end_line: location.end_line,
        end_col: location.end_col,
        content,
        approx_tokens,
        bytes,
        characters,
    })
}

#[must_use]
pub fn extract_lines(
    text: &str,
    start_line: u32,
    start_col: u32,
    end_line: u32,
    end_col: u32,
) -> String {
    // 1-indexed lines
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() || start_line == 0 {
        return String::new();
    }

    let start_idx = (start_line as usize).saturating_sub(1);
    let end_idx = (end_line as usize).min(lines.len());

    if start_idx >= lines.len() || start_idx >= end_idx {
        return String::new();
    }

    let selected = &lines[start_idx..end_idx];
    if selected.len() == 1 {
        return slice_line(selected[0], start_col, end_col);
    }

    let mut result = Vec::with_capacity(selected.len());
    result.push(slice_line(selected[0], start_col, 1));
    result.extend(
        selected[1..selected.len() - 1]
            .iter()
            .map(|line| (*line).to_string()),
    );
    result.push(slice_line(selected[selected.len() - 1], 1, end_col));
    result.join("\n")
}

fn slice_line(line: &str, start_col: u32, end_col: u32) -> String {
    let mut start = (start_col.saturating_sub(1) as usize).min(line.len());
    while start < line.len() && !line.is_char_boundary(start) {
        start += 1;
    }

    // Column 1 has historically meant "include the full line" for callers that
    // only have line-level locations.
    let mut end = if end_col <= 1 {
        line.len()
    } else {
        (end_col.saturating_sub(1) as usize).min(line.len())
    };
    while end > start && !line.is_char_boundary(end) {
        end -= 1;
    }

    line[start..end.max(start)].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn slicing_honors_columns_without_splitting_utf8() {
        assert_eq!(extract_lines("prefix café suffix", 1, 8, 1, 13), "café");
        assert_eq!(extract_lines("éclair", 1, 2, 1, 8), "clair");
    }

    #[test]
    fn source_slice_rejects_paths_outside_repository() {
        let parent = tempdir().expect("parent");
        let root = parent.path().join("repo");
        fs::create_dir(&root).expect("repo");
        fs::write(parent.path().join("outside.rs"), "secret").expect("outside file");
        let location = SourceLocation {
            file: "../outside.rs".to_string(),
            start_line: 1,
            start_col: 1,
            end_line: 1,
            end_col: 1,
        };

        assert!(matches!(
            extract_source_slice(Some(&root), &location),
            Err(crate::error::GraphiaError::InvalidArgument(_))
        ));

        let missing = SourceLocation {
            file: "../missing.rs".to_string(),
            ..location.clone()
        };
        assert!(matches!(
            extract_source_slice(Some(&root), &missing),
            Err(crate::error::GraphiaError::InvalidArgument(_))
        ));

        let absolute = SourceLocation {
            file: parent.path().join("missing.rs").display().to_string(),
            ..location
        };
        assert!(matches!(
            extract_source_slice(Some(&root), &absolute),
            Err(crate::error::GraphiaError::InvalidArgument(_))
        ));
    }
}
