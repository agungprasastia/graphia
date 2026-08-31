use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use graphia::storage::build_graph_from_repo;
use tempfile::{TempDir, tempdir};

pub const SEED: u64 = 0x4752_4150_4849_4134;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    Small,
    Medium,
    Large,
}

impl Scale {
    pub const fn file_count(self) -> usize {
        match self {
            Self::Small => 100,
            Self::Medium => 1_000,
            Self::Large => 5_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetMetadata {
    pub files: usize,
    pub source_bytes: usize,
    pub symbols: usize,
    pub edges: usize,
    pub languages: BTreeMap<String, usize>,
    pub seed: u64,
}

#[derive(Debug)]
pub struct GeneratedDataset {
    pub root: TempDir,
    pub metadata: DatasetMetadata,
}

pub fn generate(scale: Scale) -> GeneratedDataset {
    let root = tempdir().expect("create synthetic repository");
    let files = scale.file_count();
    for index in 0..files {
        let (language, extension) = match index % 4 {
            0 => ("rust", "rs"),
            1 => ("python", "py"),
            2 => ("typescript", "ts"),
            _ => ("go", "go"),
        };
        let path = root
            .path()
            .join(format!("src/{language}/module_{index:04}.{extension}"));
        fs::create_dir_all(path.parent().expect("source parent")).expect("create source parent");
        fs::write(&path, source(index, files, extension)).expect("write synthetic source");
    }

    let mut languages = BTreeMap::new();
    for index in 0..files {
        let language = ["rust", "python", "typescript", "go"][index % 4];
        *languages.entry(language.to_string()).or_insert(0) += 1;
    }
    let source_bytes = walk_files(root.path())
        .iter()
        .map(|path| fs::metadata(path).expect("source metadata").len() as usize)
        .sum();
    let graph = build_graph_from_repo(root.path()).expect("build synthetic graph");
    GeneratedDataset {
        root,
        metadata: DatasetMetadata {
            files,
            source_bytes,
            symbols: graph.node_count(),
            edges: graph.edge_count(),
            languages,
            seed: SEED,
        },
    }
}

fn source(index: usize, files: usize, extension: &str) -> String {
    let next = (index + 1) % files;
    match extension {
        "rs" => format!(
            "use crate::module_{next:04}::helper_{next};\npub struct Type{index};\npub fn function_{index}() -> usize {{ helper_{next}() }}\npub fn helper_{index}() -> usize {{ {index} }}\n"
        ),
        "py" => format!(
            "from module_{next:04} import helper_{next}\n\nclass Type{index}: pass\ndef function_{index}(): return helper_{next}()\ndef helper_{index}(): return {index}\n"
        ),
        "ts" => format!(
            "import {{ helper_{next} }} from './module_{next:04}';\nexport interface Type{index} {{ value: number }}\nexport function function_{index}(): number {{ return helper_{next}(); }}\nexport function helper_{index}(): number {{ return {index}; }}\n"
        ),
        _ => format!(
            "package module_{index:04}\n\nimport \"graphia/module_{next:04}\"\ntype Type{index} struct {{ Value int }}\nfunc Function{index}() int {{ return Helper{next}() }}\nfunc Helper{index}() int {{ return {index} }}\n"
        ),
    }
}

fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        for entry in fs::read_dir(path).expect("read synthetic directory") {
            let entry = entry.expect("read synthetic entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}
