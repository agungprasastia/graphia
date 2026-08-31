use std::path::Path;
use std::sync::mpsc::Sender;

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::error::{GraphiaError, Result};
use crate::scan::detect_language;

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

/// Check if a relative or absolute path component matches exclusion rules.
#[must_use]
pub fn is_excluded_path(path: &Path) -> bool {
    for component in path.components() {
        let name = component.as_os_str().to_string_lossy();
        if EXCLUDE_DIRS.contains(&name.as_ref()) {
            return true;
        }
        // Exclude temporary files (.tmp-*, .swp, .swo, ~)
        if name.starts_with(".tmp-") || name.ends_with(".swp") || name.ends_with('~') {
            return true;
        }
    }
    false
}

/// Check if path is relevant for code graph indexing.
#[must_use]
pub fn is_relevant_source_file(path: &Path) -> bool {
    if is_excluded_path(path) {
        return false;
    }
    // Check if it has a supported language extension or is a directory event
    detect_language(path).is_some()
}

/// Setup recursive notify watcher for repository root.
pub fn create_watcher(
    root: &Path,
    sender: Sender<notify::Result<Event>>,
) -> Result<RecommendedWatcher> {
    let canonical_root = root.canonicalize().map_err(|e| GraphiaError::Io {
        path: root.to_path_buf(),
        message: e.to_string(),
    })?;

    let mut watcher = RecommendedWatcher::new(
        move |res| {
            let _ = sender.send(res);
        },
        Config::default(),
    )
    .map_err(|e| GraphiaError::Storage {
        message: format!("failed to initialize file watcher: {e}"),
    })?;

    watcher
        .watch(&canonical_root, RecursiveMode::Recursive)
        .map_err(|e| GraphiaError::Storage {
            message: format!("failed to watch path {}: {e}", canonical_root.display()),
        })?;

    Ok(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_excluded_paths() {
        assert!(is_excluded_path(Path::new(".git/HEAD")));
        assert!(is_excluded_path(Path::new("target/debug/build")));
        assert!(is_excluded_path(Path::new("src/.tmp-123-1")));
        assert!(is_excluded_path(Path::new("foo/node_modules/bar.js")));
        assert!(!is_excluded_path(Path::new("src/main.rs")));
        assert!(!is_excluded_path(Path::new("tests/fixtures/test.py")));
    }

    #[test]
    fn test_relevant_source_files() {
        assert!(is_relevant_source_file(Path::new("src/lib.rs")));
        assert!(is_relevant_source_file(Path::new("pkg/mod.py")));
        assert!(is_relevant_source_file(Path::new("web/app.tsx")));
        assert!(!is_relevant_source_file(Path::new("Cargo.toml")));
        assert!(!is_relevant_source_file(Path::new("target/debug/lib.rs")));
        assert!(!is_relevant_source_file(Path::new(".git/index")));
    }
}
