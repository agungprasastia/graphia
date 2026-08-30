use std::collections::{BTreeMap, HashSet};

use sha2::{Digest, Sha256};

use crate::model::{
    Confidence, Edge, EdgeId, EdgeIdentity, EdgeKind, Language, Node, NodeId, NodeIdentity,
    NodeKind, SourceLocation,
};
use crate::parser::{Call, ParsedFile, Symbol};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    resolution: ResolutionReport,
    resolution_calls: Vec<(String, Call)>,
}

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
                node.location.file.as_str(),
                node.location.start_line,
                node.location.start_col,
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
        });
        let imports: Vec<(NodeId, NodeId)> = self
            .edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Imports)
            .map(|edge| (edge.from, edge.to))
            .collect();
        let mut report = ResolutionReport::default();
        let mut resolved = Vec::new();
        for (file, call) in &self.resolution_calls {
            let Some(caller) = self
                .nodes
                .iter()
                .find(|node| node.qualified_name == call.caller)
            else {
                report.unresolved_calls += 1;
                continue;
            };
            let imported_files: Vec<NodeId> = imports
                .iter()
                .filter_map(|(from, to)| (*from == self.file_node_id(file)).then_some(*to))
                .collect();
            let candidates: Vec<&Node> = self
                .nodes
                .iter()
                .filter(|node| {
                    node.name == call.callee
                        && node.kind != NodeKind::File
                        && imported_files.contains(&self.file_node_id(&node.file))
                })
                .collect();
            if candidates.len() != 1 {
                if candidates.len() > 1 {
                    report.ambiguous_calls += 1;
                } else {
                    report.unresolved_calls += 1;
                }
                continue;
            }
            let target = candidates[0];
            let identity = EdgeIdentity::new(
                caller.id,
                target.id,
                EdgeKind::Calls,
                Confidence::Inferred,
                None,
            );
            resolved.push(Edge {
                id: stable_edge_id(&identity),
                kind: EdgeKind::Calls,
                from: caller.id,
                to: target.id,
                confidence: Confidence::Inferred,
                label: None,
            });
            report.resolved_calls += 1;
        }
        self.edges.extend(resolved);
        self.resolution = report;
        self.canonicalize()?;
        Ok(self.resolution.clone())
    }

    fn file_node_id(&self, file: &str) -> NodeId {
        self.nodes
            .iter()
            .find(|node| node.kind == NodeKind::File && node.file == file)
            .map_or(NodeId(0), |node| node.id)
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
        "node\0{}\0{}\0{}\0{}:{}:{}:{}:{}",
        identity.file,
        identity.kind.code(),
        identity.qualified_name,
        identity.location.file,
        identity.location.start_line,
        identity.location.start_col,
        identity.location.end_line,
        identity.location.end_col,
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
            .then(a.1.location.cmp(&b.1.location))
    });
    all_symbols.dedup_by(|a, b| {
        a.1.kind == b.1.kind
            && a.1.qualified_name == b.1.qualified_name
            && a.1.location == b.1.location
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
        let location = SourceLocation {
            file: entry.path.clone(),
            start_line: 1,
            start_col: 1,
            end_line: 1,
            end_col: 1,
        };
        let id = stable_node_id(&NodeIdentity::new(
            &entry.path,
            NodeKind::File,
            &entry.path,
            &location,
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
        });
    }

    for (file, symbol) in &all_symbols {
        let id = stable_node_id(&NodeIdentity::new(
            file,
            symbol.kind,
            &symbol.qualified_name,
            &symbol.location,
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
            language: None,
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
                    Some(Language::TypeScript) => file.ends_with(".ts") || file.ends_with(".tsx"),
                    None => true,
                })
                .collect();
            let same_file: Vec<_> = candidates
                .iter()
                .filter(|(_, _, file)| file == &entry.path)
                .collect();
            let candidate = if same_file.len() == 1 {
                Some(same_file[0])
            } else if language_candidates.len() == 1 {
                Some(language_candidates[0])
            } else {
                if same_file.is_empty() && language_candidates.len() > 1 {
                    resolution.ambiguous_calls += 1;
                } else {
                    resolution.unresolved_calls += 1;
                }
                None
            };
            if let Some((_, _, callee_file)) = candidate
                && let Some(&callee_id) = file_node_ids.get(callee_file)
                && caller_id != callee_id
            {
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
    } else if text.contains("::") {
        text.trim_end_matches(';').replace("::", "/")
    } else {
        text.strip_prefix("from ").map_or_else(
            || text.to_string(),
            |value| {
                value
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .replace('.', "/")
            },
        )
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
                .unwrap_or(&entry.path);
            candidate == *stem
                || candidate == entry.path
                || candidate == format!("{stem}.rs")
                || candidate == format!("{stem}.py")
                || candidate == format!("{stem}.ts")
                || candidate == format!("{stem}.tsx")
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
            }],
            imports: vec![],
            calls: vec![],
        };
        let pf2 = ParsedFile {
            symbols: vec![Symbol {
                kind: NodeKind::Function,
                name: "bar".to_string(),
                qualified_name: "b.rs::bar".to_string(),
                location: loc("b.rs"),
                parent: None,
            }],
            imports: vec![],
            calls: vec![],
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
            }],
            imports: vec![],
            calls: vec![Call {
                caller: "a.rs::caller".to_string(),
                callee: "dup".to_string(),
                location: loc("a.rs"),
            }],
        };
        let pf_b = ParsedFile {
            symbols: vec![Symbol {
                kind: NodeKind::Function,
                name: "dup".to_string(),
                qualified_name: "b.rs::dup".to_string(),
                location: loc("b.rs"),
                parent: None,
            }],
            imports: vec![],
            calls: vec![],
        };
        let pf_c = ParsedFile {
            symbols: vec![Symbol {
                kind: NodeKind::Function,
                name: "dup".to_string(),
                qualified_name: "c.rs::dup".to_string(),
                location: loc("c.rs"),
                parent: None,
            }],
            imports: vec![],
            calls: vec![],
        };
        let g = build_graph(vec![
            ("a.rs".to_string(), Some(Language::Rust), pf_a),
            ("b.rs".to_string(), Some(Language::Rust), pf_b),
            ("c.rs".to_string(), Some(Language::Rust), pf_c),
        ]);
        assert!(g.edges.iter().all(|edge| edge.kind != EdgeKind::Calls));
    }
}
