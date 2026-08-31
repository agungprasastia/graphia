use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::graph::Graph;
use crate::model::EdgeKind;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LayerDefinition {
    pub name: String,
    pub path_patterns: Vec<String>,
    pub allowed_dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchitectureRulesConfig {
    pub layers: Vec<LayerDefinition>,
}

impl Default for ArchitectureRulesConfig {
    fn default() -> Self {
        Self {
            layers: vec![
                LayerDefinition {
                    name: "domain".into(),
                    path_patterns: vec!["model".into(), "domain".into()],
                    allowed_dependencies: vec![],
                },
                LayerDefinition {
                    name: "analysis".into(),
                    path_patterns: vec!["analysis".into(), "resolve".into(), "parse".into()],
                    allowed_dependencies: vec!["domain".into()],
                },
                LayerDefinition {
                    name: "app".into(),
                    path_patterns: vec!["cli".into(), "daemon".into(), "mcp".into()],
                    allowed_dependencies: vec!["domain".into(), "analysis".into()],
                },
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuleViolation {
    pub from_file: String,
    pub to_file: String,
    pub from_layer: String,
    pub to_layer: String,
    pub edge_kind: EdgeKind,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchitectureCheckReport {
    pub total_edges_evaluated: usize,
    pub violations_count: usize,
    pub violations: Vec<RuleViolation>,
    pub passed: bool,
}

#[must_use]
pub fn check_architecture_boundaries(
    graph: &Graph,
    config: &ArchitectureRulesConfig,
) -> ArchitectureCheckReport {
    let mut violations = Vec::new();
    let mut total_evaluated = 0;

    let node_layers: HashMap<_, _> = graph
        .nodes
        .iter()
        .map(|n| (n.id, (n.file.clone(), get_node_layer(&n.file, config))))
        .collect();

    let layer_allowed_map: HashMap<String, Vec<String>> = config
        .layers
        .iter()
        .map(|l| (l.name.clone(), l.allowed_dependencies.clone()))
        .collect();

    for edge in &graph.edges {
        if edge.kind == EdgeKind::Contains {
            continue;
        }

        if let (Some((from_file, Some(from_layer))), Some((to_file, Some(to_layer)))) =
            (node_layers.get(&edge.from), node_layers.get(&edge.to))
        {
            total_evaluated += 1;
            if from_layer != to_layer {
                if let Some(allowed) = layer_allowed_map.get(from_layer) {
                    if !allowed.contains(to_layer) {
                        violations.push(RuleViolation {
                            from_file: from_file.clone(),
                            to_file: to_file.clone(),
                            from_layer: from_layer.clone(),
                            to_layer: to_layer.clone(),
                            edge_kind: edge.kind,
                            reason: format!(
                                "Layer '{from_layer}' is not allowed to depend on layer '{to_layer}'"
                            ),
                        });
                    }
                }
            }
        }
    }

    violations.sort_by(|a, b| {
        a.from_file
            .cmp(&b.from_file)
            .then_with(|| a.to_file.cmp(&b.to_file))
    });
    violations.dedup();

    let violations_count = violations.len();
    let passed = violations_count == 0;

    ArchitectureCheckReport {
        total_edges_evaluated: total_evaluated,
        violations_count,
        violations,
        passed,
    }
}

fn get_node_layer(file_path: &str, config: &ArchitectureRulesConfig) -> Option<String> {
    let norm = file_path.replace('\\', "/");
    let segments: Vec<&str> = norm
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    let mut matches = config
        .layers
        .iter()
        .filter(|layer| {
            layer.path_patterns.iter().any(|pattern| {
                let pattern = pattern.trim_matches('/');
                if pattern.is_empty() {
                    return false;
                }
                if pattern.contains('/') {
                    norm == pattern || norm.ends_with(&format!("/{pattern}"))
                } else {
                    segments.iter().any(|segment| {
                        *segment == pattern || wildcard_segment_matches(segment, pattern)
                    })
                }
            })
        })
        .map(|layer| layer.name.clone())
        .collect::<Vec<_>>();

    // A path matching more than one layer is intentionally unclassified. This
    // prevents configuration order from becoming hidden semantic precedence.
    matches.sort();
    matches.dedup();
    (matches.len() == 1).then(|| matches.remove(0))
}

fn wildcard_segment_matches(segment: &str, pattern: &str) -> bool {
    let mut remaining = segment;
    for part in pattern.split('*') {
        if part.is_empty() {
            continue;
        }
        let Some(index) = remaining.find(part) else {
            return false;
        };
        remaining = &remaining[index + part.len()..];
    }
    pattern.starts_with('*') || pattern.ends_with('*') || remaining.is_empty()
}
