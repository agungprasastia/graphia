use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::graph::Graph;
use crate::model::{Confidence, EdgeKind, Node, NodeId, NodeKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DispatchConfidence {
    Extracted,
    Inferred,
    Possible,
}

impl DispatchConfidence {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Extracted => "extracted",
            Self::Inferred => "inferred",
            Self::Possible => "possible",
        }
    }
}

impl From<Confidence> for DispatchConfidence {
    fn from(c: Confidence) -> Self {
        match c {
            Confidence::Extracted => Self::Extracted,
            Confidence::Resolved | Confidence::Inferred => Self::Inferred,
            Confidence::Possible => Self::Possible,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DispatchTarget {
    pub target: Node,
    pub confidence: DispatchConfidence,
    pub via_interface: Option<String>,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallSiteAnalysis {
    pub caller: Node,
    pub call_name: String,
    pub direct_callees: Vec<Node>,
    pub dynamic_dispatch_candidates: Vec<DispatchTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RefinedCallGraph {
    pub call_sites: Vec<CallSiteAnalysis>,
    pub total_call_sites: usize,
    pub dynamic_dispatch_count: usize,
}

struct TraitMaps {
    impl_map: HashMap<NodeId, Vec<NodeId>>,
    trait_methods: HashMap<NodeId, Vec<NodeId>>,
    struct_methods: HashMap<NodeId, Vec<NodeId>>,
}

fn build_trait_maps(graph: &Graph) -> TraitMaps {
    let mut impl_map: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    let mut trait_methods: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    let mut struct_methods: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

    for edge in &graph.edges {
        if edge.kind == EdgeKind::Implements || edge.kind == EdgeKind::Inherits {
            impl_map.entry(edge.to).or_default().push(edge.from);
        } else if edge.kind == EdgeKind::Contains {
            if let (Some(parent), Some(child)) = (
                graph.nodes.iter().find(|n| n.id == edge.from),
                graph.nodes.iter().find(|n| n.id == edge.to),
            ) {
                if matches!(parent.kind, NodeKind::Trait | NodeKind::Interface)
                    && matches!(child.kind, NodeKind::Method | NodeKind::Function)
                {
                    trait_methods.entry(parent.id).or_default().push(child.id);
                } else if matches!(parent.kind, NodeKind::Struct | NodeKind::Class)
                    && matches!(child.kind, NodeKind::Method | NodeKind::Function)
                {
                    struct_methods.entry(parent.id).or_default().push(child.id);
                }
            }
        }
    }
    TraitMaps {
        impl_map,
        trait_methods,
        struct_methods,
    }
}

/// Computes a refined call graph analyzing direct calls and dynamic dispatch / trait candidates.
#[must_use]
pub fn analyze_callgraph(graph: &Graph) -> RefinedCallGraph {
    let maps = build_trait_maps(graph);
    let mut call_sites = Vec::new();
    let mut dynamic_count = 0;

    for caller in &graph.nodes {
        if !matches!(caller.kind, NodeKind::Function | NodeKind::Method) {
            continue;
        }

        let mut caller_direct_callees = Vec::new();
        let mut caller_dispatch_targets = Vec::new();

        for edge in &graph.edges {
            if edge.kind == EdgeKind::Calls && edge.from == caller.id {
                if let Some(target) = graph.nodes.iter().find(|n| n.id == edge.to) {
                    caller_direct_callees.push(target.clone());
                    find_dispatch_targets(graph, target, &maps, &mut caller_dispatch_targets);
                }
            }
        }

        if !caller_direct_callees.is_empty() || !caller_dispatch_targets.is_empty() {
            caller_direct_callees.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
            caller_direct_callees.dedup_by(|a, b| a.id == b.id);
            caller_dispatch_targets.sort_by(|a, b| {
                a.target
                    .qualified_name
                    .cmp(&b.target.qualified_name)
                    .then_with(|| a.target.id.0.cmp(&b.target.id.0))
            });
            caller_dispatch_targets.dedup_by(|a, b| a.target.id == b.target.id);

            dynamic_count += caller_dispatch_targets.len();
            call_sites.push(CallSiteAnalysis {
                caller: caller.clone(),
                call_name: caller.qualified_name.clone(),
                direct_callees: caller_direct_callees,
                dynamic_dispatch_candidates: caller_dispatch_targets,
            });
        }
    }

    call_sites.sort_by(|a, b| a.caller.qualified_name.cmp(&b.caller.qualified_name));
    let total_call_sites = call_sites.len();

    RefinedCallGraph {
        call_sites,
        total_call_sites,
        dynamic_dispatch_count: dynamic_count,
    }
}

fn find_dispatch_targets(
    graph: &Graph,
    target: &Node,
    maps: &TraitMaps,
    out: &mut Vec<DispatchTarget>,
) {
    if !matches!(target.kind, NodeKind::Method | NodeKind::Function) {
        return;
    }
    for (&trait_id, methods) in &maps.trait_methods {
        if methods.contains(&target.id) {
            let trait_name = graph
                .nodes
                .iter()
                .find(|n| n.id == trait_id)
                .map(|n| n.qualified_name.clone());

            if let Some(implementors) = maps.impl_map.get(&trait_id) {
                for &impl_id in implementors {
                    let s_name = graph
                        .nodes
                        .iter()
                        .find(|n| n.id == impl_id)
                        .map_or("", |s| s.name.as_str());
                    if let Some(m_ids) = maps.struct_methods.get(&impl_id) {
                        for &m_id in m_ids {
                            if let Some(m_node) = graph.nodes.iter().find(|n| n.id == m_id) {
                                if m_node.name == target.name {
                                    out.push(DispatchTarget {
                                        target: m_node.clone(),
                                        confidence: DispatchConfidence::Inferred,
                                        via_interface: trait_name.clone(),
                                        explanation: format!(
                                            "Dynamic dispatch candidate via {s_name}"
                                        ),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if out.is_empty() && target.name.len() > 2 {
        for other in &graph.nodes {
            if other.id != target.id
                && other.name == target.name
                && matches!(other.kind, NodeKind::Method)
            {
                out.push(DispatchTarget {
                    target: other.clone(),
                    confidence: DispatchConfidence::Possible,
                    via_interface: None,
                    explanation: format!("Possible candidate method '{}'", target.name),
                });
            }
        }
    }
}
