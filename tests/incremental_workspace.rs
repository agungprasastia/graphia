use std::fs;
use tempfile::tempdir;

use graphia::daemon::debounce::SemanticAction;
use graphia::incremental::IncrementalWorkspace;
use graphia::model::EdgeKind;

#[test]
fn test_incremental_workspace_applies_deltas_matching_clean_rebuild() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();

    let file_a = root.join("a.rs");
    let file_b = root.join("b.rs");

    fs::write(&file_a, "pub fn foo() {}").expect("write a.rs");
    fs::write(&file_b, "use crate::a::foo;\npub fn bar() { foo(); }").expect("write b.rs");

    let mut ws = IncrementalWorkspace::new(root.to_path_buf()).expect("workspace init");
    assert_eq!(ws.files.len(), 2);
    let _initial_node_count = ws.graph.node_count();

    // Modify a.rs
    fs::write(&file_a, "pub fn foo_modified() {}").expect("modify a.rs");
    let dirty = ws
        .apply_changes(&[SemanticAction::Modified(file_a.clone())])
        .expect("apply modify");
    assert!(dirty);

    // Clean rebuild comparison
    let ws_clean = IncrementalWorkspace::new(root.to_path_buf()).expect("clean workspace");
    assert_eq!(ws.graph.node_count(), ws_clean.graph.node_count());
    assert_eq!(ws.graph.edge_count(), ws_clean.graph.edge_count());
    assert_eq!(ws.graph, ws_clean.graph);
}

#[test]
fn selective_update_reparses_only_modified_file() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    let a = root.join("a.rs");
    let b = root.join("b.rs");
    fs::write(&a, "pub fn a() {}").expect("write a");
    fs::write(&b, "pub fn b() {}").expect("write b");
    let mut ws = IncrementalWorkspace::new(root.to_path_buf()).expect("workspace init");
    fs::write(&a, "pub fn changed() {}").expect("modify a");
    let summary = ws
        .apply_changes_selective(&[SemanticAction::Modified(a)])
        .expect("selective update");
    assert_eq!(summary.files_reparsed, 1);
    assert!(!summary.fallback_used);
    assert_eq!(
        ws.graph,
        IncrementalWorkspace::new(root.to_path_buf())
            .expect("clean")
            .graph
    );
}

#[test]
fn selective_update_leaves_unrelated_module_outside_component() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    let auth = root.join("auth.rs");
    let user = root.join("user.rs");
    let logger = root.join("logger.rs");
    fs::write(&auth, "pub fn authenticate() {}").expect("write auth");
    fs::write(
        &user,
        "use crate::auth::authenticate;\npub fn current_user() { authenticate(); }",
    )
    .expect("write user");
    fs::write(&logger, "pub fn log() {}").expect("write logger");
    let mut ws = IncrementalWorkspace::new(root.to_path_buf()).expect("workspace init");

    fs::write(&auth, "pub fn authenticate_v2() {}").expect("modify auth");
    let summary = ws
        .apply_changes_selective(&[SemanticAction::Modified(auth)])
        .expect("selective update");

    assert_eq!(summary.files_reparsed, 1);
    assert!(!summary.full_rebuild);
    assert!(!summary.fallback_used);
    assert!(!summary.affected_files.contains("logger.rs"));
    assert!(ws.file_nodes.contains_key("logger.rs"));
    assert_eq!(
        ws.graph,
        IncrementalWorkspace::new(root.to_path_buf())
            .expect("clean")
            .graph
    );
}

#[test]
fn unknown_rename_records_explicit_fallback_reason() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    fs::write(root.join("a.rs"), "pub fn a() {}").expect("write a");
    let mut ws = IncrementalWorkspace::new(root.to_path_buf()).expect("workspace init");
    let summary = ws
        .apply_changes_selective(&[SemanticAction::Renamed {
            from: root.join("missing.rs"),
            to: root.join("new.rs"),
        }])
        .expect("fallback update");
    assert!(summary.fallback_used);
    assert!(
        summary
            .fallback_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("unknown rename"))
    );
    assert_eq!(ws.fallback_reconcile_count, 1);
}

#[test]
fn new_dependency_matches_clean_graph_without_full_rebuild() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    let a = root.join("a.rs");
    let b = root.join("b.rs");
    let c = root.join("c.rs");
    fs::write(&a, "pub fn process() {}").expect("write a");
    fs::write(&b, "pub struct User;").expect("write b");
    fs::write(&c, "pub fn unrelated() {}").expect("write c");
    let mut ws = IncrementalWorkspace::new(root.to_path_buf()).expect("workspace init");

    fs::write(
        &a,
        "use crate::b::User;\npub fn process(user: User) { let _value = user; }",
    )
    .expect("modify a");
    let summary = ws
        .apply_changes_selective(&[SemanticAction::Modified(a)])
        .expect("selective update");

    assert!(!summary.full_rebuild);
    assert!(!summary.fallback_used);
    assert_eq!(summary.files_reparsed, 1);
    assert!(!summary.affected_files.contains("c.rs"));
    let clean = IncrementalWorkspace::new(root.to_path_buf()).expect("clean workspace");
    assert_eq!(ws.graph, clean.graph);
}

#[test]
fn new_semantic_dependencies_match_clean_without_fallback() {
    let cases = [
        (
            "reference",
            "a.rs",
            "b.rs",
            "pub fn process() { User; }",
            EdgeKind::References,
        ),
        (
            "type",
            "a.rs",
            "b.rs",
            "use crate::b::User;\npub fn process(value: User) {\n    let _ = value;\n}",
            EdgeKind::TypeReferences,
        ),
        (
            "implements",
            "a.rs",
            "b.rs",
            "struct Service; impl Repository for Service {}",
            EdgeKind::Implements,
        ),
        (
            "instantiates",
            "a.rs",
            "b.rs",
            "pub fn process() { User::new(); }",
            EdgeKind::Instantiates,
        ),
        (
            "inherits",
            "a.cs",
            "b.cs",
            "class Child : Parent {}",
            EdgeKind::Inherits,
        ),
        (
            "reexport",
            "a.rs",
            "b.rs",
            "pub use crate::b::User;",
            EdgeKind::Exports,
        ),
    ];
    for (name, a_name, b_name, edited, expected_kind) in cases {
        let temp = tempdir().expect("tempdir");
        let root = temp.path();
        let a = root.join(a_name);
        let b = root.join(b_name);
        let unrelated = root.join("unrelated.rs");
        fs::write(
            &a,
            if a_name.ends_with(".cs") {
                "class Child {}"
            } else {
                "pub fn process() {}"
            },
        )
        .expect("write a");
        fs::write(
            &b,
            if b_name.ends_with(".cs") {
                "class Parent {}"
            } else {
                "pub struct User; pub trait Repository {}"
            },
        )
        .expect("write b");
        fs::write(&unrelated, "pub fn untouched() {}").expect("write unrelated");
        let mut ws = IncrementalWorkspace::new(root.to_path_buf()).expect("workspace init");
        fs::write(&a, edited).expect("edit a");
        let summary = ws
            .apply_changes_selective(&[SemanticAction::Modified(a)])
            .expect("selective update");
        assert!(!summary.full_rebuild, "{name} used full rebuild");
        assert!(!summary.fallback_used, "{name} used fallback");
        assert_eq!(summary.files_reparsed, 1, "{name} reparsed unrelated files");
        assert!(
            !summary.affected_files.contains("unrelated.rs"),
            "{name} expanded unrelated file"
        );
        let clean = IncrementalWorkspace::new(root.to_path_buf()).expect("clean workspace");
        assert_eq!(ws.graph, clean.graph, "{name} differs from clean graph");
        assert_eq!(
            ws.graph.edges.iter().any(|edge| edge.kind == expected_kind),
            clean
                .graph
                .edges
                .iter()
                .any(|edge| edge.kind == expected_kind),
            "{name} relation differs from clean graph"
        );
    }
}
