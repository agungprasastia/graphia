use std::fs;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::error::{GraphiaError, Result};
use crate::model::Language;

pub const EXCLUDE_DIRS: &[&str] = &[
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

pub fn is_excluded_path(path: &Path) -> bool {
    for component in path.components() {
        let name = component.as_os_str().to_string_lossy();
        if EXCLUDE_DIRS.contains(&name.as_ref()) {
            return true;
        }
        if name.starts_with(".tmp-") || name.ends_with(".swp") || name.ends_with('~') {
            return true;
        }
    }
    false
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedFile {
    pub relative_path: String,
    pub absolute_path: PathBuf,
    pub language: Option<Language>,
}

#[must_use]
pub fn detect_language(path: &Path) -> Option<Language> {
    detect_language_with_content(path, None)
}

#[must_use]
pub fn detect_language_with_content(path: &Path, content: Option<&[u8]>) -> Option<Language> {
    let ext = path.extension()?.to_str()?;
    match ext.to_ascii_lowercase().as_str() {
        "rs" => Some(Language::Rust),
        "py" => Some(Language::Python),
        "ts" | "mts" | "cts" => Some(Language::TypeScript),
        "tsx" => Some(Language::Tsx),
        "js" | "mjs" | "cjs" => Some(Language::JavaScript),
        "jsx" => Some(Language::Jsx),
        "go" => Some(Language::Go),
        "c" => Some(Language::C),
        "h" => {
            if let Some(bytes) = content {
                if is_cpp_header_content(bytes) {
                    return Some(Language::Cpp);
                }
            } else if path.exists()
                && let Ok(file_bytes) = fs::read(path)
                && is_cpp_header_content(&file_bytes)
            {
                return Some(Language::Cpp);
            }
            Some(Language::C)
        }
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" => Some(Language::Cpp),
        "java" => Some(Language::Java),
        "cs" => Some(Language::CSharp),
        "kt" | "kts" => Some(Language::Kotlin),
        "zig" => Some(Language::Zig),
        "php" | "phtml" | "php3" | "php4" | "php5" | "php7" | "phps" => Some(Language::Php),
        "rb" | "erb" => Some(Language::Ruby),
        "swift" => Some(Language::Swift),
        _ => None,
    }
}

fn is_cpp_header_content(bytes: &[u8]) -> bool {
    let inspect_len = bytes.len().min(4096);
    if let Ok(text) = std::str::from_utf8(&bytes[..inspect_len])
        && (text.contains("class ")
            || text.contains("namespace ")
            || text.contains("template<")
            || text.contains("template <")
            || text.contains("public:")
            || text.contains("private:")
            || text.contains("protected:")
            || text.contains("std::")
            || text.contains("using namespace ")
            || text.contains("#include <iostream>")
            || text.contains("#include <vector>")
            || text.contains("#include <string>"))
    {
        return true;
    }
    false
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
        let language = detect_language(&abs);
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
        assert_eq!(detect_language(Path::new("a.tsx")), Some(Language::Tsx));
        assert_eq!(
            detect_language(Path::new("a.js")),
            Some(Language::JavaScript)
        );
        assert_eq!(
            detect_language(Path::new("a.mjs")),
            Some(Language::JavaScript)
        );
        assert_eq!(
            detect_language(Path::new("a.cjs")),
            Some(Language::JavaScript)
        );
        assert_eq!(detect_language(Path::new("a.jsx")), Some(Language::Jsx));
        assert_eq!(detect_language(Path::new("a.go")), Some(Language::Go));
        assert_eq!(detect_language(Path::new("a.c")), Some(Language::C));
        assert_eq!(detect_language(Path::new("a.h")), Some(Language::C));
        assert_eq!(detect_language(Path::new("a.cpp")), Some(Language::Cpp));
        assert_eq!(detect_language(Path::new("a.cc")), Some(Language::Cpp));
        assert_eq!(detect_language(Path::new("a.cxx")), Some(Language::Cpp));
        assert_eq!(detect_language(Path::new("a.hpp")), Some(Language::Cpp));
        assert_eq!(detect_language(Path::new("a.hxx")), Some(Language::Cpp));
        assert_eq!(detect_language(Path::new("a.hh")), Some(Language::Cpp));
        assert_eq!(detect_language(Path::new("a.java")), Some(Language::Java));
        assert_eq!(detect_language(Path::new("a.cs")), Some(Language::CSharp));
        assert_eq!(detect_language(Path::new("a.kt")), Some(Language::Kotlin));
        assert_eq!(detect_language(Path::new("a.kts")), Some(Language::Kotlin));
        assert_eq!(detect_language(Path::new("a.zig")), Some(Language::Zig));
        assert_eq!(detect_language(Path::new("a.php")), Some(Language::Php));
        assert_eq!(detect_language(Path::new("a.rb")), Some(Language::Ruby));
        assert_eq!(detect_language(Path::new("a.swift")), Some(Language::Swift));
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

    #[cfg(unix)]
    #[test]
    fn scan_skips_symlinked_files() {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("real.rs"), "fn real() {}").unwrap();
        symlink(dir.path().join("real.rs"), dir.path().join("linked.rs")).unwrap();
        let files = scan_repo(dir.path()).unwrap();
        assert_eq!(
            files
                .iter()
                .map(|file| file.relative_path.as_str())
                .collect::<Vec<_>>(),
            ["real.rs"]
        );
    }

    #[cfg(windows)]
    #[test]
    fn scan_skips_symlinked_files() {
        use std::os::windows::fs::symlink_file;

        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("real.rs"), "fn real() {}").unwrap();
        if symlink_file(dir.path().join("real.rs"), dir.path().join("linked.rs")).is_err() {
            return;
        }
        let files = scan_repo(dir.path()).unwrap();
        assert_eq!(
            files
                .iter()
                .map(|file| file.relative_path.as_str())
                .collect::<Vec<_>>(),
            ["real.rs"]
        );
    }
}
