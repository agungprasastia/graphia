pub mod boundaries;
pub mod callgraph;
pub mod change_coupling;
pub mod dataflow;
pub mod dead_code;
pub mod diff;
pub mod history;
pub mod typeflow;

pub use boundaries::{
    ArchitectureCheckReport, ArchitectureRulesConfig, LayerDefinition, RuleViolation,
    check_architecture_boundaries,
};
pub use callgraph::{
    CallSiteAnalysis, DispatchConfidence, DispatchTarget, RefinedCallGraph, analyze_callgraph,
};
pub use change_coupling::{ChangeCouplingReport, CoChangePair, compute_change_coupling};
pub use dataflow::{
    DataFlowGraph, DataFlowQuery, FlowAnalysisReport, FlowStep, SourceSinkFlowPath,
    build_dataflow_graph, find_source_sink_flows,
};
pub use dead_code::{DeadCodeCandidate, DeadCodeReport, detect_dead_code_candidates};
pub use diff::{ApiDiffSummary, GraphDiffSummary, NodeModification, diff_graphs, diff_public_api};
pub use history::{FileChurn, GitCommitRecord, GitHistorySummary, analyze_git_history};
pub use typeflow::{
    AssignmentEdge, AssignmentFlow, CallArgumentFlow, LocalBinding, LocalFlowGraph, ParameterFlow,
    ProceduralTypeFlow, ReturnFlow, extract_intraprocedural_typeflow, extract_local_flow_graph,
};
