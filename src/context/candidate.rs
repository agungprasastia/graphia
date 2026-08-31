use std::collections::{HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::graph::Graph;
use crate::intelligence::discover_tests;
use crate::model::{EdgeKind, Node, NodeKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CandidateRole {
    Seed,
    Container,
    Caller,
    Callee,
    ReferencedType,
    Implementation,
    Test,
    IndirectNeighbor,
}

impl CandidateRole {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Seed => "Seed",
            Self::Container => "Container",
            Self::Caller => "Caller",
            Self::Callee => "Callee",
            Self::ReferencedType => "ReferencedType",
            Self::Implementation => "Implementation",
            Self::Test => "Test",
            Self::IndirectNeighbor => "IndirectNeighbor",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextCandidate {
    pub node: Node,
    pub role: CandidateRole,
    pub distance: usize,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct ExpansionOptions {
    pub max_depth: usize,
    pub max_candidates: usize,
}

impl Default for ExpansionOptions {
    fn default() -> Self {
        Self {
            max_depth: 3,
            max_candidates: 100,
        }
    }
}

#[must_use]
pub fn expand_candidates(
    graph: &Graph,
    seeds: &[Node],
    options: &ExpansionOptions,
) -> Vec<ContextCandidate> {
    expand_candidates_with_cancel(graph, seeds, options, None)
}

pub fn expand_candidates_with_cancel(
    graph: &Graph,
    seeds: &[Node],
    options: &ExpansionOptions,
    cancelled: Option<&dyn Fn() -> bool>,
) -> Vec<ContextCandidate> {
    if seeds.is_empty() {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    let mut visited = HashSet::new();

    // 1. Add seeds
    for seed in seeds {
        if cancelled.is_some_and(|check| check()) {
            return candidates;
        }
        if visited.insert(seed.id) {
            candidates.push(ContextCandidate {
                node: seed.clone(),
                role: CandidateRole::Seed,
                distance: 0,
                reason: format!("Seed symbol: {}", seed.qualified_name),
            });
        }
    }

    // 2. Discover related tests for seeds
    let test_discovery = discover_tests(graph);
    let mut seed_test_ids = HashSet::new();

    for seed in seeds {
        if cancelled.is_some_and(|check| check()) {
            return candidates;
        }
        for mapping in &test_discovery.mappings {
            if mapping.source_file == seed.file
                || mapping.source_symbol.as_ref() == Some(&seed.qualified_name)
                || mapping.source_symbol.as_ref() == Some(&seed.name)
            {
                for test in &mapping.tests {
                    if let Some(id) = test.test_symbol_id {
                        seed_test_ids.insert((
                            id,
                            seed.qualified_name.clone(),
                            test.reason.clone(),
                        ));
                    } else if let Some(file_node) = graph
                        .nodes
                        .iter()
                        .find(|n| n.file == test.test_file && n.kind == NodeKind::File)
                    {
                        seed_test_ids.insert((
                            file_node.id,
                            seed.qualified_name.clone(),
                            test.reason.clone(),
                        ));
                    }
                }
            }
        }
    }

    for (test_id, seed_name, reason) in seed_test_ids {
        if visited.insert(test_id) {
            if let Some(test_node) = graph.nodes.iter().find(|n| n.id == test_id) {
                candidates.push(ContextCandidate {
                    node: test_node.clone(),
                    role: CandidateRole::Test,
                    distance: 1,
                    reason: format!("Test for {seed_name}: {reason}"),
                });
            }
        }
    }

    // 3. BFS expansion from seeds with depth and role categorization
    // Queue stores: (node_id, depth, path_reason)
    let mut queue = VecDeque::new();
    for seed in seeds {
        queue.push_back((seed.id, 0usize, seed.qualified_name.clone()));
    }

    let max_depth = options.max_depth.min(5);

    while let Some((curr_id, depth, path_prefix)) = queue.pop_front() {
        if cancelled.is_some_and(|check| check()) {
            return candidates;
        }
        if depth >= max_depth || candidates.len() >= options.max_candidates {
            continue;
        }

        // Check container (Contains edge pointing TO curr_id)
        for edge in &graph.edges {
            if edge.kind == EdgeKind::Contains && edge.to == curr_id && visited.insert(edge.from) {
                if let Some(container_node) = graph.nodes.iter().find(|n| n.id == edge.from) {
                    candidates.push(ContextCandidate {
                        node: container_node.clone(),
                        role: CandidateRole::Container,
                        distance: depth + 1,
                        reason: format!("Container of {path_prefix}"),
                    });
                    if depth + 1 < max_depth {
                        queue.push_back((
                            edge.from,
                            depth + 1,
                            format!("{path_prefix} <- container"),
                        ));
                    }
                }
            }
        }

        // Outgoing Calls (Callees)
        for edge in &graph.edges {
            if edge.kind == EdgeKind::Calls && edge.from == curr_id {
                let role = if depth == 0 {
                    CandidateRole::Callee
                } else {
                    CandidateRole::IndirectNeighbor
                };
                if visited.insert(edge.to) {
                    if let Some(callee_node) = graph.nodes.iter().find(|n| n.id == edge.to) {
                        candidates.push(ContextCandidate {
                            node: callee_node.clone(),
                            role,
                            distance: depth + 1,
                            reason: format!("Callee of {path_prefix}"),
                        });
                        if depth + 1 < max_depth {
                            queue.push_back((
                                edge.to,
                                depth + 1,
                                format!("{path_prefix} -> {}", callee_node.name),
                            ));
                        }
                    }
                }
            }
        }

        // Incoming Calls (Callers)
        for edge in &graph.edges {
            if edge.kind == EdgeKind::Calls && edge.to == curr_id {
                let role = if depth == 0 {
                    CandidateRole::Caller
                } else {
                    CandidateRole::IndirectNeighbor
                };
                if visited.insert(edge.from) {
                    if let Some(caller_node) = graph.nodes.iter().find(|n| n.id == edge.from) {
                        // Check if caller is a test function/file
                        let is_test = caller_node.file.contains("test")
                            || caller_node.name.starts_with("test_")
                            || caller_node.name.ends_with("_test");
                        let final_role = if is_test { CandidateRole::Test } else { role };

                        candidates.push(ContextCandidate {
                            node: caller_node.clone(),
                            role: final_role,
                            distance: depth + 1,
                            reason: format!("Caller of {path_prefix}"),
                        });
                        if depth + 1 < max_depth {
                            queue.push_back((
                                edge.from,
                                depth + 1,
                                format!("{} -> {path_prefix}", caller_node.name),
                            ));
                        }
                    }
                }
            }
        }

        // Implements / Inherits
        for edge in &graph.edges {
            if (edge.kind == EdgeKind::Implements || edge.kind == EdgeKind::Inherits)
                && (edge.from == curr_id || edge.to == curr_id)
            {
                let other_id = if edge.from == curr_id {
                    edge.to
                } else {
                    edge.from
                };
                if visited.insert(other_id) {
                    if let Some(impl_node) = graph.nodes.iter().find(|n| n.id == other_id) {
                        candidates.push(ContextCandidate {
                            node: impl_node.clone(),
                            role: CandidateRole::Implementation,
                            distance: depth + 1,
                            reason: format!("Implementation/trait link for {path_prefix}"),
                        });
                        if depth + 1 < max_depth {
                            queue.push_back((other_id, depth + 1, format!("impl({path_prefix})")));
                        }
                    }
                }
            }
        }

        // Referenced Types (Imports or Callees that are Struct, Trait, Interface, Class)
        for edge in &graph.edges {
            if (edge.kind == EdgeKind::Imports || edge.kind == EdgeKind::Calls)
                && edge.from == curr_id
            {
                if let Some(type_node) = graph.nodes.iter().find(|n| n.id == edge.to) {
                    if matches!(
                        type_node.kind,
                        NodeKind::Struct | NodeKind::Trait | NodeKind::Interface | NodeKind::Class
                    ) && visited.insert(edge.to)
                    {
                        candidates.push(ContextCandidate {
                            node: type_node.clone(),
                            role: CandidateRole::ReferencedType,
                            distance: depth + 1,
                            reason: format!("Referenced type in {path_prefix}"),
                        });
                        if depth + 1 < max_depth {
                            queue.push_back((edge.to, depth + 1, format!("type({path_prefix})")));
                        }
                    }
                }
            }
        }
    }

    if candidates.len() > options.max_candidates {
        candidates.truncate(options.max_candidates);
    }

    candidates
}
