use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::projection::AdjacencyGraph;
use super::scc::tarjan_scc;
use crate::model::EdgeKind;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cycle {
    pub path: Vec<String>,
    pub length: usize,
    pub edge_kinds: Vec<Vec<EdgeKind>>,
}

#[derive(Debug, Clone, Copy)]
pub struct CycleConfig {
    pub max_length: usize,
    pub max_cycles: usize,
    pub include_self_loops: bool,
}

impl Default for CycleConfig {
    fn default() -> Self {
        Self {
            max_length: 20,
            max_cycles: 100,
            include_self_loops: false,
        }
    }
}

#[must_use]
pub fn canonical_cycle_representation(cycle: &[usize]) -> Vec<usize> {
    if cycle.is_empty() {
        return Vec::new();
    }
    // Find index of smallest node
    let min_pos = cycle
        .iter()
        .enumerate()
        .min_by_key(|&(_, &val)| val)
        .map(|(idx, _)| idx)
        .unwrap_or(0);

    let mut canonical = Vec::with_capacity(cycle.len());
    for i in 0..cycle.len() {
        canonical.push(cycle[(min_pos + i) % cycle.len()]);
    }
    canonical
}

struct CycleDfsContext<'a> {
    graph: &'a AdjacencyGraph,
    scc_nodes: &'a std::collections::HashSet<usize>,
    config: &'a CycleConfig,
    visited: Vec<bool>,
    path: Vec<usize>,
    found: BTreeSet<Vec<usize>>,
}

#[must_use]
pub fn find_cycles(graph: &AdjacencyGraph, config: CycleConfig) -> Vec<Cycle> {
    let sccs = tarjan_scc(graph);
    let mut found_cycles: BTreeSet<Vec<usize>> = BTreeSet::new();

    for scc in &sccs {
        if found_cycles.len() >= config.max_cycles {
            break;
        }

        // Trivial 1-node SCC with no self-loop has no cycles
        if scc.is_trivial {
            continue;
        }

        if scc.members.len() == 1 {
            let u = graph.node_indices[&scc.members[0]];
            if config.include_self_loops && graph.has_self_loop(u) {
                found_cycles.insert(vec![u]);
            }
            continue;
        }

        let scc_node_set: std::collections::HashSet<usize> = scc
            .members
            .iter()
            .map(|id| graph.node_indices[id])
            .collect();

        // Bounded cycle search via deterministic DFS per start node in SCC
        let mut scc_nodes: Vec<usize> = scc_node_set.iter().copied().collect();
        scc_nodes.sort();

        for &start_node in &scc_nodes {
            if found_cycles.len() >= config.max_cycles {
                break;
            }

            let mut ctx = CycleDfsContext {
                graph,
                scc_nodes: &scc_node_set,
                config: &config,
                visited: vec![false; graph.nodes.len()],
                path: Vec::new(),
                found: BTreeSet::new(),
            };

            dfs_cycles(start_node, start_node, &mut ctx);

            for cycle in ctx.found {
                found_cycles.insert(cycle);
                if found_cycles.len() >= config.max_cycles {
                    break;
                }
            }
        }
    }

    let mut cycles: Vec<Cycle> = found_cycles
        .into_iter()
        .map(|cycle_indices| {
            let path: Vec<String> = cycle_indices
                .iter()
                .map(|&idx| graph.nodes[idx].id.clone())
                .collect();
            let length = path.len();

            let mut edge_kinds = Vec::new();
            for i in 0..length {
                let u = cycle_indices[i];
                let v = cycle_indices[(i + 1) % length];
                let kinds = graph
                    .edge_kinds
                    .get(&(u, v))
                    .cloned()
                    .unwrap_or_else(|| vec![EdgeKind::Calls]);
                edge_kinds.push(kinds);
            }

            Cycle {
                path,
                length,
                edge_kinds,
            }
        })
        .collect();

    // Deterministic sort: by length ascending, then lexicographically by path
    cycles.sort_by(|a, b| a.length.cmp(&b.length).then_with(|| a.path.cmp(&b.path)));

    if cycles.len() > config.max_cycles {
        cycles.truncate(config.max_cycles);
    }

    cycles
}

fn dfs_cycles(start_node: usize, current: usize, ctx: &mut CycleDfsContext<'_>) {
    if ctx.found.len() >= ctx.config.max_cycles {
        return;
    }

    ctx.visited[current] = true;
    ctx.path.push(current);

    for &(neighbor, _) in &ctx.graph.outgoing[current] {
        if ctx.found.len() >= ctx.config.max_cycles {
            break;
        }

        if !ctx.scc_nodes.contains(&neighbor) {
            continue;
        }

        if neighbor == start_node {
            if ctx.path.len() == 1 {
                if ctx.config.include_self_loops {
                    ctx.found.insert(canonical_cycle_representation(&ctx.path));
                }
            } else {
                ctx.found.insert(canonical_cycle_representation(&ctx.path));
            }
        } else if !ctx.visited[neighbor]
            && ctx.path.len() < ctx.config.max_length
            && neighbor > start_node
        {
            dfs_cycles(start_node, neighbor, ctx);
        }
    }

    ctx.path.pop();
    ctx.visited[current] = false;
}
