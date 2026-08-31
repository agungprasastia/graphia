use serde::{Deserialize, Serialize};

use super::candidate::{CandidateRole, ContextCandidate};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankedCandidate {
    pub candidate: ContextCandidate,
    pub score: f64,
}

#[must_use]
pub fn score_candidate(candidate: &ContextCandidate) -> f64 {
    // Base score based on role and distance
    // Seed: 1000, Callers/Callees: 800, Container: 750, Types: 700, Dist 2: 500, Dist 3: 250, Tests: 200
    match candidate.role {
        CandidateRole::Seed => 1000.0,
        CandidateRole::Caller | CandidateRole::Callee => {
            if candidate.distance <= 1 {
                800.0
            } else if candidate.distance == 2 {
                500.0
            } else {
                250.0
            }
        }
        CandidateRole::Container => 750.0,
        CandidateRole::ReferencedType => 700.0,
        CandidateRole::Implementation => 650.0,
        CandidateRole::IndirectNeighbor => {
            if candidate.distance == 2 {
                500.0
            } else if candidate.distance == 3 {
                250.0
            } else {
                100.0
            }
        }
        CandidateRole::Test => 200.0,
    }
}

#[must_use]
pub fn rank_candidates(candidates: Vec<ContextCandidate>) -> Vec<RankedCandidate> {
    let mut ranked: Vec<RankedCandidate> = candidates
        .into_iter()
        .map(|c| {
            let score = score_candidate(&c);
            RankedCandidate {
                candidate: c,
                score,
            }
        })
        .collect();

    // Deterministic tie-breaking:
    // 1. Higher score first
    // 2. Lower distance first
    // 3. Alphabetical qualified_name
    // 4. Numerical node ID
    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.candidate.distance.cmp(&b.candidate.distance))
            .then_with(|| {
                a.candidate
                    .node
                    .qualified_name
                    .cmp(&b.candidate.node.qualified_name)
            })
            .then_with(|| a.candidate.node.id.0.cmp(&b.candidate.node.id.0))
    });

    ranked
}
