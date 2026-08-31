use graphia::analysis::advanced::boundaries::{
    ArchitectureRulesConfig, LayerDefinition, check_architecture_boundaries,
};
use graphia::analysis::advanced::diff::diff_graphs;
use graphia::analysis::advanced::typeflow::extract_intraprocedural_typeflow;
use graphia::graph::build_graph;
use graphia::model::Language as GraphiaLanguage;
use graphia::parser::parse_file;

#[test]
fn test_intraprocedural_typeflow_extracts_parameters_assignments_and_returns() {
    let fn_src = r#"
fn compute_total(user: User, base_amount: f64) -> f64 {
    let fee = 10.0;
    let total = base_amount + fee;
    return total;
}
"#;
    let flow = extract_intraprocedural_typeflow("compute_total", "src/calc.rs", fn_src, 1);
    assert_eq!(flow.parameter_flows, vec!["user", "base_amount"]);
    assert!(flow.return_sources.contains(&"total".to_string()));
    assert!(flow.assignments.iter().any(|a| a.to_var == "fee"));
    assert!(flow.assignments.iter().any(|a| a.to_var == "total"));
}

#[test]
fn test_semantic_graph_diff_detects_signature_and_node_changes() {
    let code_v1 = "pub fn execute(id: u32) -> bool { true }";
    let code_v2 = "pub fn execute(id: u32, force: bool) -> bool { false }";

    let pf1 = parse_file("src/lib.rs", GraphiaLanguage::Rust, code_v1);
    let pf2 = parse_file("src/lib.rs", GraphiaLanguage::Rust, code_v2);

    let g1 = build_graph(vec![(
        "src/lib.rs".to_string(),
        Some(GraphiaLanguage::Rust),
        pf1,
    )]);
    let g2 = build_graph(vec![(
        "src/lib.rs".to_string(),
        Some(GraphiaLanguage::Rust),
        pf2,
    )]);

    let diff = diff_graphs(&g1, &g2);
    assert!(!diff.modified_nodes.is_empty() || !diff.added_nodes.is_empty());
}

#[test]
fn test_architecture_boundary_matching_with_exact_layers() {
    let code_a = "pub struct DomainModel;";
    let code_b = "use crate::domain::DomainModel; pub fn use_model() {}";

    let pf_a = parse_file("src/domain/model.rs", GraphiaLanguage::Rust, code_a);
    let pf_b = parse_file("src/app/main.rs", GraphiaLanguage::Rust, code_b);

    let graph = build_graph(vec![
        (
            "src/domain/model.rs".to_string(),
            Some(GraphiaLanguage::Rust),
            pf_a,
        ),
        (
            "src/app/main.rs".to_string(),
            Some(GraphiaLanguage::Rust),
            pf_b,
        ),
    ]);

    let config = ArchitectureRulesConfig {
        layers: vec![
            LayerDefinition {
                name: "domain".into(),
                path_patterns: vec!["src/domain".into()],
                allowed_dependencies: vec![],
            },
            LayerDefinition {
                name: "app".into(),
                path_patterns: vec!["src/app".into()],
                allowed_dependencies: vec!["domain".into()],
            },
        ],
    };

    let report = check_architecture_boundaries(&graph, &config);
    assert!(report.passed);
    assert_eq!(report.violations_count, 0);
}
