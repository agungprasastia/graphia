use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::error::{GraphiaError, Result};
use crate::model::Language;

const EXCLUDE_DIRS: &[&str] = &[
    ".git",
    ".graphia",
    "target",
    "node_modules",
    "dist",
    "build",
    "__pycache__",
    ".venv",
    "venv",
    ".mypy_cache",
    ".pytest_cache",
    "vendor",
    ".next",
    "out",
    "coverage",
    ".tox",
    ".eggs",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedFile {
    pub relative_path: String,
    pub absolute_path: PathBuf,
    pub language: Option<Language>,
}

#[must_use]
pub fn detect_language(path: &Path) -> Option<Language> {
    let ext = path.extension()?.to_str()?;
    match ext.to_ascii_lowercase().as_str() {
        "rs" => Some(Language::Rust),
        "py" => Some(Language::Python),
        "ts" | "tsx" | "mts" | "cts" => Some(Language::TypeScript),
        _ => None,
    }
}

fn is_excluded_dir(name: &str) -> bool {
    EXCLUDE_DIRS.contains(&name)
}

fn normalize_relative(relative: &Path) -> String {
    relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// Scan supported source files in repository.
///
/// # Errors
///
/// Returns an error when repository canonicalization fails.
pub fn scan_repo(root: &Path) -> Result<Vec<ScannedFile>> {
    let canonical_root = root.canonicalize().map_err(|e| GraphiaError::Io {
        path: root.to_path_buf(),
        message: e.to_string(),
    })?;

    let mut files = Vec::new();

    for entry in WalkDir::new(&canonical_root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let file_name = e.file_name().to_string_lossy();
            if e.file_type().is_dir() && is_excluded_dir(&file_name) {
                return false;
            }
            true
        })
    {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                return Err(GraphiaError::Io {
                    path: err
                        .path()
                        .map_or_else(|| canonical_root.clone(), Path::to_path_buf),
                    message: err.to_string(),
                });
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let abs = entry.path().to_path_buf();
        let Ok(rel_path) = abs.strip_prefix(&canonical_root) else {
            continue;
        };
        let relative_path = normalize_relative(rel_path);
        if relative_path.is_empty() {
            continue;
        }
        let language = detect_language(Path::new(&relative_path));
        files.push(ScannedFile {
            relative_path,
            absolute_path: abs,
            language,
        });
    }

    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn language_detection() {
        assert_eq!(detect_language(Path::new("a.rs")), Some(Language::Rust));
        assert_eq!(detect_language(Path::new("a.py")), Some(Language::Python));
        assert_eq!(
            detect_language(Path::new("a.ts")),
            Some(Language::TypeScript)
        );
        assert_eq!(
            detect_language(Path::new("a.tsx")),
            Some(Language::TypeScript)
        );
        assert_eq!(detect_language(Path::new("a.txt")), None);
    }

    #[test]
    fn scan_deterministic_ordering() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("b.rs"), "fn b(){}").unwrap();
        fs::write(dir.path().join("a.rs"), "fn a(){}").unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".git").join("x"), "x").unwrap();
        let files = scan_repo(dir.path()).unwrap();
        let names: Vec<_> = files.iter().map(|f| f.relative_path.as_str()).collect();
        assert_eq!(names, vec!["a.rs", "b.rs"]);
    }

    #[test]
    fn scan_ignores_excluded_dirs() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("target")).unwrap();
        fs::write(dir.path().join("target").join("a.rs"), "fn a(){}").unwrap();
        fs::write(dir.path().join("keep.rs"), "fn keep(){}").unwrap();
        let files = scan_repo(dir.path()).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "keep.rs");
    }
}
