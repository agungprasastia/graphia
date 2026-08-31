use serde::{Deserialize, Serialize};

use super::projection::AdjacencyGraph;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StronglyConnectedComponent {
    pub id: usize,
    pub members: Vec<String>,
    pub size: usize,
    pub is_trivial: bool,
}

#[must_use]
pub fn tarjan_scc(graph: &AdjacencyGraph) -> Vec<StronglyConnectedComponent> {
    let n = graph.nodes.len();
    let mut index_counter = 0usize;
    let mut indices = vec![usize::MAX; n];
    let mut lowlinks = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut stack = Vec::new();
    let mut raw_sccs: Vec<Vec<usize>> = Vec::new();

    // Deterministic iteration order: nodes are already sorted canonically 0..n
    for start_node in 0..n {
        if indices[start_node] == usize::MAX {
            // Non-recursive / explicit stack Tarjan to guarantee unbounded recursion safety
            struct Frame {
                u: usize,
                edge_idx: usize,
            }

            let mut call_stack = vec![Frame {
                u: start_node,
                edge_idx: 0,
            }];
            indices[start_node] = index_counter;
            lowlinks[start_node] = index_counter;
            index_counter += 1;
            stack.push(start_node);
            on_stack[start_node] = true;

            while let Some(frame) = call_stack.last_mut() {
                let u = frame.u;
                if frame.edge_idx < graph.outgoing[u].len() {
                    let (v, _) = graph.outgoing[u][frame.edge_idx];
                    frame.edge_idx += 1;

                    if indices[v] == usize::MAX {
                        indices[v] = index_counter;
                        lowlinks[v] = index_counter;
                        index_counter += 1;
                        stack.push(v);
                        on_stack[v] = true;

                        call_stack.push(Frame { u: v, edge_idx: 0 });
                    } else if on_stack[v] {
                        lowlinks[u] = lowlinks[u].min(indices[v]);
                    }
                } else {
                    let finished = call_stack.pop().unwrap();
                    let u = finished.u;

                    if let Some(parent) = call_stack.last() {
                        lowlinks[parent.u] = lowlinks[parent.u].min(lowlinks[u]);
                    }

                    if lowlinks[u] == indices[u] {
                        let mut scc = Vec::new();
                        while let Some(w) = stack.pop() {
                            on_stack[w] = false;
                            scc.push(w);
                            if w == u {
                                break;
                            }
                        }
                        raw_sccs.push(scc);
                    }
                }
            }
        }
    }

    // Canonicalize components:
    // Sort each component's member IDs lexicographically
    let mut result: Vec<StronglyConnectedComponent> = raw_sccs
        .into_iter()
        .map(|scc_indices| {
            let mut members: Vec<String> = scc_indices
                .into_iter()
                .map(|idx| graph.nodes[idx].id.clone())
                .collect();
            members.sort();
            let size = members.len();
            let is_trivial = if size == 1 {
                let node_idx = graph.node_indices[&members[0]];
                !graph.has_self_loop(node_idx)
            } else {
                false
            };
            StronglyConnectedComponent {
                id: 0,
                members,
                size,
                is_trivial,
            }
        })
        .collect();

    // Sort components deterministically by size (descending), then lexicographical first member
    result.sort_by(|a, b| {
        b.size
            .cmp(&a.size)
            .then_with(|| a.members.first().cmp(&b.members.first()))
    });

    for (i, scc) in result.iter_mut().enumerate() {
        scc.id = i;
    }

    result
}
