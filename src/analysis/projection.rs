use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::graph::Graph;
use crate::model::{EdgeKind, NodeKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnalysisLevel {
    Symbol,
    File,
    Module,
}

impl AnalysisLevel {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Symbol => "symbol",
            Self::File => "file",
            Self::Module => "module",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectedNode {
    pub id: String,
    pub name: String,
    pub level: AnalysisLevel,
    pub member_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectedEdge {
    pub from: String,
    pub to: String,
    pub weight: usize,
    pub kinds: Vec<EdgeKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectedGraph {
    pub level: AnalysisLevel,
    pub nodes: Vec<ProjectedNode>,
    pub edges: Vec<ProjectedEdge>,
}

#[derive(Debug, Clone)]
pub struct AdjacencyGraph {
    pub level: AnalysisLevel,
    pub nodes: Vec<ProjectedNode>,
    pub node_indices: HashMap<String, usize>,
    pub outgoing: Vec<Vec<(usize, usize)>>,
    pub incoming: Vec<Vec<(usize, usize)>>,
    pub edge_kinds: HashMap<(usize, usize), Vec<EdgeKind>>,
}

impl AdjacencyGraph {
    #[must_use]
    pub fn has_self_loop(&self, node_idx: usize) -> bool {
        self.outgoing[node_idx]
            .iter()
            .any(|(target, _)| *target == node_idx)
    }

    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.outgoing.iter().map(Vec::len).sum()
    }
}

#[must_use]
pub fn file_to_module(file_path: &str) -> String {
    let normalized = file_path.replace('\\', "/");
    let path = Path::new(&normalized);
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_string_lossy().replace('\\', "/"),
        _ => ".".to_string(),
    }
}

#[must_use]
pub fn project_graph(
    graph: &Graph,
    level: AnalysisLevel,
    edge_filter: Option<EdgeKind>,
) -> ProjectedGraph {
    match level {
        AnalysisLevel::Symbol => project_symbols(graph, edge_filter),
        AnalysisLevel::File => project_files(graph, edge_filter),
        AnalysisLevel::Module => project_modules(graph, edge_filter),
    }
}

fn project_symbols(graph: &Graph, edge_filter: Option<EdgeKind>) -> ProjectedGraph {
    let mut symbol_nodes: Vec<_> = graph
        .nodes
        .iter()
        .filter(|n| n.kind != NodeKind::File)
        .collect();

    // If graph contains only File nodes (e.g. synthetic graph), fallback to all nodes
    if symbol_nodes.is_empty() {
        symbol_nodes = graph.nodes.iter().collect();
    }

    let mut nodes: Vec<ProjectedNode> = symbol_nodes
        .iter()
        .map(|node| ProjectedNode {
            id: node.qualified_name.clone(),
            name: node.name.clone(),
            level: AnalysisLevel::Symbol,
            member_count: 1,
        })
        .collect();

    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    nodes.dedup_by(|a, b| a.id == b.id);

    let id_to_name: HashMap<_, _> = graph
        .nodes
        .iter()
        .map(|n| (n.id, n.qualified_name.clone()))
        .collect();

    let mut edge_map: BTreeMap<(String, String), (usize, BTreeSet<EdgeKind>)> = BTreeMap::new();

    for edge in &graph.edges {
        if edge.kind == EdgeKind::Contains && edge_filter != Some(EdgeKind::Contains) {
            continue;
        }
        if let Some(filter) = edge_filter
            && edge.kind != filter
        {
            continue;
        }
        if let (Some(from_name), Some(to_name)) =
            (id_to_name.get(&edge.from), id_to_name.get(&edge.to))
        {
            let entry = edge_map
                .entry((from_name.clone(), to_name.clone()))
                .or_insert_with(|| (0, BTreeSet::new()));
            entry.0 += 1;
            entry.1.insert(edge.kind);
        }
    }

    let mut edges: Vec<ProjectedEdge> = edge_map
        .into_iter()
        .map(|((from, to), (weight, kinds))| ProjectedEdge {
            from,
            to,
            weight,
            kinds: kinds.into_iter().collect(),
        })
        .collect();

    edges.sort_by(|a, b| a.from.cmp(&b.from).then_with(|| a.to.cmp(&b.to)));

    ProjectedGraph {
        level: AnalysisLevel::Symbol,
        nodes,
        edges,
    }
}

fn project_files(graph: &Graph, edge_filter: Option<EdgeKind>) -> ProjectedGraph {
    let mut file_members: BTreeMap<String, usize> = BTreeMap::new();
    let mut node_to_file: HashMap<_, _> = HashMap::new();

    for node in &graph.nodes {
        let file = if node.file.is_empty() {
            node.name.clone()
        } else {
            node.file.clone()
        };
        *file_members.entry(file.clone()).or_insert(0) += 1;
        node_to_file.insert(node.id, file);
    }

    let mut nodes: Vec<ProjectedNode> = file_members
        .into_iter()
        .map(|(file, count)| ProjectedNode {
            id: file.clone(),
            name: file,
            level: AnalysisLevel::File,
            member_count: count,
        })
        .collect();

    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    let mut edge_map: BTreeMap<(String, String), (usize, BTreeSet<EdgeKind>)> = BTreeMap::new();

    for edge in &graph.edges {
        if edge.kind == EdgeKind::Contains && edge_filter != Some(EdgeKind::Contains) {
            continue;
        }
        if let Some(filter) = edge_filter
            && edge.kind != filter
        {
            continue;
        }
        if let (Some(from_file), Some(to_file)) =
            (node_to_file.get(&edge.from), node_to_file.get(&edge.to))
        {
            let entry = edge_map
                .entry((from_file.clone(), to_file.clone()))
                .or_insert_with(|| (0, BTreeSet::new()));
            entry.0 += 1;
            entry.1.insert(edge.kind);
        }
    }

    let mut edges: Vec<ProjectedEdge> = edge_map
        .into_iter()
        .map(|((from, to), (weight, kinds))| ProjectedEdge {
            from,
            to,
            weight,
            kinds: kinds.into_iter().collect(),
        })
        .collect();

    edges.sort_by(|a, b| a.from.cmp(&b.from).then_with(|| a.to.cmp(&b.to)));

    ProjectedGraph {
        level: AnalysisLevel::File,
        nodes,
        edges,
    }
}

fn project_modules(graph: &Graph, edge_filter: Option<EdgeKind>) -> ProjectedGraph {
    let mut module_members: BTreeMap<String, usize> = BTreeMap::new();
    let mut node_to_module: HashMap<_, _> = HashMap::new();

    for node in &graph.nodes {
        let file = if node.file.is_empty() {
            &node.name
        } else {
            &node.file
        };
        let module = file_to_module(file);
        *module_members.entry(module.clone()).or_insert(0) += 1;
        node_to_module.insert(node.id, module);
    }

    let mut nodes: Vec<ProjectedNode> = module_members
        .into_iter()
        .map(|(module, count)| ProjectedNode {
            id: module.clone(),
            name: module,
            level: AnalysisLevel::Module,
            member_count: count,
        })
        .collect();

    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    let mut edge_map: BTreeMap<(String, String), (usize, BTreeSet<EdgeKind>)> = BTreeMap::new();

    for edge in &graph.edges {
        if edge.kind == EdgeKind::Contains && edge_filter != Some(EdgeKind::Contains) {
            continue;
        }
        if let Some(filter) = edge_filter
            && edge.kind != filter
        {
            continue;
        }
        if let (Some(from_mod), Some(to_mod)) =
            (node_to_module.get(&edge.from), node_to_module.get(&edge.to))
        {
            let entry = edge_map
                .entry((from_mod.clone(), to_mod.clone()))
                .or_insert_with(|| (0, BTreeSet::new()));
            entry.0 += 1;
            entry.1.insert(edge.kind);
        }
    }

    let mut edges: Vec<ProjectedEdge> = edge_map
        .into_iter()
        .map(|((from, to), (weight, kinds))| ProjectedEdge {
            from,
            to,
            weight,
            kinds: kinds.into_iter().collect(),
        })
        .collect();

    edges.sort_by(|a, b| a.from.cmp(&b.from).then_with(|| a.to.cmp(&b.to)));

    ProjectedGraph {
        level: AnalysisLevel::Module,
        nodes,
        edges,
    }
}

impl ProjectedGraph {
    #[must_use]
    pub fn to_adjacency(&self) -> AdjacencyGraph {
        let node_indices: HashMap<String, usize> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.clone(), i))
            .collect();

        let n = self.nodes.len();
        let mut outgoing = vec![Vec::new(); n];
        let mut incoming = vec![Vec::new(); n];
        let mut edge_kinds = HashMap::new();

        for edge in &self.edges {
            if let (Some(&u), Some(&v)) = (node_indices.get(&edge.from), node_indices.get(&edge.to))
            {
                outgoing[u].push((v, edge.weight));
                incoming[v].push((u, edge.weight));
                edge_kinds.insert((u, v), edge.kinds.clone());
            }
        }

        for list in outgoing.iter_mut().chain(incoming.iter_mut()) {
            list.sort_by_key(|&(target, _)| target);
        }

        AdjacencyGraph {
            level: self.level,
            nodes: self.nodes.clone(),
            node_indices,
            outgoing,
            incoming,
            edge_kinds,
        }
    }
}
