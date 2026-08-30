use std::collections::{BTreeMap, HashMap, VecDeque};

use crate::graph::Graph;
use crate::model::{EdgeKind, Node, NodeId, NodeKind};

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

#[derive(Debug, Clone)]
pub struct QueryIndex {
    names: HashMap<String, Vec<NodeId>>,
    qualified_names: HashMap<String, Vec<NodeId>>,
    files: HashMap<String, Vec<NodeId>>,
    outgoing: Vec<Vec<NodeId>>,
    incoming: Vec<Vec<NodeId>>,
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

        let mut outgoing = vec![Vec::new(); graph.nodes.len()];
        let mut incoming = vec![Vec::new(); graph.nodes.len()];
        for edge in &graph.edges {
            if let (Some(out), Some(inc)) = (
                outgoing.get_mut(edge.from.0 as usize),
                incoming.get_mut(edge.to.0 as usize),
            ) {
                out.push(edge.to);
                inc.push(edge.from);
            }
        }
        for values in outgoing.iter_mut().chain(incoming.iter_mut()) {
            values.sort();
            values.dedup();
        }
        Self {
            names,
            qualified_names,
            files,
            outgoing,
            incoming,
        }
    }

    #[must_use]
    pub fn find<'a>(&self, graph: &'a Graph, term: &str) -> Vec<&'a Node> {
        let ids = self
            .qualified_names
            .get(term)
            .or_else(|| self.names.get(term))
            .or_else(|| self.files.get(term));
        if let Some(ids) = ids {
            return ids
                .iter()
                .filter_map(|id| graph.nodes.iter().find(|node| node.id == *id))
                .collect();
        }
        graph
            .nodes
            .iter()
            .filter(|node| {
                node.name.contains(term)
                    || node.qualified_name.contains(term)
                    || node.file.contains(term)
            })
            .collect()
    }

    #[must_use]
    pub fn outgoing(&self, id: NodeId) -> &[NodeId] {
        self.outgoing.get(id.0 as usize).map_or(&[], Vec::as_slice)
    }

    #[must_use]
    pub fn incoming(&self, id: NodeId) -> &[NodeId] {
        self.incoming.get(id.0 as usize).map_or(&[], Vec::as_slice)
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
        max_depth: usize,
        max_visited: usize,
    ) -> Result<Option<Vec<NodeId>>, TraversalError> {
        if from.0 as usize >= self.outgoing.len() || to.0 as usize >= self.outgoing.len() {
            return Ok(None);
        }
        let mut queue = VecDeque::from([(from, 0usize)]);
        let mut previous = vec![None; self.outgoing.len()];
        let mut seen = vec![false; self.outgoing.len()];
        seen[from.0 as usize] = true;
        let mut visited = 0;
        while let Some((current, depth)) = queue.pop_front() {
            visited += 1;
            if visited > max_visited {
                return Err(TraversalError {
                    visited,
                    limit: max_visited,
                });
            }
            if current == to {
                let mut path = Vec::new();
                let mut cursor = Some(current);
                while let Some(id) = cursor {
                    path.push(id);
                    cursor = previous[id.0 as usize];
                }
                path.reverse();
                return Ok(Some(path));
            }
            if depth >= max_depth {
                continue;
            }
            for next in self.outgoing(current) {
                let index = next.0 as usize;
                if !seen[index] {
                    seen[index] = true;
                    previous[index] = Some(current);
                    queue.push_back((*next, depth + 1));
                }
            }
        }
        Ok(None)
    }

    #[must_use]
    pub fn explain(&self, graph: &Graph, node: &Node) -> String {
        let parent = graph
            .edges
            .iter()
            .find(|edge| edge.kind == EdgeKind::Contains && edge.to == node.id)
            .and_then(|edge| {
                graph
                    .nodes
                    .iter()
                    .find(|candidate| candidate.id == edge.from)
            })
            .map_or_else(|| "-".to_string(), |parent| parent.qualified_name.clone());
        let format_nodes = |ids: &[NodeId]| {
            ids.iter()
                .filter_map(|id| graph.nodes.iter().find(|candidate| candidate.id == *id))
                .map(|related| related.qualified_name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        format!(
            "{} {}\nlocation: {}:{}:{}\nparent: {}\nincoming: {}\noutgoing: {}",
            node.kind.as_str(),
            node.qualified_name,
            node.location.file,
            node.location.start_line,
            node.location.start_col,
            parent,
            format_nodes(self.incoming(node.id)),
            format_nodes(self.outgoing(node.id)),
        )
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
    use crate::model::{Confidence, Edge, EdgeId, EdgeKind, SourceLocation};

    fn node(id: u64, name: &str) -> Node {
        Node {
            id: NodeId(id),
            kind: NodeKind::Function,
            name: name.to_string(),
            qualified_name: format!("a.rs::{name}"),
            file: "a.rs".to_string(),
            location: SourceLocation {
                file: "a.rs".to_string(),
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 1,
            },
            language: None,
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
            index.shortest_path(NodeId(0), NodeId(2), 10, 10),
            Ok(Some(vec![NodeId(0), NodeId(1), NodeId(2)]))
        );
        assert!(
            index
                .shortest_path(NodeId(0), NodeId(2), 1, 10)
                .unwrap()
                .is_none()
        );
    }
}
