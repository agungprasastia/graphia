use std::collections::{BTreeMap, HashMap, VecDeque};

use crate::error::{GraphiaError, Result};
use crate::graph::Graph;
use crate::model::{EdgeId, EdgeKind, Node, NodeId, NodeKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryMatch {
    pub node_id: NodeId,
    pub kind: NodeKind,
    pub name: String,
    pub qualified_name: String,
    pub file: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraversalError {
    pub visited: usize,
    pub limit: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraversalLimits {
    pub max_depth: usize,
    pub max_visited: usize,
}

impl TraversalLimits {
    #[must_use]
    pub const fn new(max_depth: usize, max_visited: usize) -> Self {
        Self {
            max_depth,
            max_visited,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explanation {
    pub node: NodeId,
    pub kind: NodeKind,
    pub location: String,
    pub parent: Option<NodeId>,
    pub incoming: Vec<NodeId>,
    pub outgoing: Vec<NodeId>,
    pub callers: Vec<NodeId>,
    pub callees: Vec<NodeId>,
    pub imports: Vec<NodeId>,
}

#[derive(Debug, Clone)]
pub struct QueryIndex {
    names: HashMap<String, Vec<NodeId>>,
    qualified_names: HashMap<String, Vec<NodeId>>,
    files: HashMap<String, Vec<NodeId>>,
    slots: HashMap<NodeId, usize>,
    outgoing: Vec<Vec<NodeId>>,
    incoming: Vec<Vec<NodeId>>,
    outgoing_edges: Vec<Vec<(NodeId, EdgeId, EdgeKind)>>,
    incoming_edges: Vec<Vec<(NodeId, EdgeId, EdgeKind)>>,
}

impl QueryIndex {
    #[must_use]
    pub fn new(graph: &Graph) -> Self {
        let mut names: HashMap<String, Vec<NodeId>> = HashMap::new();
        let mut qualified_names: HashMap<String, Vec<NodeId>> = HashMap::new();
        let mut files: HashMap<String, Vec<NodeId>> = HashMap::new();
        for node in &graph.nodes {
            names.entry(node.name.clone()).or_default().push(node.id);
            qualified_names
                .entry(node.qualified_name.clone())
                .or_default()
                .push(node.id);
            files.entry(node.file.clone()).or_default().push(node.id);
        }
        for values in names
            .values_mut()
            .chain(qualified_names.values_mut())
            .chain(files.values_mut())
        {
            values.sort();
        }

        let slots: HashMap<_, _> = graph
            .nodes
            .iter()
            .enumerate()
            .map(|(slot, node)| (node.id, slot))
            .collect();
        let mut outgoing = vec![Vec::new(); graph.nodes.len()];
        let mut incoming = vec![Vec::new(); graph.nodes.len()];
        let mut outgoing_edges = vec![Vec::new(); graph.nodes.len()];
        let mut incoming_edges = vec![Vec::new(); graph.nodes.len()];
        for edge in &graph.edges {
            if let (Some(&from), Some(&to)) = (slots.get(&edge.from), slots.get(&edge.to)) {
                outgoing[from].push(edge.to);
                incoming[to].push(edge.from);
                outgoing_edges[from].push((edge.to, edge.id, edge.kind));
                incoming_edges[to].push((edge.from, edge.id, edge.kind));
            }
        }
        for values in outgoing.iter_mut().chain(incoming.iter_mut()) {
            values.sort();
            values.dedup();
        }
        for values in outgoing_edges.iter_mut().chain(incoming_edges.iter_mut()) {
            values.sort_by_key(|(node, edge, kind)| (*node, *edge, *kind));
            values.dedup();
        }
        Self {
            names,
            qualified_names,
            files,
            slots,
            outgoing,
            incoming,
            outgoing_edges,
            incoming_edges,
        }
    }

    #[must_use]
    pub fn find<'a>(&self, graph: &'a Graph, term: &str) -> Vec<&'a Node> {
        if term.is_empty() {
            return Vec::new();
        }
        let mut exact_ids = self
            .qualified_names
            .get(term)
            .into_iter()
            .chain(self.names.get(term))
            .chain(self.files.get(term))
            .flat_map(|ids| ids.iter().copied())
            .collect::<Vec<_>>();
        exact_ids.sort();
        exact_ids.dedup();
        if !exact_ids.is_empty() {
            return exact_ids
                .iter()
                .filter_map(|id| graph.nodes.iter().find(|node| node.id == *id))
                .collect();
        }
        graph
            .nodes
            .iter()
            .filter(|node| node.name.contains(term) || node.qualified_name.contains(term))
            .collect()
    }

    #[must_use]
    pub fn outgoing(&self, id: NodeId) -> &[NodeId] {
        self.slots
            .get(&id)
            .and_then(|slot| self.outgoing.get(*slot))
            .map_or(&[], Vec::as_slice)
    }

    #[must_use]
    pub fn incoming(&self, id: NodeId) -> &[NodeId] {
        self.slots
            .get(&id)
            .and_then(|slot| self.incoming.get(*slot))
            .map_or(&[], Vec::as_slice)
    }

    fn outgoing_edges(&self, id: NodeId) -> &[(NodeId, EdgeId, EdgeKind)] {
        self.slots
            .get(&id)
            .and_then(|slot| self.outgoing_edges.get(*slot))
            .map_or(&[], Vec::as_slice)
    }

    fn incoming_edges(&self, id: NodeId) -> &[(NodeId, EdgeId, EdgeKind)] {
        self.slots
            .get(&id)
            .and_then(|slot| self.incoming_edges.get(*slot))
            .map_or(&[], Vec::as_slice)
    }

    #[must_use]
    pub fn related<'a>(&self, graph: &'a Graph, id: NodeId, incoming: bool) -> Vec<&'a Node> {
        let ids = if incoming {
            self.incoming(id)
        } else {
            self.outgoing(id)
        };
        ids.iter()
            .filter_map(|related_id| graph.nodes.iter().find(|node| node.id == *related_id))
            .collect()
    }

    pub fn shortest_path(
        &self,
        from: NodeId,
        to: NodeId,
        limits: TraversalLimits,
    ) -> std::result::Result<Option<Vec<EdgeId>>, TraversalError> {
        let Some(&from_slot) = self.slots.get(&from) else {
            return Ok(None);
        };
        if !self.slots.contains_key(&to) {
            return Ok(None);
        }
        let mut queue = VecDeque::from([(from, 0usize)]);
        let mut previous: Vec<Option<(NodeId, EdgeId)>> = vec![None; self.outgoing.len()];
        let mut seen = vec![false; self.outgoing.len()];
        seen[from_slot] = true;
        let mut visited = 0;
        while let Some((current, depth)) = queue.pop_front() {
            visited += 1;
            if visited > limits.max_visited {
                return Err(TraversalError {
                    visited,
                    limit: limits.max_visited,
                });
            }
            if current == to {
                let mut path = Vec::new();
                let mut cursor = Some(current);
                while let Some(id) = cursor {
                    let Some((parent, edge)) = previous[self.slots[&id]] else {
                        break;
                    };
                    path.push(edge);
                    cursor = Some(parent);
                }
                path.reverse();
                return Ok(Some(path));
            }
            if depth >= limits.max_depth {
                continue;
            }
            for (next, edge, _) in self.outgoing_edges(current) {
                let index = self.slots[next];
                if !seen[index] {
                    seen[index] = true;
                    previous[index] = Some((current, *edge));
                    queue.push_back((*next, depth + 1));
                }
            }
        }
        Ok(None)
    }

    pub fn explain(&self, graph: &Graph, node: NodeId) -> Result<Explanation> {
        let Some(found) = graph.nodes.iter().find(|candidate| candidate.id == node) else {
            return Err(GraphiaError::InvalidArgument("symbol not found".into()));
        };
        let mut parents = self
            .incoming_edges(node)
            .iter()
            .filter_map(|(id, _, kind)| (*kind == EdgeKind::Contains).then_some(*id))
            .collect::<Vec<_>>();
        parents.sort();
        let parent = parents.first().copied();
        let incoming = self.incoming(node).to_vec();
        let outgoing = self.outgoing(node).to_vec();
        let callers = self
            .incoming_edges(node)
            .iter()
            .filter_map(|(id, _, kind)| (*kind == EdgeKind::Calls).then_some(*id))
            .collect();
        let callees = self
            .outgoing_edges(node)
            .iter()
            .filter_map(|(id, _, kind)| (*kind == EdgeKind::Calls).then_some(*id))
            .collect();
        let imports = self
            .outgoing_edges(node)
            .iter()
            .filter_map(|(id, _, kind)| (*kind == EdgeKind::Imports).then_some(*id))
            .collect();
        Ok(Explanation {
            node,
            kind: found.kind,
            location: format!(
                "{}:{}:{}",
                found.location.file, found.location.start_line, found.location.start_col
            ),
            parent,
            incoming,
            outgoing,
            callers,
            callees,
            imports,
        })
    }

    #[must_use]
    pub fn stats(&self, graph: &Graph) -> BTreeMap<&'static str, usize> {
        let mut stats = BTreeMap::new();
        for node in &graph.nodes {
            *stats.entry(node.kind.as_str()).or_insert(0) += 1;
        }
        stats
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryResult {
    pub matches: Vec<QueryMatch>,
}

impl From<&Node> for QueryMatch {
    fn from(node: &Node) -> Self {
        Self {
            node_id: node.id,
            kind: node.kind,
            name: node.name.clone(),
            qualified_name: node.qualified_name.clone(),
            file: node.file.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Confidence, Edge, EdgeId, EdgeKind};

    fn node(id: u64, name: &str) -> Node {
        Node {
            id: crate::model::NodeId(id),
            kind: crate::model::NodeKind::Function,
            name: name.to_string(),
            qualified_name: name.to_string(),
            file: "test.rs".to_string(),
            location: crate::model::SourceLocation {
                file: "test.rs".to_string(),
                start_line: 1,
                start_col: 1,
                end_line: 5,
                end_col: 1,
            },
            language: Some(crate::model::Language::Rust),
            visibility: crate::model::Visibility::Public,
            signature: None,
            container: None,
        }
    }

    #[test]
    fn exact_and_partial_lookup() {
        let graph = Graph::new(vec![node(0, "foo"), node(1, "foobar")], vec![]);
        let index = QueryIndex::new(&graph);
        assert_eq!(index.find(&graph, "foo").len(), 1);
        assert_eq!(index.find(&graph, "bar").len(), 1);
    }

    #[test]
    fn shortest_path_obeys_cycles_and_limits() {
        let graph = Graph::new(
            vec![node(0, "a"), node(1, "b"), node(2, "c")],
            vec![
                Edge {
                    id: EdgeId(0),
                    kind: EdgeKind::Calls,
                    from: NodeId(0),
                    to: NodeId(1),
                    confidence: Confidence::Extracted,
                    label: None,
                },
                Edge {
                    id: EdgeId(1),
                    kind: EdgeKind::Calls,
                    from: NodeId(1),
                    to: NodeId(0),
                    confidence: Confidence::Extracted,
                    label: None,
                },
                Edge {
                    id: EdgeId(2),
                    kind: EdgeKind::Calls,
                    from: NodeId(1),
                    to: NodeId(2),
                    confidence: Confidence::Extracted,
                    label: None,
                },
            ],
        );
        let index = QueryIndex::new(&graph);
        assert_eq!(
            index.shortest_path(NodeId(0), NodeId(2), TraversalLimits::new(10, 10)),
            Ok(Some(vec![EdgeId(0), EdgeId(2)]))
        );
        assert!(
            index
                .shortest_path(NodeId(0), NodeId(2), TraversalLimits::new(1, 10))
                .unwrap()
                .is_none()
        );
    }
}
