use std::fs;
use std::path::Path;

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
        root.join(&location.file)
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
    _start_col: u32,
    end_line: u32,
    _end_col: u32,
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

    lines[start_idx..end_idx].join("\n")
}
