use std::collections::{BTreeMap, HashSet};

use sha2::{Digest, Sha256};

use crate::model::{
    Confidence, Edge, EdgeId, EdgeIdentity, EdgeKind, Language, Node, NodeId, NodeIdentity,
    NodeKind, SourceLocation,
};
use crate::parser::{Call, ParsedFile, Symbol};
use crate::resolve::ResolutionEngine;

#[derive(Debug, Clone)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    resolution: ResolutionReport,
    resolution_calls: Vec<(String, Call)>,
    parsed_files: Vec<(String, Option<Language>, ParsedFile)>,
}

impl PartialEq for Graph {
    fn eq(&self, other: &Self) -> bool {
        self.nodes == other.nodes
            && self.edges == other.edges
            && self.resolution == other.resolution
    }
}

impl Eq for Graph {}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolutionReport {
    pub resolved_calls: usize,
    pub unresolved_calls: usize,
    pub ambiguous_calls: usize,
}

impl Graph {
    pub fn canonicalize(&mut self) -> crate::error::Result<()> {
        self.nodes.sort_by(|a, b| {
            a.qualified_name
                .cmp(&b.qualified_name)
                .then(a.kind.code().cmp(&b.kind.code()))
                .then(a.id.0.cmp(&b.id.0))
        });
        self.edges.sort_by(|a, b| {
            a.kind
                .code()
                .cmp(&b.kind.code())
                .then(a.from.0.cmp(&b.from.0))
                .then(a.to.0.cmp(&b.to.0))
                .then(a.id.0.cmp(&b.id.0))
        });
        self.validate()
    }

    pub fn validate(&self) -> crate::error::Result<()> {
        let node_ids: HashSet<NodeId> = self.nodes.iter().map(|node| node.id).collect();
        if node_ids.len() != self.nodes.len() {
            return Err(crate::error::GraphiaError::Storage {
                message: "duplicate node ID".into(),
            });
        }
        let mut identities = HashSet::new();
        for node in &self.nodes {
            let identity = (
                node.kind,
                node.qualified_name.as_str(),
                node.file.as_str(),
                node.container.as_deref().unwrap_or(""),
                node.signature.as_deref().unwrap_or(""),
            );
            if !identities.insert(identity) {
                return Err(crate::error::GraphiaError::GraphInvariant {
                    message: format!("duplicate node identity {}", node.qualified_name),
                });
            }
            let expected = stable_node_id(&NodeIdentity::from_node(node));
            if node.id != expected {
                return Err(crate::error::GraphiaError::GraphInvariant {
                    message: format!("node ID does not match identity {}", node.qualified_name),
                });
            }
        }
        let edge_ids: HashSet<EdgeId> = self.edges.iter().map(|edge| edge.id).collect();
        if edge_ids.len() != self.edges.len() {
            return Err(crate::error::GraphiaError::Storage {
                message: "duplicate edge ID".into(),
            });
        }
        for edge in &self.edges {
            if !node_ids.contains(&edge.from) || !node_ids.contains(&edge.to) {
                return Err(crate::error::GraphiaError::GraphInvariant {
                    message: format!("dangling edge {}", edge.id.0),
                });
            }
            let expected = stable_edge_id(&EdgeIdentity::new(
                edge.from,
                edge.to,
                edge.kind,
                edge.confidence,
                edge.label.clone(),
            ));
            if edge.id != expected {
                return Err(crate::error::GraphiaError::GraphInvariant {
                    message: format!("edge ID does not match identity {}", edge.id.0),
                });
            }
        }
        if self.nodes.windows(2).any(|pair| {
            (
                pair[0].qualified_name.as_str(),
                pair[0].kind.code(),
                pair[0].id,
            ) > (
                pair[1].qualified_name.as_str(),
                pair[1].kind.code(),
                pair[1].id,
            )
        }) || self.edges.windows(2).any(|pair| {
            (pair[0].kind.code(), pair[0].from, pair[0].to, pair[0].id)
                > (pair[1].kind.code(), pair[1].from, pair[1].to, pair[1].id)
        }) {
            return Err(crate::error::GraphiaError::GraphInvariant {
                message: "records are not canonicalized".into(),
            });
        }
        Ok(())
    }

    #[must_use]
    pub fn new(nodes: Vec<Node>, edges: Vec<Edge>) -> Self {
        Self {
            nodes,
            edges,
            resolution: ResolutionReport::default(),
            resolution_calls: Vec::new(),
            parsed_files: Vec::new(),
        }
    }

    pub fn resolve_cross_file(&mut self) -> crate::error::Result<ResolutionReport> {
        self.nodes.sort_by(|a, b| {
            a.qualified_name
                .cmp(&b.qualified_name)
                .then(a.kind.code().cmp(&b.kind.code()))
                .then(a.id.0.cmp(&b.id.0))
        });
        self.edges.sort_by(|a, b| {
            a.kind
                .code()
                .cmp(&b.kind.code())
                .then(a.from.0.cmp(&b.from.0))
                .then(a.to.0.cmp(&b.to.0))
                .then(a.id.0.cmp(&b.id.0))
        });
        self.edges.retain(|edge| {
            !(edge.kind == EdgeKind::Calls && edge.confidence == Confidence::Inferred)
                && !(edge.kind == EdgeKind::Calls && edge.confidence == Confidence::Extracted)
        });

        let imports: Vec<(NodeId, NodeId)> = self
            .edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Imports)
            .map(|edge| (edge.from, edge.to))
            .collect();

        let mut engine = ResolutionEngine::new();
        engine.index_files(&self.nodes, &self.parsed_files);

        let (resolved_edges, summary) =
            engine.resolve_calls(&self.resolution_calls, &self.nodes, &imports);

        self.edges.extend(resolved_edges);
        let mut edge_ids = HashSet::new();
        self.edges.retain(|e| edge_ids.insert(e.id));
        self.resolution = ResolutionReport {
            resolved_calls: summary.resolved_calls,
            unresolved_calls: summary.unresolved_calls,
            ambiguous_calls: summary.ambiguous_calls,
        };
        self.canonicalize()?;
        Ok(self.resolution.clone())
    }

    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

fn stable_id(seed: &str) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    let digest = hasher.finalize();
    u64::from_le_bytes(digest[..8].try_into().unwrap_or([0; 8]))
}

#[must_use]
pub fn stable_node_id(identity: &NodeIdentity) -> NodeId {
    NodeId(stable_id(&format!(
        "node\0{}\0{}\0{}\0{}\0{}\0{}",
        identity.language.map_or(0, Language::code),
        identity.file,
        identity.kind.code(),
        identity.qualified_name,
        identity.container.as_deref().unwrap_or(""),
        identity.signature.as_deref().unwrap_or(""),
    )))
}

#[must_use]
pub fn stable_edge_id(identity: &EdgeIdentity) -> EdgeId {
    EdgeId(stable_id(&format!(
        "edge\0{}\0{}\0{}\0{}\0{}\0{}",
        identity.from.0,
        identity.to.0,
        identity.kind.code(),
        identity.confidence.code(),
        identity.label.is_some(),
        identity.label.as_deref().unwrap_or(""),
    )))
}

#[derive(Debug, Clone)]
struct FileEntry {
    path: String,
    language: Option<Language>,
    parsed: ParsedFile,
}

#[must_use]
pub fn build_graph(files: Vec<(String, Option<Language>, ParsedFile)>) -> Graph {
    let mut entries: Vec<FileEntry> = files
        .into_iter()
        .map(|(path, language, parsed)| FileEntry {
            path,
            language,
            parsed,
        })
        .collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));

    let mut all_symbols: Vec<(String, Symbol)> = entries
        .iter()
        .flat_map(|entry| {
            entry
                .parsed
                .symbols
                .iter()
                .cloned()
                .map(|symbol| (entry.path.clone(), symbol))
        })
        .collect();
    all_symbols.sort_by(|a, b| {
        a.1.qualified_name
            .cmp(&b.1.qualified_name)
            .then(a.1.kind.cmp(&b.1.kind))
            .then(a.0.cmp(&b.0))
            .then(a.1.signature.cmp(&b.1.signature))
            .then(a.1.container.cmp(&b.1.container))
            .then(a.1.location.cmp(&b.1.location))
    });
    all_symbols.dedup_by(|a, b| {
        a.0 == b.0
            && a.1.kind == b.1.kind
            && a.1.qualified_name == b.1.qualified_name
            && a.1.signature == b.1.signature
            && a.1.container == b.1.container
    });

    let mut nodes = Vec::with_capacity(entries.len() + all_symbols.len());
    let mut file_node_ids = BTreeMap::new();
    let mut symbol_node_ids: BTreeMap<String, Vec<NodeId>> = BTreeMap::new();
    let mut name_to_symbols: BTreeMap<String, Vec<(String, NodeId, String)>> = BTreeMap::new();

    let mut resolution = ResolutionReport::default();
    let mut resolution_calls = Vec::new();
    for entry in &entries {
        resolution_calls.extend(
            entry
                .parsed
                .calls
                .iter()
                .cloned()
                .map(|call| (entry.path.clone(), call)),
        );
        let _location = SourceLocation {
            file: entry.path.clone(),
            start_line: 1,
            start_col: 1,
            end_line: 1,
            end_col: 1,
        };
        let id = stable_node_id(&NodeIdentity::new(
            entry.language,
            &entry.path,
            NodeKind::File,
            &entry.path,
            None,
            None,
        ));
        file_node_ids.insert(entry.path.clone(), id);
        nodes.push(Node {
            id,
            kind: NodeKind::File,
            name: entry.path.clone(),
            qualified_name: entry.path.clone(),
            file: entry.path.clone(),
            location: SourceLocation {
                file: entry.path.clone(),
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 1,
            },
            language: entry.language,
            visibility: crate::model::Visibility::Public,
            signature: None,
            container: None,
        });
    }

    for (file, symbol) in &all_symbols {
        let file_entry_lang = entries
            .iter()
            .find(|e| e.path == *file)
            .and_then(|e| e.language);
        let id = stable_node_id(&NodeIdentity::new(
            file_entry_lang,
            file,
            symbol.kind,
            &symbol.qualified_name,
            symbol.container.as_deref().or(symbol.parent.as_deref()),
            symbol.signature.as_deref(),
        ));
        symbol_node_ids
            .entry(symbol.qualified_name.clone())
            .or_default()
            .push(id);
        name_to_symbols
            .entry(symbol.name.clone())
            .or_default()
            .push((symbol.qualified_name.clone(), id, file.clone()));
        nodes.push(Node {
            id,
            kind: symbol.kind,
            name: symbol.name.clone(),
            qualified_name: symbol.qualified_name.clone(),
            file: file.clone(),
            location: symbol.location.clone(),
            language: file_entry_lang,
            visibility: symbol.visibility,
            signature: symbol.signature.clone(),
            container: symbol.container.clone().or_else(|| symbol.parent.clone()),
        });
    }

    let mut edge_specs: Vec<(EdgeKind, NodeId, NodeId, Confidence, Option<String>)> = Vec::new();
    let mut edge_keys = HashSet::new();
    let mut add_edge = |kind, from, to, confidence, label: Option<String>| {
        let key = (kind, from, to, confidence, label.clone());
        if edge_keys.insert(key) {
            edge_specs.push((kind, from, to, confidence, label));
        }
    };

    for (file, symbol) in &all_symbols {
        if let (Some(&file_id), Some(symbol_ids)) = (
            file_node_ids.get(file),
            symbol_node_ids.get(&symbol.qualified_name),
        ) {
            if let Some(&symbol_id) = symbol_ids.first() {
                add_edge(
                    EdgeKind::Contains,
                    file_id,
                    symbol_id,
                    Confidence::Extracted,
                    None,
                );
            }
        }
    }

    for entry in &entries {
        let Some(&from_id) = file_node_ids.get(&entry.path) else {
            continue;
        };
        let mut imports = entry.parsed.imports.clone();
        imports.sort_by(|a, b| a.path.cmp(&b.path).then(a.location.cmp(&b.location)));
        for import in imports {
            if let Some(target) = resolve_import(&entry.path, &import.path, &entries)
                && let Some(&to_id) = file_node_ids.get(&target)
                && from_id != to_id
            {
                add_edge(
                    EdgeKind::Imports,
                    from_id,
                    to_id,
                    Confidence::Inferred,
                    Some(import.path),
                );
            }
        }
    }

    for entry in &entries {
        let mut calls = entry.parsed.calls.clone();
        calls.sort_by(|a, b| {
            a.caller
                .cmp(&b.caller)
                .then(a.callee.cmp(&b.callee))
                .then(a.location.cmp(&b.location))
        });
        for call in calls {
            let Some(caller_id) = symbol_node_ids
                .get(&call.caller)
                .and_then(|ids| ids.first())
                .copied()
            else {
                continue;
            };
            let Some(candidates) = name_to_symbols.get(&call.callee) else {
                resolution.unresolved_calls += 1;
                continue;
            };
            let language_candidates: Vec<_> = candidates
                .iter()
                .filter(|(_, _, file)| match entry.language {
                    Some(Language::Rust) => file.ends_with(".rs"),
                    Some(Language::Python) => file.ends_with(".py"),
                    Some(
                        Language::TypeScript | Language::Tsx | Language::JavaScript | Language::Jsx,
                    ) => {
                        file.ends_with(".ts")
                            || file.ends_with(".tsx")
                            || file.ends_with(".mts")
                            || file.ends_with(".cts")
                            || file.ends_with(".js")
                            || file.ends_with(".mjs")
                            || file.ends_with(".cjs")
                            || file.ends_with(".jsx")
                    }
                    Some(Language::Go) => file.ends_with(".go"),
                    Some(Language::C) => file.ends_with(".c") || file.ends_with(".h"),
                    Some(Language::Cpp) => {
                        file.ends_with(".cpp")
                            || file.ends_with(".cc")
                            || file.ends_with(".cxx")
                            || file.ends_with(".hpp")
                            || file.ends_with(".hxx")
                            || file.ends_with(".hh")
                            || file.ends_with(".h")
                    }
                    Some(Language::Java) => file.ends_with(".java"),
                    Some(Language::CSharp) => file.ends_with(".cs"),
                    Some(Language::Kotlin) => file.ends_with(".kt") || file.ends_with(".kts"),
                    Some(Language::Zig) => file.ends_with(".zig"),
                    Some(Language::Php) => {
                        file.ends_with(".php")
                            || file.ends_with(".phtml")
                            || file.ends_with(".php3")
                            || file.ends_with(".php4")
                            || file.ends_with(".php5")
                            || file.ends_with(".php7")
                            || file.ends_with(".phps")
                    }
                    Some(Language::Ruby) => file.ends_with(".rb") || file.ends_with(".erb"),
                    Some(Language::Swift) => file.ends_with(".swift"),
                    None => true,
                })
                .collect();
            let same_file: Vec<_> = candidates
                .iter()
                .filter(|(qname, _, file)| file == &entry.path && qname != &call.caller)
                .cloned()
                .collect();
            let imported_files: Vec<_> = entry
                .parsed
                .imports
                .iter()
                .filter_map(|import| resolve_import(&entry.path, &import.path, &entries))
                .collect();
            let imported_candidates: Vec<_> = language_candidates
                .iter()
                .filter(|(_, _, file)| imported_files.iter().any(|imported| imported == file))
                .copied()
                .collect();
            let candidate = if same_file.len() == 1 {
                Some(same_file[0].clone())
            } else if imported_candidates.len() == 1 {
                Some(imported_candidates[0].clone())
            } else {
                if same_file.is_empty() && imported_candidates.len() > 1 {
                    resolution.ambiguous_calls += 1;
                } else {
                    resolution.unresolved_calls += 1;
                }
                None
            };
            if let Some((_, callee_id, _)) = candidate {
                resolution.resolved_calls += 1;
                add_edge(
                    EdgeKind::Calls,
                    caller_id,
                    callee_id,
                    Confidence::Inferred,
                    None,
                );
            }
        }

        if let Some(&file_id) = file_node_ids.get(&entry.path) {
            for exp in &entry.parsed.exports {
                if let Some(target_qname) = &exp.target {
                    if let Some(target_ids) = symbol_node_ids.get(target_qname) {
                        if let Some(&target_id) = target_ids.first() {
                            add_edge(
                                EdgeKind::Exports,
                                file_id,
                                target_id,
                                Confidence::Extracted,
                                Some(exp.name.clone()),
                            );
                        }
                    }
                }
            }

            for tref in &entry.parsed.type_references {
                let from_id = if let Some(container) = &tref.container {
                    symbol_node_ids
                        .get(container)
                        .and_then(|ids| ids.first())
                        .copied()
                        .unwrap_or(file_id)
                } else {
                    file_id
                };

                if let Some(candidates) = name_to_symbols.get(&tref.name) {
                    if let Some((_, target_id, _)) = candidates.first() {
                        add_edge(
                            EdgeKind::TypeReferences,
                            from_id,
                            *target_id,
                            Confidence::Inferred,
                            Some(tref.name.clone()),
                        );
                    }
                }
            }

            for rref in &entry.parsed.references {
                let from_id = if let Some(caller) = &rref.caller {
                    symbol_node_ids
                        .get(caller)
                        .and_then(|ids| ids.first())
                        .copied()
                        .unwrap_or(file_id)
                } else {
                    file_id
                };

                if let Some(candidates) = name_to_symbols.get(&rref.name) {
                    if let Some((_, target_id, _)) = candidates.first() {
                        add_edge(
                            EdgeKind::References,
                            from_id,
                            *target_id,
                            Confidence::Inferred,
                            Some(rref.name.clone()),
                        );
                    }
                }
            }
        }
    }

    edge_specs.sort_by(|a, b| {
        a.0.code()
            .cmp(&b.0.code())
            .then(a.1.0.cmp(&b.1.0))
            .then(a.2.0.cmp(&b.2.0))
            .then(a.3.code().cmp(&b.3.code()))
            .then(a.4.cmp(&b.4))
    });
    let edges = edge_specs
        .into_iter()
        .map(|(kind, from, to, confidence, label)| Edge {
            id: stable_edge_id(&EdgeIdentity::new(
                from,
                to,
                kind,
                confidence,
                label.clone(),
            )),
            kind,
            from,
            to,
            confidence,
            label,
        })
        .collect();

    let mut graph = Graph {
        nodes,
        edges,
        resolution,
        resolution_calls,
        parsed_files: entries
            .into_iter()
            .map(|e| (e.path, e.language, e.parsed))
            .collect(),
    };
    graph.nodes.sort_by(|a, b| {
        a.qualified_name
            .cmp(&b.qualified_name)
            .then(a.kind.code().cmp(&b.kind.code()))
            .then(a.id.0.cmp(&b.id.0))
    });
    graph.edges.sort_by(|a, b| {
        a.kind
            .code()
            .cmp(&b.kind.code())
            .then(a.from.0.cmp(&b.from.0))
            .then(a.to.0.cmp(&b.to.0))
            .then(a.id.0.cmp(&b.id.0))
    });
    graph
}

fn resolve_import(
    importing_file: &str,
    import_path: &str,
    entries: &[FileEntry],
) -> Option<String> {
    let text = import_path.trim();
    let candidate = if let Some(candidate) = text
        .split(['\'', '"'])
        .find(|part| part.starts_with('.') || part.starts_with('/'))
    {
        let base = importing_file.rsplit_once('/').map_or("", |(dir, _)| dir);
        let joined = if base.is_empty() {
            candidate.to_string()
        } else {
            format!("{base}/{candidate}")
        };
        normalize_import_path(&joined)
    } else if let Some(path) = text.strip_prefix("use ") {
        path.trim_end_matches(';')
            .trim_start_matches("crate::")
            .replace("::", "/")
    } else if let Some(value) = text.strip_prefix("from ") {
        value
            .split_whitespace()
            .next()
            .unwrap_or("")
            .replace('.', "/")
    } else if text.contains("::") {
        text.trim_end_matches(';').replace("::", "/")
    } else if text.ends_with(".rs")
        || text.ends_with(".py")
        || text.ends_with(".ts")
        || text.ends_with(".js")
        || text.ends_with(".c")
        || text.ends_with(".h")
        || text.ends_with(".cpp")
        || text.ends_with(".zig")
        || text.ends_with(".php")
        || text.ends_with(".rb")
        || text.ends_with(".swift")
    {
        text.to_string()
    } else if text.contains('.') {
        text.trim_end_matches(';').replace('.', "/")
    } else {
        text.to_string()
    };
    if candidate.is_empty() {
        return None;
    }

    let mut matches = entries
        .iter()
        .filter(|entry| {
            let stem = entry
                .path
                .strip_suffix(".rs")
                .or_else(|| entry.path.strip_suffix(".py"))
                .or_else(|| entry.path.strip_suffix(".ts"))
                .or_else(|| entry.path.strip_suffix(".tsx"))
                .or_else(|| entry.path.strip_suffix(".mts"))
                .or_else(|| entry.path.strip_suffix(".cts"))
                .or_else(|| entry.path.strip_suffix(".js"))
                .or_else(|| entry.path.strip_suffix(".mjs"))
                .or_else(|| entry.path.strip_suffix(".cjs"))
                .or_else(|| entry.path.strip_suffix(".jsx"))
                .or_else(|| entry.path.strip_suffix(".go"))
                .or_else(|| entry.path.strip_suffix(".cpp"))
                .or_else(|| entry.path.strip_suffix(".cc"))
                .or_else(|| entry.path.strip_suffix(".cxx"))
                .or_else(|| entry.path.strip_suffix(".hpp"))
                .or_else(|| entry.path.strip_suffix(".hxx"))
                .or_else(|| entry.path.strip_suffix(".hh"))
                .or_else(|| entry.path.strip_suffix(".c"))
                .or_else(|| entry.path.strip_suffix(".h"))
                .or_else(|| entry.path.strip_suffix(".java"))
                .or_else(|| entry.path.strip_suffix(".cs"))
                .or_else(|| entry.path.strip_suffix(".kt"))
                .or_else(|| entry.path.strip_suffix(".kts"))
                .or_else(|| entry.path.strip_suffix(".zig"))
                .or_else(|| entry.path.strip_suffix(".php"))
                .or_else(|| entry.path.strip_suffix(".rb"))
                .or_else(|| entry.path.strip_suffix(".swift"))
                .unwrap_or(&entry.path);
            candidate == *stem
                || candidate == entry.path
                || candidate.ends_with(&format!("/{stem}"))
                || candidate.ends_with(&format!("/{}", entry.path))
                || format!("{candidate}.java") == entry.path
                || format!("{candidate}.kt") == entry.path
                || format!("{candidate}.kts") == entry.path
                || format!("{candidate}.cs") == entry.path
                || format!("{candidate}.zig") == entry.path
                || format!("{candidate}.php") == entry.path
                || format!("{candidate}.rb") == entry.path
                || format!("{candidate}.swift") == entry.path
                || candidate == format!("{stem}.rs")
                || candidate == format!("{stem}.py")
                || candidate == format!("{stem}.ts")
                || candidate == format!("{stem}.tsx")
                || candidate == format!("{stem}.mts")
                || candidate == format!("{stem}.cts")
                || candidate == format!("{stem}.js")
                || candidate == format!("{stem}.mjs")
                || candidate == format!("{stem}.cjs")
                || candidate == format!("{stem}.jsx")
                || candidate == format!("{stem}.go")
                || candidate == format!("{stem}.c")
                || candidate == format!("{stem}.h")
                || candidate == format!("{stem}.cpp")
                || candidate == format!("{stem}.cc")
                || candidate == format!("{stem}.cxx")
                || candidate == format!("{stem}.hpp")
                || candidate == format!("{stem}.hxx")
                || candidate == format!("{stem}.hh")
                || candidate == format!("{stem}.java")
                || candidate == format!("{stem}.cs")
                || candidate == format!("{stem}.kt")
                || candidate == format!("{stem}.zig")
                || candidate == format!("{stem}.php")
                || candidate == format!("{stem}.rb")
                || candidate == format!("{stem}.swift")
                || candidate == format!("{stem}.kts")
                || candidate == format!("{stem}/mod")
                || candidate == format!("{stem}/index")
        })
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    matches.sort();
    (matches.len() == 1).then(|| matches.remove(0))
}

fn normalize_import_path(path: &str) -> String {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            value => parts.push(value),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Language, SourceLocation};
    use crate::parser::{Call, ParsedFile, Symbol};

    fn loc(file: &str) -> SourceLocation {
        SourceLocation {
            file: file.to_string(),
            start_line: 1,
            start_col: 1,
            end_line: 1,
            end_col: 1,
        }
    }

    #[test]
    fn deterministic_ids() {
        let pf1 = ParsedFile {
            symbols: vec![Symbol {
                kind: NodeKind::Function,
                name: "foo".to_string(),
                qualified_name: "a.rs::foo".to_string(),
                location: loc("a.rs"),
                parent: None,
                visibility: crate::model::Visibility::Public,
                signature: None,
                container: None,
            }],
            imports: vec![],
            calls: vec![],
            definitions: vec![],
            references: vec![],
            exports: vec![],
            type_references: vec![],
        };
        let pf2 = ParsedFile {
            symbols: vec![Symbol {
                kind: NodeKind::Function,
                name: "bar".to_string(),
                qualified_name: "b.rs::bar".to_string(),
                location: loc("b.rs"),
                parent: None,
                visibility: crate::model::Visibility::Public,
                signature: None,
                container: None,
            }],
            imports: vec![],
            calls: vec![],
            definitions: vec![],
            references: vec![],
            exports: vec![],
            type_references: vec![],
        };
        let g1 = build_graph(vec![
            ("a.rs".to_string(), Some(Language::Rust), pf1.clone()),
            ("b.rs".to_string(), Some(Language::Rust), pf2.clone()),
        ]);
        let g2 = build_graph(vec![
            ("b.rs".to_string(), Some(Language::Rust), pf2),
            ("a.rs".to_string(), Some(Language::Rust), pf1),
        ]);
        assert_eq!(g1, g2);
        g1.validate().expect("valid graph");
    }

    #[test]
    fn calls_only_unique_or_same_file() {
        let pf_a = ParsedFile {
            symbols: vec![Symbol {
                kind: NodeKind::Function,
                name: "caller".to_string(),
                qualified_name: "a.rs::caller".to_string(),
                location: loc("a.rs"),
                parent: None,
                visibility: crate::model::Visibility::Public,
                signature: None,
                container: None,
            }],
            imports: vec![],
            calls: vec![Call {
                caller: "a.rs::caller".to_string(),
                callee: "dup".to_string(),
                location: loc("a.rs"),
            }],
            definitions: vec![],
            references: vec![],
            exports: vec![],
            type_references: vec![],
        };
        let pf_b = ParsedFile {
            symbols: vec![Symbol {
                kind: NodeKind::Function,
                name: "dup".to_string(),
                qualified_name: "b.rs::dup".to_string(),
                location: loc("b.rs"),
                parent: None,
                visibility: crate::model::Visibility::Public,
                signature: None,
                container: None,
            }],
            imports: vec![],
            calls: vec![],
            definitions: vec![],
            references: vec![],
            exports: vec![],
            type_references: vec![],
        };
        let pf_c = ParsedFile {
            symbols: vec![Symbol {
                kind: NodeKind::Function,
                name: "dup".to_string(),
                qualified_name: "c.rs::dup".to_string(),
                location: loc("c.rs"),
                parent: None,
                visibility: crate::model::Visibility::Public,
                signature: None,
                container: None,
            }],
            imports: vec![],
            calls: vec![],
            definitions: vec![],
            references: vec![],
            exports: vec![],
            type_references: vec![],
        };
        let g = build_graph(vec![
            ("a.rs".to_string(), Some(Language::Rust), pf_a),
            ("b.rs".to_string(), Some(Language::Rust), pf_b),
            ("c.rs".to_string(), Some(Language::Rust), pf_c),
        ]);
        assert!(g.edges.iter().all(|edge| edge.kind != EdgeKind::Calls));
    }

    #[test]
    fn unique_unimported_target_is_not_resolved() {
        let caller = ParsedFile {
            symbols: vec![Symbol {
                kind: NodeKind::Function,
                name: "caller".to_string(),
                qualified_name: "a.rs::caller".to_string(),
                location: loc("a.rs"),
                parent: None,
                visibility: crate::model::Visibility::Public,
                signature: None,
                container: None,
            }],
            imports: vec![],
            calls: vec![Call {
                caller: "a.rs::caller".to_string(),
                callee: "target".to_string(),
                location: loc("a.rs"),
            }],
            definitions: vec![],
            references: vec![],
            exports: vec![],
            type_references: vec![],
        };
        let target = ParsedFile {
            symbols: vec![Symbol {
                kind: NodeKind::Function,
                name: "target".to_string(),
                qualified_name: "b.rs::target".to_string(),
                location: loc("b.rs"),
                parent: None,
                visibility: crate::model::Visibility::Public,
                signature: None,
                container: None,
            }],
            imports: vec![],
            calls: vec![],
            definitions: vec![],
            references: vec![],
            exports: vec![],
            type_references: vec![],
        };
        let graph = build_graph(vec![
            ("a.rs".to_string(), Some(Language::Rust), caller),
            ("b.rs".to_string(), Some(Language::Rust), target),
        ]);
        assert!(graph.edges.iter().all(|edge| edge.kind != EdgeKind::Calls));
    }

    #[test]
    fn initial_calls_edge_points_to_target_symbol() {
        let caller = ParsedFile {
            symbols: vec![Symbol {
                kind: NodeKind::Function,
                name: "caller".to_string(),
                qualified_name: "a.rs::caller".to_string(),
                location: loc("a.rs"),
                parent: None,
                visibility: crate::model::Visibility::Public,
                signature: None,
                container: None,
            }],
            imports: vec![crate::parser::Import {
                path: "b.rs".to_string(),
                location: loc("a.rs"),
            }],
            calls: vec![Call {
                caller: "a.rs::caller".to_string(),
                callee: "target".to_string(),
                location: loc("a.rs"),
            }],
            definitions: vec![],
            references: vec![],
            exports: vec![],
            type_references: vec![],
        };
        let target = ParsedFile {
            symbols: vec![Symbol {
                kind: NodeKind::Function,
                name: "target".to_string(),
                qualified_name: "b.rs::target".to_string(),
                location: loc("b.rs"),
                parent: None,
                visibility: crate::model::Visibility::Public,
                signature: None,
                container: None,
            }],
            imports: vec![],
            calls: vec![],
            definitions: vec![],
            references: vec![],
            exports: vec![],
            type_references: vec![],
        };
        let graph = build_graph(vec![
            ("a.rs".to_string(), Some(Language::Rust), caller),
            ("b.rs".to_string(), Some(Language::Rust), target),
        ]);
        let caller_id = graph
            .nodes
            .iter()
            .find(|node| node.qualified_name == "a.rs::caller")
            .unwrap()
            .id;
        let target_id = graph
            .nodes
            .iter()
            .find(|node| node.qualified_name == "b.rs::target")
            .unwrap()
            .id;
        assert!(graph.edges.iter().any(|edge| edge.kind == EdgeKind::Calls
            && edge.from == caller_id
            && edge.to == target_id));
    }
}
