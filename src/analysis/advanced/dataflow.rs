use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use super::callgraph::DispatchConfidence;
use crate::graph::Graph;
use crate::model::{Confidence, EdgeKind, Node, NodeId};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataFlowEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataFlowGraph {
    pub edges: Vec<DataFlowEdge>,
}

impl DataFlowGraph {
    #[must_use]
    pub fn from_graph(graph: &Graph) -> Self {
        Self {
            edges: graph
                .edges
                .iter()
                .filter(|edge| edge.kind == EdgeKind::Calls)
                .map(|edge| DataFlowEdge {
                    from: edge.from,
                    to: edge.to,
                    kind: edge.kind,
                    confidence: edge.confidence,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DataFlowQuery<'a> {
    graph: &'a DataFlowGraph,
}

impl<'a> DataFlowQuery<'a> {
    #[must_use]
    pub fn new(graph: &'a DataFlowGraph) -> Self {
        Self { graph }
    }

    #[must_use]
    pub fn trace_flow(
        &self,
        source: NodeId,
        sink: NodeId,
        max_depth: usize,
        max_paths: usize,
    ) -> Vec<Vec<(NodeId, Confidence)>> {
        let mut paths = Vec::new();
        let mut queue = VecDeque::from([(source, vec![(source, Confidence::Extracted)])]);
        while let Some((current, path)) = queue.pop_front() {
            if current == sink {
                paths.push(path.clone());
                if paths.len() >= max_paths {
                    break;
                }
                continue;
            }
            if path.len().saturating_sub(1) >= max_depth {
                continue;
            }
            for edge in self.graph.edges.iter().filter(|edge| edge.from == current) {
                if path.iter().any(|(id, _)| *id == edge.to) {
                    continue;
                }
                let mut next = path.clone();
                next.push((edge.to, edge.confidence));
                queue.push_back((edge.to, next));
            }
        }
        paths
    }
}

#[must_use]
pub fn build_dataflow_graph(graph: &Graph) -> DataFlowGraph {
    DataFlowGraph::from_graph(graph)
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
            let dataflow = DataFlowGraph::from_graph(graph);
            let query = DataFlowQuery::new(&dataflow);
            for path in query.trace_flow(
                source.id,
                sink.id,
                15,
                max_paths.saturating_sub(paths.len()),
            ) {
                paths.push(flow_path(graph, source, sink, &path));
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

fn flow_path(
    graph: &Graph,
    source: &Node,
    sink: &Node,
    path: &[(NodeId, Confidence)],
) -> SourceSinkFlowPath {
    let mut overall = DispatchConfidence::Extracted;
    let steps = path
        .iter()
        .enumerate()
        .filter_map(|(index, (id, confidence))| {
            let node = graph.nodes.iter().find(|node| node.id == *id)?.clone();
            let dispatch = match confidence {
                Confidence::Possible => {
                    overall = DispatchConfidence::Possible;
                    DispatchConfidence::Possible
                }
                Confidence::Resolved | Confidence::Inferred => {
                    if overall != DispatchConfidence::Possible {
                        overall = DispatchConfidence::Inferred;
                    }
                    DispatchConfidence::Inferred
                }
                Confidence::Extracted => DispatchConfidence::Extracted,
            };
            Some(FlowStep {
                node,
                step_index: index,
                confidence: dispatch,
                edge_type: if index == 0 {
                    "source".into()
                } else {
                    "Calls".into()
                },
            })
        })
        .collect::<Vec<_>>();
    SourceSinkFlowPath {
        source: source.clone(),
        sink: sink.clone(),
        length: steps.len(),
        steps,
        overall_confidence: overall,
    }
}
