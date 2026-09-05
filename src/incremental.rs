use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::daemon::debounce::SemanticAction;
use crate::error::{GraphiaError, Result};
use crate::graph::{Graph, build_graph};
use crate::model::{EdgeIdentity, EdgeKind, Language, NodeId, NodeKind};
use crate::parser::{ParsedFile, parse_bytes};
use crate::scan::{ScannedFile, scan_repo};
use crate::storage::{FileMetadata, Metadata, compare_metadata, metadata_for_files};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const CACHE_SCHEMA_VERSION: u32 = crate::storage::FILE_SCHEMA_VERSION;
type ResolutionKey = (u8, String);

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
    pub file_nodes: BTreeMap<String, Vec<NodeId>>,
    pub file_edges: BTreeMap<String, Vec<EdgeIdentity>>,
    pub import_dependents: BTreeMap<String, BTreeSet<String>>,
    pub type_dependents: BTreeMap<String, BTreeSet<String>>,
    symbol_providers: BTreeMap<String, BTreeSet<String>>,
    import_targets: BTreeMap<String, BTreeSet<String>>,
    pub pending_resolution: BTreeMap<(u8, String), BTreeSet<String>>,
    pub resolution_dependents: BTreeMap<(u8, String), BTreeSet<String>>,
    pending_resolution_files: BTreeMap<String, BTreeSet<ResolutionKey>>,
    resolution_dependency_files: BTreeMap<String, BTreeSet<ResolutionKey>>,
    pub fallback_reconcile_count: usize,
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncrementalUpdateSummary {
    pub files_reparsed: usize,
    pub files_parsed: usize,
    pub affected_files: BTreeSet<String>,
    pub files_affected: usize,
    pub nodes_mutated: usize,
    pub nodes_added: usize,
    pub nodes_removed: usize,
    pub edges_added: usize,
    pub edges_removed: usize,
    pub full_rebuild: bool,
    pub fallback_used: bool,
    pub fallback_reason: Option<String>,
    /// Number of direct pending-index buckets queried by this update.
    pub pending_index_lookups: usize,
    /// Number of pending consumers returned by those direct lookups.
    pub pending_entries_examined: usize,
    /// Number of direct all-reference dependency buckets queried by this update.
    pub resolution_index_lookups: usize,
    /// Number of resolved/ambiguous consumers returned by those direct lookups.
    pub resolution_entries_examined: usize,
    /// Ordinary selective updates must keep this at zero.
    pub full_pending_scans: usize,
}

impl IncrementalWorkspace {
    pub fn new(repo_root: PathBuf) -> Result<Self> {
        let mut ws = Self {
            repo_root: repo_root.clone(),
            files: BTreeMap::new(),
            metadata: BTreeMap::new(),
            graph: Graph::new(Vec::new(), Vec::new()),
            file_nodes: BTreeMap::new(),
            file_edges: BTreeMap::new(),
            import_dependents: BTreeMap::new(),
            type_dependents: BTreeMap::new(),
            symbol_providers: BTreeMap::new(),
            import_targets: BTreeMap::new(),
            pending_resolution: BTreeMap::new(),
            resolution_dependents: BTreeMap::new(),
            pending_resolution_files: BTreeMap::new(),
            resolution_dependency_files: BTreeMap::new(),
            fallback_reconcile_count: 0,
            fallback_reason: None,
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
        graph.set_source_root(self.repo_root.clone());
        graph.resolve_cross_file()?;
        graph.canonicalize()?;
        self.graph = graph;
        self.rebuild_indexes();
        self.pending_resolution.clear();
        self.resolution_dependents.clear();
        self.pending_resolution_files.clear();
        self.resolution_dependency_files.clear();
        self.rebuild_pending_index(self.files.keys().cloned().collect());
        Ok(())
    }

    pub fn apply_changes(&mut self, actions: &[SemanticAction]) -> Result<bool> {
        Ok(self.apply_changes_selective(actions)?.files_reparsed > 0
            || actions
                .iter()
                .any(|action| !matches!(action, SemanticAction::Modified(_))))
    }

    pub fn compute_affected_closure(&self, changed_files: &[String]) -> BTreeSet<String> {
        let mut affected = changed_files.iter().cloned().collect::<BTreeSet<_>>();
        let mut pending = changed_files.to_vec();
        while let Some(file) = pending.pop() {
            let dependents = self
                .import_dependents
                .get(&file)
                .into_iter()
                .flatten()
                .chain(self.type_dependents.get(&file).into_iter().flatten())
                .cloned()
                .collect::<Vec<_>>();
            for dependent in dependents {
                if affected.insert(dependent.clone()) {
                    pending.push(dependent);
                }
            }
        }
        affected
    }

    pub fn apply_changes_selective(
        &mut self,
        actions: &[SemanticAction],
    ) -> Result<IncrementalUpdateSummary> {
        if actions.is_empty() {
            return Ok(IncrementalUpdateSummary {
                files_reparsed: 0,
                files_parsed: 0,
                affected_files: BTreeSet::new(),
                files_affected: 0,
                nodes_mutated: 0,
                nodes_added: 0,
                nodes_removed: 0,
                edges_added: 0,
                edges_removed: 0,
                full_rebuild: false,
                fallback_used: false,
                fallback_reason: None,
                pending_index_lookups: 0,
                pending_entries_examined: 0,
                resolution_index_lookups: 0,
                resolution_entries_examined: 0,
                full_pending_scans: 0,
            });
        }

        let mut changed_files = BTreeSet::new();
        // Keep old semantic surfaces while replacing parsed files.  Removed names
        // must invalidate formerly-resolved consumers just as added names wake
        // unresolved consumers.
        let mut previous = BTreeMap::new();
        let mut files_reparsed = 0;
        let mut fallback_reason = None;
        for action in actions {
            match action {
                SemanticAction::Created(path) | SemanticAction::Modified(path) => {
                    let (full_path, rel_path) = self.normalize_path(path);
                    changed_files.insert(rel_path.clone());
                    if let Some((_, parsed)) = self.files.get(&rel_path) {
                        previous.insert(rel_path.clone(), parsed.clone());
                    }
                    if let Some(lang) = crate::scan::detect_language(&full_path) {
                        if full_path.exists() {
                            let content =
                                fs::read(&full_path).map_err(|error| GraphiaError::Io {
                                    path: full_path.clone(),
                                    message: error.to_string(),
                                })?;
                            let parsed = parse_bytes(&rel_path, lang, &content)?;
                            self.files.insert(rel_path.clone(), (Some(lang), parsed));
                            if let Ok(metadata) = fs::metadata(&full_path) {
                                self.metadata.insert(
                                    rel_path.clone(),
                                    FileMetadata {
                                        path: rel_path.clone(),
                                        size: metadata.len(),
                                        modified_ns: metadata
                                            .modified()
                                            .ok()
                                            .and_then(|value| {
                                                value.duration_since(std::time::UNIX_EPOCH).ok()
                                            })
                                            .map(|value| value.as_nanos()),
                                        hash: Sha256::digest(&content).into(),
                                        language: Some(lang),
                                        parser_version: CACHE_SCHEMA_VERSION,
                                    },
                                );
                            }
                            files_reparsed += 1;
                        } else {
                            self.files.remove(&rel_path);
                            self.metadata.remove(&rel_path);
                        }
                    } else {
                        self.files.remove(&rel_path);
                        self.metadata.remove(&rel_path);
                    }
                }
                SemanticAction::Removed(path) => {
                    let (_, rel_path) = self.normalize_path(path);
                    changed_files.insert(rel_path.clone());
                    if let Some((_, parsed)) = self.files.get(&rel_path) {
                        previous.insert(rel_path.clone(), parsed.clone());
                    }
                    self.files.remove(&rel_path);
                    self.metadata.remove(&rel_path);
                }
                SemanticAction::Renamed { from, to } => {
                    let (_, from_rel) = self.normalize_path(from);
                    let (to_full, to_rel) = self.normalize_path(to);
                    if !self.files.contains_key(&from_rel) || !to_full.exists() {
                        fallback_reason = Some(format!("unknown rename: {from_rel} -> {to_rel}"));
                        break;
                    }
                    changed_files.insert(from_rel.clone());
                    changed_files.insert(to_rel.clone());
                    if let Some((_, parsed)) = self.files.get(&from_rel) {
                        previous.insert(from_rel.clone(), parsed.clone());
                    }
                    self.files.remove(&from_rel);
                    self.metadata.remove(&from_rel);
                    let Some(lang) = crate::scan::detect_language(&to_full) else {
                        continue;
                    };
                    let content = fs::read(&to_full).map_err(|error| GraphiaError::Io {
                        path: to_full.clone(),
                        message: error.to_string(),
                    })?;
                    self.files.insert(
                        to_rel.clone(),
                        (Some(lang), parse_bytes(&to_rel, lang, &content)?),
                    );
                    if let Ok(metadata) = fs::metadata(&to_full) {
                        self.metadata.insert(
                            to_rel.clone(),
                            FileMetadata {
                                path: to_rel.clone(),
                                size: metadata.len(),
                                modified_ns: metadata
                                    .modified()
                                    .ok()
                                    .and_then(|value| {
                                        value.duration_since(std::time::UNIX_EPOCH).ok()
                                    })
                                    .map(|value| value.as_nanos()),
                                hash: Sha256::digest(&content).into(),
                                language: Some(lang),
                                parser_version: CACHE_SCHEMA_VERSION,
                            },
                        );
                    }
                    files_reparsed += 1;
                }
            }
        }

        if let Some(reason) = fallback_reason {
            self.fallback_reason = Some(reason.clone());
            self.reconcile_full()?;
            return Ok(IncrementalUpdateSummary {
                files_reparsed,
                files_parsed: files_reparsed,
                affected_files: changed_files,
                files_affected: self.files.len(),
                nodes_mutated: self.graph.nodes.len(),
                nodes_added: self.graph.nodes.len(),
                nodes_removed: 0,
                edges_added: self.graph.edges.len(),
                edges_removed: 0,
                full_rebuild: true,
                fallback_used: true,
                fallback_reason: Some(reason),
                pending_index_lookups: 0,
                pending_entries_examined: 0,
                resolution_index_lookups: 0,
                resolution_entries_examined: 0,
                full_pending_scans: 0,
            });
        }

        if changed_files.is_empty() {
            return Ok(IncrementalUpdateSummary {
                files_reparsed,
                files_parsed: files_reparsed,
                affected_files: BTreeSet::new(),
                files_affected: 0,
                nodes_mutated: 0,
                nodes_added: 0,
                nodes_removed: 0,
                edges_added: 0,
                edges_removed: 0,
                full_rebuild: false,
                fallback_used: false,
                fallback_reason: None,
                pending_index_lookups: 0,
                pending_entries_examined: 0,
                resolution_index_lookups: 0,
                resolution_entries_examined: 0,
                full_pending_scans: 0,
            });
        }
        let dependency_hints = self.new_dependency_hints(&changed_files);
        let (
            reverse_resolution,
            pending_index_lookups,
            pending_entries_examined,
            resolution_index_lookups,
            resolution_entries_examined,
        ) = self.reverse_resolution_hints(&changed_files, &previous);
        let mut dependency_seeds = changed_files.clone();
        dependency_seeds.extend(dependency_hints.iter().cloned());
        dependency_seeds.extend(reverse_resolution.iter().cloned());
        let affected_files =
            self.compute_affected_closure(&dependency_seeds.iter().cloned().collect::<Vec<_>>());
        let resolution_files = affected_files.clone();
        let mutation_files = self.expand_graph_component(&affected_files);
        let mutation_files = mutation_files
            .difference(&dependency_hints)
            .cloned()
            .collect::<BTreeSet<_>>();
        let mutation_files = mutation_files
            .difference(&reverse_resolution)
            .cloned()
            .chain(changed_files.iter().cloned())
            .collect::<BTreeSet<_>>();
        let old_nodes = self.graph.nodes.len();
        let old_component_nodes = self
            .graph
            .nodes
            .iter()
            .filter(|node| mutation_files.contains(&node.file))
            .count();
        let old_component_edges = self
            .graph
            .edges
            .iter()
            .filter(|edge| {
                self.graph
                    .nodes
                    .iter()
                    .find(|node| node.id == edge.from)
                    .is_some_and(|node| mutation_files.contains(&node.file))
            })
            .count();
        let parsed_entries = self
            .files
            .iter()
            .filter(|(path, _)| mutation_files.contains(*path))
            .map(|(p, (lang, parsed))| (p.clone(), *lang, parsed.clone()))
            .collect();
        let fragment = build_graph(parsed_entries);
        let new_component_nodes = fragment.nodes.len();
        let removed_node_ids = self
            .graph
            .nodes
            .iter()
            .filter(|node| mutation_files.contains(&node.file))
            .map(|node| node.id)
            .collect::<BTreeSet<_>>();
        self.graph
            .nodes
            .retain(|node| !mutation_files.contains(&node.file));
        self.graph.edges.retain(|edge| {
            !removed_node_ids.contains(&edge.from) && !removed_node_ids.contains(&edge.to)
        });
        self.graph.nodes.extend(fragment.nodes);
        self.graph.edges.extend(fragment.edges);
        self.graph.update_resolution_context(
            changed_files
                .iter()
                .map(|path| {
                    self.files.get(path).map_or_else(
                        || (path.clone(), None, None),
                        |(language, parsed)| (path.clone(), *language, Some(parsed.clone())),
                    )
                })
                .collect(),
        );
        self.graph.set_source_root(self.repo_root.clone());
        self.graph.resolve_cross_file_affected(&affected_files)?;
        let new_component_edges = self
            .graph
            .edges
            .iter()
            .filter(|edge| {
                self.graph
                    .nodes
                    .iter()
                    .find(|node| node.id == edge.from)
                    .is_some_and(|node| mutation_files.contains(&node.file))
            })
            .count();
        self.rebuild_indexes();
        self.rebuild_pending_index(resolution_files.clone());
        self.fallback_reason = None;
        Ok(IncrementalUpdateSummary {
            files_reparsed,
            files_parsed: files_reparsed,
            affected_files,
            files_affected: mutation_files.len(),
            nodes_mutated: old_nodes.abs_diff(self.graph.nodes.len()),
            nodes_added: new_component_nodes,
            nodes_removed: old_component_nodes,
            edges_added: new_component_edges,
            edges_removed: old_component_edges,
            full_rebuild: false,
            fallback_used: false,
            fallback_reason: None,
            pending_index_lookups,
            pending_entries_examined,
            resolution_index_lookups,
            resolution_entries_examined,
            full_pending_scans: 0,
        })
    }

    fn expand_graph_component(&self, seed: &BTreeSet<String>) -> BTreeSet<String> {
        let mut files = seed.clone();
        let mut pending = seed.iter().cloned().collect::<Vec<_>>();
        while let Some(file) = pending.pop() {
            let node_ids = self
                .graph
                .nodes
                .iter()
                .filter(|node| node.file == file)
                .map(|node| node.id)
                .collect::<BTreeSet<_>>();
            for edge in &self.graph.edges {
                if !matches!(
                    edge.kind,
                    EdgeKind::Imports
                        | EdgeKind::References
                        | EdgeKind::TypeReferences
                        | EdgeKind::Inherits
                        | EdgeKind::Implements
                        | EdgeKind::Instantiates
                        | EdgeKind::Exports
                        | EdgeKind::Calls
                ) {
                    continue;
                }
                if !node_ids.contains(&edge.from) && !node_ids.contains(&edge.to) {
                    continue;
                }
                for id in [edge.from, edge.to] {
                    if let Some(node) = self.graph.nodes.iter().find(|node| node.id == id)
                        && files.insert(node.file.clone())
                    {
                        pending.push(node.file.clone());
                    }
                }
            }
        }
        files
    }

    fn new_dependency_hints(&self, changed_files: &BTreeSet<String>) -> BTreeSet<String> {
        let mut hints = BTreeSet::new();
        for file in changed_files {
            let Some((_, parsed)) = self.files.get(file) else {
                continue;
            };
            let mut names = BTreeSet::new();
            names.extend(
                parsed
                    .type_references
                    .iter()
                    .map(|item| semantic_name(&item.name)),
            );
            names.extend(
                parsed
                    .references
                    .iter()
                    .map(|item| semantic_name(&item.name)),
            );
            names.extend(
                parsed
                    .instantiations
                    .iter()
                    .map(|item| semantic_name(&item.type_name)),
            );
            names.extend(parsed.inheritances.iter().flat_map(|item| {
                [
                    semantic_name(&item.base_type),
                    semantic_name(&item.derived_type),
                ]
            }));
            names.extend(parsed.implementations.iter().flat_map(|item| {
                [
                    semantic_name(&item.trait_or_interface),
                    semantic_name(&item.implementing_type),
                ]
            }));
            names.extend(parsed.calls.iter().map(|item| semantic_name(&item.callee)));
            for name in names {
                if let Some(providers) = self.symbol_providers.get(&name) {
                    hints.extend(providers.iter().cloned());
                }
            }
            for import in &parsed.imports {
                for key in import_lookup_keys(&import.path) {
                    if let Some(targets) = self.import_targets.get(&key) {
                        hints.extend(targets.iter().cloned());
                    }
                }
            }
        }
        hints
    }

    fn reverse_resolution_hints(
        &self,
        changed_files: &BTreeSet<String>,
        previous: &BTreeMap<String, ParsedFile>,
    ) -> (BTreeSet<String>, usize, usize, usize, usize) {
        let mut names = BTreeSet::new();
        for file in changed_files.iter().chain(previous.keys()) {
            names.extend(import_lookup_keys(file));
        }
        for parsed in changed_files
            .iter()
            .filter_map(|file| self.files.get(file).map(|(_, parsed)| parsed))
            .chain(previous.values())
        {
            names.extend(
                parsed
                    .symbols
                    .iter()
                    .map(|symbol| semantic_name(&symbol.name)),
            );
            // Exports include Rust `pub use` re-exports.  They are a provider
            // surface even when no new declaration node was parsed.
            names.extend(parsed.exports.iter().flat_map(|export| {
                std::iter::once(semantic_name(&export.name))
                    .chain(export.target.as_deref().map(semantic_name))
            }));
        }

        let mut consumers = BTreeSet::new();
        let mut lookups = 0;
        let mut entries = 0;
        let mut resolution_lookups = 0;
        let mut resolution_entries = 0;
        for name in names {
            for kind in [
                EdgeKind::References,
                EdgeKind::TypeReferences,
                EdgeKind::Calls,
                EdgeKind::Instantiates,
                EdgeKind::Inherits,
                EdgeKind::Implements,
                EdgeKind::Imports,
                EdgeKind::Exports,
            ] {
                lookups += 1;
                if let Some(files) = self.pending_resolution.get(&(kind.code(), name.clone())) {
                    entries += files.len();
                    consumers.extend(
                        files
                            .iter()
                            .filter(|file| !changed_files.contains(*file))
                            .cloned(),
                    );
                }
                resolution_lookups += 1;
                if let Some(files) = self.resolution_dependents.get(&(kind.code(), name.clone())) {
                    resolution_entries += files.len();
                    consumers.extend(
                        files
                            .iter()
                            .filter(|file| !changed_files.contains(*file))
                            .cloned(),
                    );
                }
            }
        }
        (
            consumers,
            lookups,
            entries,
            resolution_lookups,
            resolution_entries,
        )
    }

    fn rebuild_pending_index(&mut self, files: BTreeSet<String>) {
        for file in files {
            remove_owned_resolution_entries(
                &mut self.pending_resolution,
                &mut self.pending_resolution_files,
                &file,
            );
            remove_owned_resolution_entries(
                &mut self.resolution_dependents,
                &mut self.resolution_dependency_files,
                &file,
            );
            let Some((_, parsed)) = self.files.get(&file).cloned() else {
                continue;
            };
            let mut dependencies = Vec::new();
            let mut pending = Vec::new();
            for item in &parsed.calls {
                let key = (EdgeKind::Calls.code(), semantic_name(&item.callee));
                dependencies.push(key.clone());
                if !self.graph.call_is_resolved(&file, item) {
                    pending.push(key);
                }
            }
            {
                let mut add = |kind: EdgeKind, name: &str| {
                    let key = (kind.code(), semantic_name(name));
                    dependencies.push(key.clone());
                    if self.reference_is_unresolved(&file, kind, name) {
                        pending.push(key);
                    }
                };
                for item in &parsed.references {
                    add(EdgeKind::References, &item.name);
                }
                for item in &parsed.type_references {
                    add(EdgeKind::TypeReferences, &item.name);
                }
                for item in &parsed.instantiations {
                    add(EdgeKind::Instantiates, &item.type_name);
                }
                for item in &parsed.inheritances {
                    add(EdgeKind::Inherits, &item.base_type);
                }
                for item in &parsed.implementations {
                    add(EdgeKind::Implements, &item.trait_or_interface);
                }
            }
            for item in &parsed.imports {
                let non_definitive =
                    self.reference_is_unresolved(&file, EdgeKind::Imports, &item.path);
                for name in import_lookup_keys(&item.path) {
                    let key = (EdgeKind::Imports.code(), name);
                    dependencies.push(key.clone());
                    if non_definitive {
                        pending.push(key);
                    }
                }
            }
            for item in &parsed.exports {
                let key = (EdgeKind::Exports.code(), semantic_name(&item.name));
                dependencies.push(key.clone());
                if self.reference_is_unresolved(&file, EdgeKind::Exports, &item.name) {
                    pending.push(key);
                }
            }
            for key in dependencies {
                insert_owned_resolution_entry(
                    &mut self.resolution_dependents,
                    &mut self.resolution_dependency_files,
                    key,
                    &file,
                );
            }
            for key in pending {
                insert_owned_resolution_entry(
                    &mut self.pending_resolution,
                    &mut self.pending_resolution_files,
                    key,
                    &file,
                );
            }
        }
    }

    fn reference_is_unresolved(&self, file: &str, kind: EdgeKind, name: &str) -> bool {
        !self.graph.edges.iter().any(|edge| {
            edge.kind == kind
                && edge.label.as_deref().is_some_and(|label| label == name)
                && self
                    .graph
                    .nodes
                    .iter()
                    .find(|node| node.id == edge.from)
                    .is_some_and(|node| node.file == file)
        })
    }

    fn normalize_path(&self, path: &Path) -> (PathBuf, String) {
        let full = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.repo_root.join(path)
        };
        let rel = full
            .strip_prefix(&self.repo_root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        (full, rel)
    }

    fn rebuild_indexes(&mut self) {
        self.file_nodes.clear();
        self.file_edges.clear();
        self.import_dependents.clear();
        self.type_dependents.clear();
        self.symbol_providers.clear();
        self.import_targets.clear();
        for file in self.files.keys() {
            for key in import_lookup_keys(file) {
                self.import_targets
                    .entry(key)
                    .or_default()
                    .insert(file.clone());
            }
        }
        for node in &self.graph.nodes {
            self.file_nodes
                .entry(node.file.clone())
                .or_default()
                .push(node.id);
            if node.kind != NodeKind::File {
                for key in [
                    semantic_name(&node.name),
                    semantic_name(&node.qualified_name),
                ] {
                    self.symbol_providers
                        .entry(key)
                        .or_default()
                        .insert(node.file.clone());
                }
            }
        }
        for edge in &self.graph.edges {
            let Some(from) = self.graph.nodes.iter().find(|node| node.id == edge.from) else {
                continue;
            };
            let identity = EdgeIdentity::new(
                edge.from,
                edge.to,
                edge.kind,
                edge.confidence,
                edge.label.clone(),
            );
            self.file_edges
                .entry(from.file.clone())
                .or_default()
                .push(identity);
            if let Some(to) = self.graph.nodes.iter().find(|node| node.id == edge.to) {
                if edge.kind == EdgeKind::Imports {
                    self.import_dependents
                        .entry(to.file.clone())
                        .or_default()
                        .insert(from.file.clone());
                }
                if edge.kind == EdgeKind::TypeReferences
                    || edge.kind == EdgeKind::Inherits
                    || edge.kind == EdgeKind::Implements
                {
                    self.type_dependents
                        .entry(to.file.clone())
                        .or_default()
                        .insert(from.file.clone());
                }
            }
        }
    }
}

fn insert_owned_resolution_entry(
    index: &mut BTreeMap<ResolutionKey, BTreeSet<String>>,
    owners: &mut BTreeMap<String, BTreeSet<ResolutionKey>>,
    key: ResolutionKey,
    file: &str,
) {
    index
        .entry(key.clone())
        .or_default()
        .insert(file.to_string());
    owners.entry(file.to_string()).or_default().insert(key);
}

fn remove_owned_resolution_entries(
    index: &mut BTreeMap<ResolutionKey, BTreeSet<String>>,
    owners: &mut BTreeMap<String, BTreeSet<ResolutionKey>>,
    file: &str,
) {
    let Some(keys) = owners.remove(file) else {
        return;
    };
    for key in keys {
        let empty = index.get_mut(&key).is_some_and(|files| {
            files.remove(file);
            files.is_empty()
        });
        if empty {
            index.remove(&key);
        }
    }
}

fn import_lookup_keys(value: &str) -> BTreeSet<String> {
    const EXTENSIONS: [&str; 28] = [
        ".rs", ".py", ".ts", ".tsx", ".mts", ".cts", ".js", ".mjs", ".cjs", ".jsx", ".go", ".c",
        ".h", ".cpp", ".cc", ".cxx", ".hpp", ".hxx", ".hh", ".java", ".cs", ".kt", ".kts", ".zig",
        ".php", ".rb", ".swift", ".m",
    ];
    let quoted = value.find(['\'', '"']).and_then(|start| {
        let delimiter = value.as_bytes()[start] as char;
        let rest = &value[start + 1..];
        rest.find(delimiter).map(|end| &rest[..end])
    });
    let unquoted = || {
        let trimmed = value.trim();
        if let Some(rest) = trimmed.strip_prefix("use ") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("from ") {
            rest.split_whitespace().next().unwrap_or(rest)
        } else if let Some(rest) = trimmed.strip_prefix("import ") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("#include ") {
            rest
        } else {
            trimmed
        }
    };
    let candidate = quoted
        .unwrap_or_else(unquoted)
        .trim_matches(|character: char| {
            matches!(
                character,
                '.' | '/' | ';' | '\'' | '"' | '(' | ')' | '<' | '>'
            )
        });
    if candidate.is_empty() {
        return BTreeSet::new();
    }
    let mut normalized = candidate.replace('\\', "/").replace("::", "/");
    if !EXTENSIONS
        .iter()
        .any(|extension| normalized.ends_with(extension))
        && !normalized.contains('/')
        && normalized.contains('.')
    {
        normalized = normalized.replace('.', "/");
    }
    let mut keys = BTreeSet::new();
    keys.insert(normalized.clone());
    if let Some(base) = normalized.rsplit('/').next() {
        keys.insert(base.to_string());
    }
    for extension in EXTENSIONS {
        if let Some(stem) = normalized.strip_suffix(extension) {
            keys.insert(stem.to_string());
            if let Some(base) = stem.rsplit('/').next() {
                keys.insert(base.to_string());
            }
        }
    }
    keys
}

fn semantic_name(name: &str) -> String {
    name.trim_end_matches(';')
        .rsplit([':', '.'])
        .next()
        .unwrap_or(name)
        .to_string()
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
    graph.set_source_root(root.to_path_buf());
    graph.resolve_cross_file()?;
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
    crate::storage::save_graph_json(&graph, &root.join(".graphia/graph.json"))?;
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
