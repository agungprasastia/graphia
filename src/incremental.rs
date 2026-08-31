use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::daemon::debounce::SemanticAction;
use crate::error::{GraphiaError, Result};
use crate::graph::{Graph, build_graph};
use crate::model::Language;
use crate::parser::{ParsedFile, parse_bytes};
use crate::scan::{ScannedFile, scan_repo};
use crate::storage::{FileMetadata, Metadata, compare_metadata, metadata_for_files};
use serde::{Deserialize, Serialize};

const CACHE_SCHEMA_VERSION: u32 = crate::storage::FILE_SCHEMA_VERSION;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedFile {
    pub metadata: FileMetadata,
    pub parsed: ParsedFile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cache {
    pub schema_version: u32,
    pub files: Vec<CachedFile>,
}

#[derive(Debug, Clone)]
pub struct IncrementalWorkspace {
    pub repo_root: PathBuf,
    pub files: BTreeMap<String, (Option<Language>, ParsedFile)>,
    pub metadata: BTreeMap<String, FileMetadata>,
    pub graph: Graph,
    pub fallback_reconcile_count: usize,
}

impl IncrementalWorkspace {
    pub fn new(repo_root: PathBuf) -> Result<Self> {
        let mut ws = Self {
            repo_root: repo_root.clone(),
            files: BTreeMap::new(),
            metadata: BTreeMap::new(),
            graph: Graph::new(Vec::new(), Vec::new()),
            fallback_reconcile_count: 0,
        };
        ws.reconcile_full()?;
        // The initial repository build is not a fallback reconcile.
        ws.fallback_reconcile_count = 0;
        Ok(ws)
    }

    pub fn reconcile_full(&mut self) -> Result<()> {
        self.fallback_reconcile_count += 1;
        let scanned = scan_repo(&self.repo_root)?;
        let current = metadata_for_files(&scanned)?;

        let mut parsed_entries = Vec::with_capacity(scanned.len());
        self.files.clear();
        self.metadata.clear();

        for file in scanned {
            let Some(language) = file.language else {
                continue;
            };
            let meta = current
                .files
                .iter()
                .find(|item| item.path == file.relative_path)
                .ok_or_else(|| GraphiaError::Storage {
                    message: "scanned file metadata missing".into(),
                })?
                .clone();

            let content = fs::read(&file.absolute_path).map_err(|error| GraphiaError::Io {
                path: file.absolute_path.clone(),
                message: error.to_string(),
            })?;
            let parsed = parse_bytes(&file.relative_path, language, &content)?;

            self.files
                .insert(file.relative_path.clone(), (file.language, parsed.clone()));
            self.metadata.insert(file.relative_path.clone(), meta);

            parsed_entries.push((file.relative_path, file.language, parsed));
        }

        let mut graph = build_graph(parsed_entries);
        graph.canonicalize()?;
        self.graph = graph;
        Ok(())
    }

    pub fn apply_changes(&mut self, actions: &[SemanticAction]) -> Result<bool> {
        if actions.is_empty() {
            return Ok(false);
        }

        let mut dirty = false;
        for action in actions {
            match action {
                SemanticAction::Created(path) | SemanticAction::Modified(path) => {
                    let full_path = if path.is_absolute() {
                        path.clone()
                    } else {
                        self.repo_root.join(path)
                    };

                    let rel_path = full_path
                        .strip_prefix(&self.repo_root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .replace('\\', "/");

                    let lang = crate::scan::detect_language(&full_path);
                    if let Some(lang) = lang {
                        if full_path.exists() {
                            let content =
                                fs::read(&full_path).map_err(|error| GraphiaError::Io {
                                    path: full_path.clone(),
                                    message: error.to_string(),
                                })?;
                            let parsed = parse_bytes(&rel_path, lang, &content)?;
                            self.files.insert(rel_path.clone(), (Some(lang), parsed));
                            dirty = true;
                        } else {
                            self.files.remove(&rel_path);
                            dirty = true;
                        }
                    } else {
                        self.files.remove(&rel_path);
                        dirty = true;
                    }
                }
                SemanticAction::Removed(path) => {
                    let rel_path = path.to_string_lossy().replace('\\', "/");
                    self.files.remove(&rel_path);
                    dirty = true;
                }
                SemanticAction::Renamed { from, to } => {
                    let from_rel = from.to_string_lossy().replace('\\', "/");
                    let to_full = if to.is_absolute() {
                        to.clone()
                    } else {
                        self.repo_root.join(to)
                    };
                    let to_rel = to_full
                        .strip_prefix(&self.repo_root)
                        .unwrap_or(to)
                        .to_string_lossy()
                        .replace('\\', "/");

                    self.files.remove(&from_rel);
                    let lang = crate::scan::detect_language(&to_full);
                    if let Some(lang) = lang {
                        if to_full.exists() {
                            let content = fs::read(&to_full).map_err(|error| GraphiaError::Io {
                                path: to_full.clone(),
                                message: error.to_string(),
                            })?;
                            let parsed = parse_bytes(&to_rel, lang, &content)?;
                            self.files.insert(to_rel, (Some(lang), parsed));
                            dirty = true;
                        }
                    }
                }
            }
        }

        if dirty {
            let parsed_entries: Vec<(String, Option<Language>, ParsedFile)> = self
                .files
                .iter()
                .map(|(p, (lang, pf))| (p.clone(), *lang, pf.clone()))
                .collect();
            let mut graph = build_graph(parsed_entries);
            graph.canonicalize()?;
            self.graph = graph;
        }

        Ok(dirty)
    }
}

fn validate_cache(cache: &Cache) -> bool {
    if cache.schema_version != CACHE_SCHEMA_VERSION {
        return false;
    }
    let mut paths = std::collections::BTreeSet::new();
    cache.files.iter().all(|file| {
        let path = file.metadata.path.as_str();
        !path.is_empty()
            && paths.insert(path)
            && file.metadata.parser_version == CACHE_SCHEMA_VERSION
            && file.metadata.language.is_some()
            && file
                .parsed
                .symbols
                .iter()
                .all(|item| item.location.file == path)
            && file
                .parsed
                .imports
                .iter()
                .all(|item| item.location.file == path)
            && file
                .parsed
                .calls
                .iter()
                .all(|item| item.location.file == path)
    })
}

pub fn classify_changes(
    previous: &[FileMetadata],
    current: &[ScannedFile],
) -> Result<Vec<FileChange>> {
    let current_metadata = metadata_for_files(current)?;
    Ok(compare_metadata(
        Some(&Metadata {
            schema_version: CACHE_SCHEMA_VERSION,
            files: previous.to_vec(),
        }),
        &current_metadata,
    )
    .into_iter()
    .map(|change| FileChange {
        path: change.path,
        kind: match change.change {
            crate::storage::FileChange::Added => ChangeKind::Added,
            crate::storage::FileChange::Modified => ChangeKind::Modified,
            crate::storage::FileChange::Deleted => ChangeKind::Deleted,
            crate::storage::FileChange::Unchanged => ChangeKind::Unchanged,
        },
    })
    .collect())
}

pub fn update_repository(root: &Path) -> Result<Graph> {
    let scanned = scan_repo(root)?;
    let current = metadata_for_files(&scanned)?;
    let cache_path = root.join(".graphia/parsed.json");
    let previous = load_cache(&cache_path)?;
    let previous_by_path = previous
        .as_ref()
        .map(|cache| {
            cache
                .files
                .iter()
                .map(|file| (file.metadata.path.as_str(), file))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let mut cached_files = Vec::with_capacity(scanned.len());
    let mut parsed = Vec::with_capacity(scanned.len());
    for file in scanned {
        let Some(language) = file.language else {
            continue;
        };
        let metadata = current
            .files
            .iter()
            .find(|item| item.path == file.relative_path)
            .ok_or_else(|| GraphiaError::Storage {
                message: "scanned file metadata missing".into(),
            })?;
        let cached = previous_by_path.get(file.relative_path.as_str());
        let parsed_file = if let Some(cached) = cached.filter(|item| {
            item.metadata.hash == metadata.hash
                && item.metadata.language == metadata.language
                && item.metadata.parser_version == metadata.parser_version
        }) {
            cached.parsed.clone()
        } else {
            let content = fs::read(&file.absolute_path).map_err(|error| GraphiaError::Io {
                path: file.absolute_path.clone(),
                message: error.to_string(),
            })?;
            parse_bytes(&file.relative_path, language, &content)?
        };
        parsed.push((
            file.relative_path.clone(),
            file.language,
            parsed_file.clone(),
        ));
        cached_files.push(CachedFile {
            metadata: metadata.clone(),
            parsed: parsed_file,
        });
    }
    let mut graph = build_graph(parsed);
    graph.canonicalize()?;
    let cache = Cache {
        schema_version: CACHE_SCHEMA_VERSION,
        files: cached_files,
    };
    let bytes = serde_json::to_vec_pretty(&cache).map_err(|error| GraphiaError::Storage {
        message: error.to_string(),
    })?;
    crate::storage::atomic_write(&cache_path, &bytes)?;
    crate::storage::save_metadata(root, &current)?;
    crate::storage::save_graph_json(&graph, &root.join("graph.json"))?;
    crate::storage::save_graph_binary(&graph, &root.join(".graphia/index.bin"))?;
    Ok(graph)
}

fn load_cache(path: &Path) -> Result<Option<Cache>> {
    if !path.exists() {
        return Ok(None);
    }
    let data = fs::read_to_string(path).map_err(|error| GraphiaError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let Ok(cache) = serde_json::from_str::<Cache>(&data) else {
        return Ok(None);
    };
    if !validate_cache(&cache) {
        return Ok(None);
    }
    Ok(Some(cache))
}
