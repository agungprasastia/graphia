use serde::{Deserialize, Serialize};

use super::projection::AdjacencyGraph;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CouplingMetrics {
    pub id: String,
    pub afferent_coupling: usize, // Ca: incoming dependents (fan-in)
    pub efferent_coupling: usize, // Ce: outgoing dependencies (fan-out)
    pub fan_in: usize,
    pub fan_out: usize,
    pub instability: f64, // I = Ce / (Ca + Ce), guard 0 if Ca + Ce == 0
}

#[must_use]
pub fn compute_coupling(graph: &AdjacencyGraph) -> Vec<CouplingMetrics> {
    let n = graph.nodes.len();
    let mut results = Vec::with_capacity(n);

    for i in 0..n {
        let ca = graph.incoming[i].len();
        let ce = graph.outgoing[i].len();
        let total = ca + ce;
        let instability = if total == 0 {
            0.0
        } else {
            (ce as f64) / (total as f64)
        };

        results.push(CouplingMetrics {
            id: graph.nodes[i].id.clone(),
            afferent_coupling: ca,
            efferent_coupling: ce,
            fan_in: ca,
            fan_out: ce,
            instability,
        });
    }

    results.sort_by(|a, b| {
        b.afferent_coupling
            .cmp(&a.afferent_coupling)
            .then_with(|| b.efferent_coupling.cmp(&a.efferent_coupling))
            .then_with(|| a.id.cmp(&b.id))
    });

    results
}
