use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;

use serde::{Deserialize, Serialize};

use super::callgraph::DispatchConfidence;
use super::typeflow::extract_local_flow_graph;
use crate::graph::Graph;
use crate::model::{Confidence, EdgeKind, Node, NodeId, NodeIdentity, NodeKind};

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
    pub nodes: Vec<Node>,
    pub edges: Vec<DataFlowEdge>,
}

impl DataFlowGraph {
    #[must_use]
    pub fn from_graph(graph: &Graph) -> Self {
        let mut result = Self {
            nodes: graph.nodes.clone(),
            edges: Vec::new(),
        };
        let Some(root) = graph.source_root() else {
            return result;
        };
        let mut flows = BTreeMap::new();
        for function in graph.nodes.iter().filter(|node| {
            matches!(
                node.kind,
                NodeKind::Function | NodeKind::Method | NodeKind::Constructor
            )
        }) {
            let Ok(source) = fs::read_to_string(root.join(&function.file)) else {
                continue;
            };
            let flow = extract_local_flow_graph(&function.name, &function.file, &source, 1);
            let mut names = BTreeSet::new();
            for parameter in &flow.parameters {
                names.insert(parameter.name.clone());
            }
            for binding in &flow.bindings {
                names.insert(binding.name.clone());
            }
            let mut ids = BTreeMap::new();
            for name in names {
                let id = flow_node(function, &name);
                ids.insert(name.clone(), id);
                result.nodes.push(flow_node_record(id, &name, function));
            }
            let return_id = flow_node(function, "return");
            result
                .nodes
                .push(flow_node_record(return_id, "return", function));
            for assignment in &flow.assignments {
                if let (Some(&from), Some(&to)) =
                    (ids.get(&assignment.from), ids.get(&assignment.to))
                {
                    result.edges.push(DataFlowEdge {
                        from,
                        to,
                        kind: EdgeKind::References,
                        confidence: Confidence::Extracted,
                    });
                }
            }
            for ret in &flow.returns {
                if let Some(&from) = ids.get(&ret.source) {
                    result.edges.push(DataFlowEdge {
                        from,
                        to: return_id,
                        kind: EdgeKind::References,
                        confidence: Confidence::Extracted,
                    });
                }
            }
            flows.insert(function.id, (flow, ids, return_id));
        }
        for (caller_id, (flow, ids, _)) in &flows {
            for argument in &flow.call_arguments {
                let Some(&from) = ids.get(&argument.argument) else {
                    continue;
                };
                let Some(call) = graph.edges.iter().find(|edge| {
                    edge.from == *caller_id
                        && edge.kind == EdgeKind::Calls
                        && graph
                            .nodes
                            .iter()
                            .any(|node| node.id == edge.to && node.name == argument.call)
                }) else {
                    continue;
                };
                let Some((callee_flow, callee_ids, _)) = flows.get(&call.to) else {
                    continue;
                };
                let Some(parameter) = callee_flow.parameters.get(argument.index) else {
                    continue;
                };
                let Some(&to) = callee_ids.get(&parameter.name) else {
                    continue;
                };
                result.edges.push(DataFlowEdge {
                    from,
                    to,
                    kind: EdgeKind::References,
                    confidence: Confidence::Resolved,
                });
            }
            for assignment in &flow.assignments {
                let Some(&to) = ids.get(&assignment.to) else {
                    continue;
                };
                let Some(call) = graph.edges.iter().find(|edge| {
                    edge.from == *caller_id
                        && edge.kind == EdgeKind::Calls
                        && graph
                            .nodes
                            .iter()
                            .any(|node| node.id == edge.to && node.name == assignment.from)
                }) else {
                    continue;
                };
                let Some((_, _, return_id)) = flows.get(&call.to) else {
                    continue;
                };
                result.edges.push(DataFlowEdge {
                    from: *return_id,
                    to,
                    kind: EdgeKind::References,
                    confidence: Confidence::Resolved,
                });
            }
        }
        result
            .edges
            .sort_by_key(|edge| (edge.from, edge.to, edge.kind.code(), edge.confidence.code()));
        result.edges.dedup();
        result
    }
}

fn flow_node(function: &Node, name: &str) -> NodeId {
    crate::graph::stable_node_id(&NodeIdentity::new(
        function.language,
        &function.file,
        NodeKind::Variable,
        &format!("{}::#flow::{}", function.qualified_name, name),
        Some(&function.qualified_name),
        None,
    ))
}

fn flow_node_record(id: NodeId, name: &str, function: &Node) -> Node {
    Node {
        id,
        kind: NodeKind::Variable,
        name: name.into(),
        qualified_name: format!("{}::#flow::{}", function.qualified_name, name),
        file: function.file.clone(),
        location: function.location.clone(),
        language: function.language,
        visibility: crate::model::Visibility::Private,
        signature: None,
        container: Some(function.qualified_name.clone()),
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
    let dataflow = DataFlowGraph::from_graph(graph);
    let sources = dataflow
        .nodes
        .iter()
        .filter(|node| {
            node.name.contains(source_query) || node.qualified_name.contains(source_query)
        })
        .cloned()
        .collect::<Vec<_>>();
    let sinks = dataflow
        .nodes
        .iter()
        .filter(|node| node.name.contains(sink_query) || node.qualified_name.contains(sink_query))
        .cloned()
        .collect::<Vec<_>>();

    let mut paths = Vec::new();
    let max_paths = limit.unwrap_or(10);

    for source in &sources {
        for sink in &sinks {
            if source.id == sink.id {
                continue;
            }
            let query = DataFlowQuery::new(&dataflow);
            for path in query.trace_flow(
                source.id,
                sink.id,
                15,
                max_paths.saturating_sub(paths.len()),
            ) {
                paths.push(flow_path(&dataflow, source, sink, &path));
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
    dataflow: &DataFlowGraph,
    source: &Node,
    sink: &Node,
    path: &[(NodeId, Confidence)],
) -> SourceSinkFlowPath {
    let mut overall = DispatchConfidence::Extracted;
    let steps = path
        .iter()
        .enumerate()
        .filter_map(|(index, (id, confidence))| {
            let node = dataflow.nodes.iter().find(|node| node.id == *id)?.clone();
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
                    dataflow
                        .edges
                        .iter()
                        .find(|edge| edge.to == *id)
                        .map_or_else(|| "References".into(), |edge| edge.kind.as_str().into())
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
