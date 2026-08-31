use std::collections::{HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use super::callgraph::DispatchConfidence;
use crate::graph::Graph;
use crate::model::{EdgeKind, Node};
use crate::query::QueryIndex;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlowStep {
    pub node: Node,
    pub step_index: usize,
    pub confidence: DispatchConfidence,
    pub edge_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceSinkFlowPath {
    pub source: Node,
    pub sink: Node,
    pub length: usize,
    pub steps: Vec<FlowStep>,
    pub overall_confidence: DispatchConfidence,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlowAnalysisReport {
    pub source_query: String,
    pub sink_query: String,
    pub paths_found: usize,
    pub paths: Vec<SourceSinkFlowPath>,
}

#[must_use]
pub fn find_source_sink_flows(
    graph: &Graph,
    source_query: &str,
    sink_query: &str,
    limit: Option<usize>,
) -> FlowAnalysisReport {
    let index = QueryIndex::new(graph);
    let sources = index.find(graph, source_query);
    let sinks = index.find(graph, sink_query);

    let mut paths = Vec::new();
    let max_paths = limit.unwrap_or(10);

    for source in &sources {
        for sink in &sinks {
            if source.id == sink.id {
                continue;
            }
            if let Some(path) = bfs_flow_path(graph, source, sink) {
                paths.push(path);
                if paths.len() >= max_paths {
                    break;
                }
            }
        }
        if paths.len() >= max_paths {
            break;
        }
    }

    paths.sort_by_key(|p| p.length);
    let paths_found = paths.len();

    FlowAnalysisReport {
        source_query: source_query.to_string(),
        sink_query: sink_query.to_string(),
        paths_found,
        paths,
    }
}

fn bfs_flow_path(graph: &Graph, source: &Node, sink: &Node) -> Option<SourceSinkFlowPath> {
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();

    // queue stores: (current_node_id, path_tuples: Vec<(node_id, edge_type, DispatchConfidence)>)
    queue.push_back((
        source.id,
        vec![(
            source.id,
            "source".to_string(),
            DispatchConfidence::Extracted,
        )],
    ));
    visited.insert(source.id);

    while let Some((curr_id, curr_path)) = queue.pop_front() {
        if curr_id == sink.id {
            let mut steps = Vec::new();
            let mut overall = DispatchConfidence::Extracted;

            for (idx, (nid, edge_type, conf)) in curr_path.into_iter().enumerate() {
                if let Some(node) = graph.nodes.iter().find(|n| n.id == nid) {
                    if conf == DispatchConfidence::Possible {
                        overall = DispatchConfidence::Possible;
                    } else if conf == DispatchConfidence::Inferred
                        && overall != DispatchConfidence::Possible
                    {
                        overall = DispatchConfidence::Inferred;
                    }
                    steps.push(FlowStep {
                        node: node.clone(),
                        step_index: idx,
                        confidence: conf,
                        edge_type,
                    });
                }
            }

            return Some(SourceSinkFlowPath {
                source: source.clone(),
                sink: sink.clone(),
                length: steps.len(),
                steps,
                overall_confidence: overall,
            });
        }

        if curr_path.len() > 15 {
            continue;
        }

        for edge in &graph.edges {
            if edge.from == curr_id
                && (edge.kind == EdgeKind::Calls
                    || edge.kind == EdgeKind::Contains
                    || edge.kind == EdgeKind::Imports)
            {
                let next_id = edge.to;
                if visited.insert(next_id) {
                    let mut next_path = curr_path.clone();
                    let conf = match edge.confidence {
                        crate::model::Confidence::Extracted => DispatchConfidence::Extracted,
                        crate::model::Confidence::Resolved | crate::model::Confidence::Inferred => {
                            DispatchConfidence::Inferred
                        }
                        crate::model::Confidence::Possible => DispatchConfidence::Possible,
                    };
                    next_path.push((next_id, edge.kind.as_str().to_string(), conf));
                    queue.push_back((next_id, next_path));
                }
            }
        }
    }

    None
}
