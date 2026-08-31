use graphia::analysis::advanced::boundaries::{
    ArchitectureRulesConfig, LayerDefinition, check_architecture_boundaries,
};
use graphia::analysis::advanced::diff::diff_graphs;
use graphia::analysis::advanced::typeflow::{
    extract_intraprocedural_typeflow, extract_local_flow_graph,
};
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
fn test_local_flow_extracts_exact_parameter_types_constructor_and_chain() {
    let source = "fn save(user: User, db: &Database) { let db = Database::new(); let x = user; let y = x; return y; }";
    let flow = extract_local_flow_graph("save", "src/save.rs", source, 1);
    let parameters = flow
        .parameters
        .iter()
        .map(|p| (p.name.as_str(), p.type_name.as_deref()))
        .collect::<Vec<_>>();
    assert_eq!(
        parameters,
        vec![("user", Some("User")), ("db", Some("&Database"))]
    );
    assert!(flow.bindings.iter().any(|binding| binding.name == "db"));
    assert!(
        flow.assignments
            .iter()
            .any(|assignment| assignment.to == "db" && assignment.from.contains("Database"))
    );
    assert!(
        flow.assignments
            .windows(2)
            .any(|chain| chain[0].from == "user"
                && chain[0].to == "x"
                && chain[1].from == "x"
                && chain[1].to == "y")
    );
    assert!(flow.returns.iter().any(|ret| ret.source == "y"));
}

#[test]
fn test_local_flow_tracks_shadowed_bindings_by_scope() {
    let source =
        "fn shadow(input: User) { let value = input; { let value = input; return value; } }";
    let flow = extract_local_flow_graph("shadow", "src/shadow.rs", source, 1);
    let values = flow
        .bindings
        .iter()
        .filter(|binding| binding.name == "value")
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 2);
    assert_ne!(values[0].scope_id, values[1].scope_id);
}

#[test]
fn test_local_flow_extracts_call_arguments_and_return_binding() {
    let source = "fn caller(input: User) { let result = save(input); return result; }";
    let flow = extract_local_flow_graph("caller", "src/caller.rs", source, 1);
    assert!(
        flow.call_arguments
            .iter()
            .any(|argument| argument.call == "save"
                && argument.argument == "input"
                && argument.index == 0)
    );
    assert!(
        flow.assignments
            .iter()
            .any(|assignment| assignment.to == "result" && assignment.from.contains("save"))
    );
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
