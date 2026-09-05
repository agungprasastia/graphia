use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::projection::AdjacencyGraph;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Community {
    pub id: usize,
    pub members: Vec<String>,
    pub size: usize,
    pub internal_edges: usize,
    pub external_edges: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct CommunityConfig {
    pub max_iterations: usize,
}

impl Default for CommunityConfig {
    fn default() -> Self {
        Self { max_iterations: 50 }
    }
}

/// Deterministic Label Propagation Community Detection with canonical tie-breaking
#[must_use]
pub fn detect_communities(graph: &AdjacencyGraph, config: CommunityConfig) -> Vec<Community> {
    let n = graph.nodes.len();
    if n == 0 {
        return Vec::new();
    }

    // Initialize each node with its own label (0..n)
    let mut labels: Vec<usize> = (0..n).collect();

    // Construct undirected adjacency with weights for community clustering
    let mut undirected_adj: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
    for (u, adj) in undirected_adj.iter_mut().enumerate() {
        let mut neighbors_map: BTreeMap<usize, usize> = BTreeMap::new();
        for &(v, w) in &graph.outgoing[u] {
            *neighbors_map.entry(v).or_insert(0) += w;
        }
        for &(v, w) in &graph.incoming[u] {
            *neighbors_map.entry(v).or_insert(0) += w;
        }
        *adj = neighbors_map.into_iter().collect();
    }

    // A self-vote prevents adjacent nodes from swapping labels forever while
    // retaining synchronous, order-independent updates.
    for _ in 0..config.max_iterations {
        let mut next_labels = labels.clone();
        let mut changed = false;

        // Iterate deterministically in node index order (0..n)
        for u in 0..n {
            if undirected_adj[u].is_empty() {
                continue;
            }

            // Count label weights among neighbors
            let mut label_weights: BTreeMap<usize, usize> = BTreeMap::new();
            for &(v, w) in &undirected_adj[u] {
                let lbl = labels[v];
                *label_weights.entry(lbl).or_insert(0) += w;
            }
            *label_weights.entry(labels[u]).or_insert(0) += 1;

            // Canonical tie-breaking: max weight, then smallest label ID
            let mut best_label = labels[u];
            let mut max_weight = 0;

            for (&lbl, &w) in &label_weights {
                if w > max_weight || (w == max_weight && lbl < best_label) {
                    max_weight = w;
                    best_label = lbl;
                }
            }

            if best_label != labels[u] {
                next_labels[u] = best_label;
                changed = true;
            }
        }

        labels = next_labels;
        if !changed {
            break;
        }
    }

    // Group nodes by community label
    let mut comm_groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (node_idx, &lbl) in labels.iter().enumerate() {
        comm_groups.entry(lbl).or_default().push(node_idx);
    }

    let mut communities = Vec::new();
    for (_, member_indices) in comm_groups {
        let member_set: std::collections::HashSet<usize> = member_indices.iter().copied().collect();
        let mut members: Vec<String> = member_indices
            .iter()
            .map(|&idx| graph.nodes[idx].id.clone())
            .collect();
        members.sort();

        let mut internal_edges = 0;
        let mut external_edges = 0;

        for &u in &member_indices {
            for &(v, _) in &graph.outgoing[u] {
                if member_set.contains(&v) {
                    internal_edges += 1;
                } else {
                    external_edges += 1;
                }
            }
        }

        communities.push(Community {
            id: 0,
            members,
            size: member_indices.len(),
            internal_edges,
            external_edges,
        });
    }

    // Sort communities by size descending, then lexicographical first member
    communities.sort_by(|a, b| {
        b.size
            .cmp(&a.size)
            .then_with(|| a.members.first().cmp(&b.members.first()))
    });

    for (i, c) in communities.iter_mut().enumerate() {
        c.id = i;
    }

    communities
}
