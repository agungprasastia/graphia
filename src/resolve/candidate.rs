use std::collections::BTreeSet;

use crate::model::NodeId;

/// Maximum number of candidates retained in an ambiguous candidate set or candidate selector.
pub const MAX_AMBIGUOUS_CANDIDATES: usize = 16;

/// Unified semantic resolution result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Exactly one candidate resolved unambiguously
    Resolved(NodeId),
    /// Multiple equally plausible candidates (bounded set)
    Ambiguous(Vec<NodeId>),
    /// No matching candidate found
    Unresolved,
}

impl Resolution {
    #[must_use]
    pub fn is_resolved(&self) -> bool {
        matches!(self, Self::Resolved(_))
    }

    #[must_use]
    pub fn is_ambiguous(&self) -> bool {
        matches!(self, Self::Ambiguous(_))
    }

    #[must_use]
    pub fn is_unresolved(&self) -> bool {
        matches!(self, Self::Unresolved)
    }

    #[must_use]
    pub fn node_id(&self) -> Option<NodeId> {
        match self {
            Self::Resolved(id) => Some(*id),
            _ => None,
        }
    }
}

/// Reason for resolution.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResolutionReason {
    /// Resolved via exact lexical scope shadowing (local function/block)
    LexicalScope,
    /// Resolved via same file definition
    SameFile,
    /// Resolved via explicit alias binding (`import { foo as bar }`)
    ExplicitAlias,
    /// Resolved via explicit imported symbol (`import { foo }`)
    ExplicitImport,
    /// Resolved via imported file / wildcard import
    WildcardImport,
    /// Resolved via receiver method lookup (`user.login()` -> `User::login`)
    ReceiverMethod,
    /// Resolved via type inheritance / trait / interface implementation
    InheritedMethod,
}

/// The resolution state of a call reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionState {
    /// Exactly one candidate resolved with high confidence
    Resolved {
        target: NodeId,
        reason: ResolutionReason,
    },
    /// Multiple equally plausible candidates (bounded set)
    Ambiguous { candidates: Vec<NodeId> },
    /// No matching candidate found
    Unresolved,
}

impl From<ResolutionState> for Resolution {
    fn from(state: ResolutionState) -> Self {
        match state {
            ResolutionState::Resolved { target, .. } => Resolution::Resolved(target),
            ResolutionState::Ambiguous { candidates } => Resolution::Ambiguous(candidates),
            ResolutionState::Unresolved => Resolution::Unresolved,
        }
    }
}

/// Candidate symbol match with score and reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub node_id: NodeId,
    pub priority: u32,
    pub reason: ResolutionReason,
}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| self.node_id.0.cmp(&other.node_id.0))
    }
}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Helper to accumulate and select candidates (bounded to at most 16 candidates).
#[derive(Debug, Clone, Default)]
pub struct CandidateSelector {
    candidates: BTreeSet<Candidate>,
}

impl CandidateSelector {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a candidate with priority score. Lower priority number = higher precedence.
    /// Priority guidelines:
    /// 1: LexicalScope / Local definition
    /// 2: SameFile
    /// 3: ExplicitAlias
    /// 4: ExplicitImport
    /// 5: ReceiverMethod / InheritedMethod
    /// 6: WildcardImport
    pub fn add(&mut self, node_id: NodeId, priority: u32, reason: ResolutionReason) {
        if self.candidates.len() >= MAX_AMBIGUOUS_CANDIDATES {
            // Bounded candidate storage: if capacity reached, only keep if higher priority
            if let Some(max_cand) = self.candidates.iter().next_back().cloned()
                && (priority < max_cand.priority
                    || (priority == max_cand.priority && node_id.0 < max_cand.node_id.0))
            {
                self.candidates.remove(&max_cand);
                self.candidates.insert(Candidate {
                    node_id,
                    priority,
                    reason,
                });
            }
            return;
        }
        self.candidates.insert(Candidate {
            node_id,
            priority,
            reason,
        });
    }

    /// Select the best resolution state.
    #[must_use]
    pub fn resolve(self) -> ResolutionState {
        if self.candidates.is_empty() {
            return ResolutionState::Unresolved;
        }

        // Group by highest priority (lowest priority number)
        let Some(min_priority) = self.candidates.iter().map(|c| c.priority).min() else {
            return ResolutionState::Unresolved;
        };
        let top_candidates: Vec<_> = self
            .candidates
            .iter()
            .filter(|c| c.priority == min_priority)
            .collect();

        if top_candidates.len() == 1 {
            let best = top_candidates[0];
            ResolutionState::Resolved {
                target: best.node_id,
                reason: best.reason.clone(),
            }
        } else {
            let candidates = top_candidates
                .iter()
                .take(MAX_AMBIGUOUS_CANDIDATES)
                .map(|c| c.node_id)
                .collect();
            ResolutionState::Ambiguous { candidates }
        }
    }

    /// Select as unified Resolution enum.
    #[must_use]
    pub fn resolve_unified(self) -> Resolution {
        self.resolve().into()
    }
}
