use std::fs;
use tempfile::tempdir;

use graphia::daemon::debounce::SemanticAction;
use graphia::graph::Graph;
use graphia::incremental::IncrementalUpdateSummary;
use graphia::incremental::IncrementalWorkspace;
use graphia::model::EdgeKind;
use graphia::resolve::{Resolution, ResolutionEngine};

fn has_relation_from_file_to_name(
    graph: &Graph,
    file: &str,
    kind: EdgeKind,
    target_name: &str,
) -> bool {
    graph.edges.iter().any(|edge| {
        edge.kind == kind
            && graph
                .nodes
                .iter()
                .find(|node| node.id == edge.from)
                .is_some_and(|node| node.file == file)
            && graph
                .nodes
                .iter()
                .find(|node| node.id == edge.to)
                .is_some_and(|node| node.name == target_name)
    })
}

fn assert_canonical_equivalent(incremental: &IncrementalWorkspace, clean: &IncrementalWorkspace) {
    incremental.graph.validate().expect("incremental canonical");
    clean.graph.validate().expect("clean canonical");
    assert_eq!(incremental.graph, clean.graph);
}

fn assert_selective_resolution(
    summary: &IncrementalUpdateSummary,
    consumer: &str,
    unrelated: &str,
) {
    assert_eq!(summary.files_reparsed, 1);
    assert!(!summary.full_rebuild);
    assert!(!summary.fallback_used);
    assert!(summary.affected_files.contains(consumer));
    assert!(!summary.affected_files.contains(unrelated));
    assert_eq!(summary.full_pending_scans, 0);
}

fn isolate_resolution_indexes(workspace: &mut IncrementalWorkspace) {
    workspace.import_dependents.clear();
    workspace.type_dependents.clear();
}

fn assert_pending_index_woke_consumer(summary: &IncrementalUpdateSummary) {
    assert!(summary.pending_index_lookups > 0);
    assert!(summary.pending_entries_examined > 0);
}

fn assert_resolution_index_woke_consumer(summary: &IncrementalUpdateSummary) {
    assert!(summary.resolution_index_lookups > 0);
    assert!(summary.resolution_entries_examined > 0);
}

fn assert_positive_relation(
    incremental: &IncrementalWorkspace,
    clean: &IncrementalWorkspace,
    consumer: &str,
    kind: EdgeKind,
    target: &str,
) {
    let clean_has_expected_edge =
        has_relation_from_file_to_name(&clean.graph, consumer, kind, target);
    let incremental_has_expected_edge =
        has_relation_from_file_to_name(&incremental.graph, consumer, kind, target);
    let describe = |graph: &Graph| {
        graph
            .edges
            .iter()
            .filter(|edge| edge.kind == kind)
            .map(|edge| {
                let from = graph.nodes.iter().find(|node| node.id == edge.from);
                let to = graph.nodes.iter().find(|node| node.id == edge.to);
                format!(
                    "{}:{} -> {}:{} ({:?})",
                    from.map_or("?", |node| node.file.as_str()),
                    from.map_or("?", |node| node.name.as_str()),
                    to.map_or("?", |node| node.file.as_str()),
                    to.map_or("?", |node| node.name.as_str()),
                    edge.label
                )
            })
            .collect::<Vec<_>>()
    };
    assert!(
        clean_has_expected_edge,
        "clean missing {kind:?} to {target}: {:?}",
        describe(&clean.graph)
    );
    assert!(
        incremental_has_expected_edge,
        "incremental missing {kind:?} to {target}: {:?}",
        describe(&incremental.graph)
    );
    assert_canonical_equivalent(incremental, clean);
}

fn resolve_type_reference(workspace: &IncrementalWorkspace, file: &str, name: &str) -> Resolution {
    let files = workspace
        .files
        .iter()
        .map(|(path, (language, parsed))| (path.clone(), *language, parsed.clone()))
        .collect::<Vec<_>>();
    let mut engine = ResolutionEngine::new();
    engine.index_files(&workspace.graph.nodes, &files);
    engine.resolve_type_reference(file, name)
}

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
        assert!(
            clean
                .graph
                .edges
                .iter()
                .any(|edge| edge.kind == expected_kind),
            "{name} clean graph missing expected relation"
        );
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

#[test]
fn new_definition_resolves_existing_unresolved_reference_selectively() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    fs::create_dir_all(root.join("src")).expect("src");
    let a = root.join("src/a.rs");
    let b = root.join("src/b.rs");
    fs::write(&a, "pub fn unrelated() {}").expect("write a");
    fs::write(&b, "pub fn use_value() { MissingType; }").expect("write b");
    let mut ws = IncrementalWorkspace::new(root.to_path_buf()).expect("workspace init");
    fs::write(&a, "pub struct MissingType;").expect("define type");
    let summary = ws
        .apply_changes_selective(&[SemanticAction::Modified(a)])
        .expect("selective update");
    assert_eq!(summary.files_reparsed, 1);
    assert!(!summary.full_rebuild);
    assert!(!summary.fallback_used);
    assert!(summary.affected_files.contains("src/b.rs"));
    let clean = IncrementalWorkspace::new(root.to_path_buf()).expect("clean workspace");
    assert_eq!(ws.graph, clean.graph);
    assert!(
        clean
            .graph
            .edges
            .iter()
            .any(|edge| edge.kind == EdgeKind::References)
    );
}

#[test]
fn reverse_unresolved_references_resolve_without_consumer_reparse() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    let provider = root.join("provider.py");
    let consumer = root.join("consumer.py");
    fs::write(&provider, "def existing():\n    pass\n").expect("provider");
    fs::write(
        &consumer,
        "from provider import missing_value\ndef consume():\n    return missing_value\n",
    )
    .expect("consumer");
    fs::write(root.join("unrelated.py"), "def untouched():\n    pass\n").expect("unrelated");
    let mut incremental = IncrementalWorkspace::new(root.to_path_buf()).expect("initial");
    assert!(!has_relation_from_file_to_name(
        &incremental.graph,
        "consumer.py",
        EdgeKind::References,
        "missing_value"
    ));

    isolate_resolution_indexes(&mut incremental);
    fs::write(&provider, "def missing_value():\n    pass\n").expect("edit provider");
    let summary = incremental
        .apply_changes_selective(&[SemanticAction::Modified(provider)])
        .expect("incremental");
    let clean = IncrementalWorkspace::new(root.to_path_buf()).expect("clean");

    assert_selective_resolution(&summary, "consumer.py", "unrelated.py");
    assert_pending_index_woke_consumer(&summary);
    assert_positive_relation(
        &incremental,
        &clean,
        "consumer.py",
        EdgeKind::References,
        "missing_value",
    );
}

#[test]
fn created_module_wakes_pending_import_without_workspace_scan() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    let provider = root.join("missing.ts");
    fs::write(
        root.join("consumer.ts"),
        "import './missing'; export function consume() {}",
    )
    .expect("consumer");
    fs::write(root.join("unrelated.ts"), "export function untouched() {}").expect("unrelated");
    let mut incremental = IncrementalWorkspace::new(root.to_path_buf()).expect("initial");
    assert!(!has_relation_from_file_to_name(
        &incremental.graph,
        "consumer.ts",
        EdgeKind::Imports,
        "missing.ts"
    ));

    isolate_resolution_indexes(&mut incremental);
    fs::write(&provider, "export {};").expect("create provider");
    let summary = incremental
        .apply_changes_selective(&[SemanticAction::Created(provider)])
        .expect("incremental");
    let clean = IncrementalWorkspace::new(root.to_path_buf()).expect("clean");

    assert_selective_resolution(&summary, "consumer.ts", "unrelated.ts");
    assert_pending_index_woke_consumer(&summary);
    assert_positive_relation(
        &incremental,
        &clean,
        "consumer.ts",
        EdgeKind::Imports,
        "missing.ts",
    );
}

#[test]
fn explicit_extension_import_does_not_expand_same_extension_files() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    let consumer = root.join("consumer.ts");
    fs::write(root.join("foo.ts"), "export {};").expect("target");
    fs::write(root.join("bar.ts"), "export {};").expect("unrelated same extension");
    fs::write(&consumer, "export function consume() {}").expect("consumer");
    let mut incremental = IncrementalWorkspace::new(root.to_path_buf()).expect("initial");

    fs::write(&consumer, "import './foo.ts'; export function consume() {}").expect("edit consumer");
    let summary = incremental
        .apply_changes_selective(&[SemanticAction::Modified(consumer)])
        .expect("incremental");
    let clean = IncrementalWorkspace::new(root.to_path_buf()).expect("clean");

    assert_eq!(summary.files_reparsed, 1);
    assert!(!summary.full_rebuild);
    assert!(!summary.fallback_used);
    assert!(summary.affected_files.contains("foo.ts"));
    assert!(!summary.affected_files.contains("bar.ts"));
    assert_positive_relation(
        &incremental,
        &clean,
        "consumer.ts",
        EdgeKind::Imports,
        "foo.ts",
    );
}

#[test]
fn reverse_unresolved_type_references_resolve_without_consumer_reparse() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    let provider = root.join("provider.ts");
    let consumer = root.join("consumer.ts");
    fs::write(&provider, "export class Existing {}").expect("provider");
    fs::write(
        &consumer,
        "import { MissingType } from './provider'; export function consume(value: MissingType) {}",
    )
    .expect("consumer");
    fs::write(root.join("unrelated.ts"), "export function untouched() {}").expect("unrelated");
    let mut incremental = IncrementalWorkspace::new(root.to_path_buf()).expect("initial");
    assert!(!has_relation_from_file_to_name(
        &incremental.graph,
        "consumer.ts",
        EdgeKind::TypeReferences,
        "MissingType"
    ));

    isolate_resolution_indexes(&mut incremental);
    fs::write(&provider, "export class MissingType {}").expect("edit provider");
    let summary = incremental
        .apply_changes_selective(&[SemanticAction::Modified(provider)])
        .expect("incremental");
    let clean = IncrementalWorkspace::new(root.to_path_buf()).expect("clean");

    assert_selective_resolution(&summary, "consumer.ts", "unrelated.ts");
    assert_pending_index_woke_consumer(&summary);
    assert_positive_relation(
        &incremental,
        &clean,
        "consumer.ts",
        EdgeKind::TypeReferences,
        "MissingType",
    );
}

#[test]
fn reverse_unresolved_calls_resolve_and_leave_pending_index() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    let provider = root.join("provider.rs");
    let consumer = root.join("consumer.rs");
    fs::write(&provider, "pub fn existing() {}").expect("provider");
    fs::write(
        &consumer,
        "use crate::provider::missing_function; pub fn consumer() { missing_function(); }",
    )
    .expect("consumer");
    fs::write(root.join("unrelated.rs"), "pub fn untouched() {}").expect("unrelated");
    let mut incremental = IncrementalWorkspace::new(root.to_path_buf()).expect("initial");
    let pending_key = (EdgeKind::Calls.code(), "missing_function".to_string());
    assert!(!has_relation_from_file_to_name(
        &incremental.graph,
        "consumer.rs",
        EdgeKind::Calls,
        "missing_function"
    ));
    assert!(
        incremental
            .pending_resolution
            .get(&pending_key)
            .is_some_and(|files| files.contains("consumer.rs"))
    );

    isolate_resolution_indexes(&mut incremental);
    fs::write(&provider, "pub fn missing_function() {}").expect("edit provider");
    let summary = incremental
        .apply_changes_selective(&[SemanticAction::Modified(provider)])
        .expect("incremental");
    let clean = IncrementalWorkspace::new(root.to_path_buf()).expect("clean");

    assert_selective_resolution(&summary, "consumer.rs", "unrelated.rs");
    assert_pending_index_woke_consumer(&summary);
    assert_positive_relation(
        &incremental,
        &clean,
        "consumer.rs",
        EdgeKind::Calls,
        "missing_function",
    );
    assert!(
        incremental
            .pending_resolution
            .get(&pending_key)
            .is_none_or(|files| !files.contains("consumer.rs"))
    );
}

#[test]
fn reverse_unresolved_inherits_resolves_without_consumer_reparse() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    let provider = root.join("provider.ts");
    let consumer = root.join("consumer.ts");
    fs::write(&provider, "export class Existing {}").expect("provider");
    fs::write(
        &consumer,
        "import { MissingParent } from './provider'; export class Child extends MissingParent {}",
    )
    .expect("consumer");
    fs::write(root.join("unrelated.ts"), "export class Untouched {}").expect("unrelated");
    let mut incremental = IncrementalWorkspace::new(root.to_path_buf()).expect("initial");
    assert!(!has_relation_from_file_to_name(
        &incremental.graph,
        "consumer.ts",
        EdgeKind::Inherits,
        "MissingParent"
    ));

    isolate_resolution_indexes(&mut incremental);
    fs::write(&provider, "export class MissingParent {}").expect("edit provider");
    let summary = incremental
        .apply_changes_selective(&[SemanticAction::Modified(provider)])
        .expect("incremental");
    let clean = IncrementalWorkspace::new(root.to_path_buf()).expect("clean");

    assert_selective_resolution(&summary, "consumer.ts", "unrelated.ts");
    assert_pending_index_woke_consumer(&summary);
    assert_positive_relation(
        &incremental,
        &clean,
        "consumer.ts",
        EdgeKind::Inherits,
        "MissingParent",
    );
}

#[test]
fn reverse_unresolved_implements_resolves_without_consumer_reparse() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    let provider = root.join("provider.ts");
    let consumer = root.join("consumer.ts");
    fs::write(&provider, "export interface Existing {}").expect("provider");
    fs::write(
        &consumer,
        "import { MissingInterface } from './provider'; export class Worker implements MissingInterface { run() {} }",
    )
    .expect("consumer");
    fs::write(root.join("unrelated.ts"), "export class Untouched {}").expect("unrelated");
    let mut incremental = IncrementalWorkspace::new(root.to_path_buf()).expect("initial");
    assert!(!has_relation_from_file_to_name(
        &incremental.graph,
        "consumer.ts",
        EdgeKind::Implements,
        "MissingInterface"
    ));

    isolate_resolution_indexes(&mut incremental);
    fs::write(&provider, "export interface MissingInterface {}").expect("edit provider");
    let summary = incremental
        .apply_changes_selective(&[SemanticAction::Modified(provider)])
        .expect("incremental");
    let clean = IncrementalWorkspace::new(root.to_path_buf()).expect("clean");

    assert_selective_resolution(&summary, "consumer.ts", "unrelated.ts");
    assert_pending_index_woke_consumer(&summary);
    assert_positive_relation(
        &incremental,
        &clean,
        "consumer.ts",
        EdgeKind::Implements,
        "MissingInterface",
    );
}

#[test]
fn reverse_unresolved_instantiates_resolves_without_consumer_reparse() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    let provider = root.join("provider.ts");
    let consumer = root.join("consumer.ts");
    fs::write(&provider, "export class Existing {}").expect("provider");
    fs::write(
        &consumer,
        "import { MissingType } from './provider'; export function make() { return new MissingType(); }",
    )
    .expect("consumer");
    fs::write(root.join("unrelated.ts"), "export function untouched() {}").expect("unrelated");
    let mut incremental = IncrementalWorkspace::new(root.to_path_buf()).expect("initial");
    assert!(!has_relation_from_file_to_name(
        &incremental.graph,
        "consumer.ts",
        EdgeKind::Instantiates,
        "MissingType"
    ));

    isolate_resolution_indexes(&mut incremental);
    fs::write(&provider, "export class MissingType {}").expect("edit provider");
    let summary = incremental
        .apply_changes_selective(&[SemanticAction::Modified(provider)])
        .expect("incremental");
    let clean = IncrementalWorkspace::new(root.to_path_buf()).expect("clean");

    assert_selective_resolution(&summary, "consumer.ts", "unrelated.ts");
    assert_pending_index_woke_consumer(&summary);
    assert_positive_relation(
        &incremental,
        &clean,
        "consumer.ts",
        EdgeKind::Instantiates,
        "MissingType",
    );
}

#[test]
fn facade_reexport_wakes_waiting_consumer_without_reparse() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    let facade = root.join("facade.ts");
    fs::write(root.join("target.ts"), "export class Foo {}").expect("target");
    fs::write(&facade, "export function placeholder() {}").expect("facade");
    fs::write(
        root.join("consumer.ts"),
        "import { Foo } from './facade'; export function consume(value: Foo) {}",
    )
    .expect("consumer");
    fs::write(root.join("unrelated.ts"), "export function untouched() {}").expect("unrelated");
    let mut incremental = IncrementalWorkspace::new(root.to_path_buf()).expect("initial");
    assert!(!has_relation_from_file_to_name(
        &incremental.graph,
        "consumer.ts",
        EdgeKind::TypeReferences,
        "Foo"
    ));

    isolate_resolution_indexes(&mut incremental);
    fs::write(&facade, "export { Foo } from './target';").expect("edit facade");
    let summary = incremental
        .apply_changes_selective(&[SemanticAction::Modified(facade)])
        .expect("incremental");
    let clean = IncrementalWorkspace::new(root.to_path_buf()).expect("clean");

    assert_selective_resolution(&summary, "consumer.ts", "unrelated.ts");
    assert_pending_index_woke_consumer(&summary);
    assert_positive_relation(
        &incremental,
        &clean,
        "consumer.ts",
        EdgeKind::TypeReferences,
        "Foo",
    );
    assert!(has_relation_from_file_to_name(
        &clean.graph,
        "facade.ts",
        EdgeKind::Exports,
        "Foo"
    ));
    assert!(has_relation_from_file_to_name(
        &incremental.graph,
        "facade.ts",
        EdgeKind::Exports,
        "Foo"
    ));
}

#[test]
fn resolved_reference_becomes_ambiguous_when_equal_candidate_is_added() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    let provider_b = root.join("b.ts");
    fs::write(root.join("a.ts"), "export class Foo {}").expect("a");
    fs::write(&provider_b, "export class Other {}").expect("b");
    fs::write(
        root.join("consumer.ts"),
        "import { Foo } from './a'; import { Foo } from './b'; export function consume(value: Foo) {}",
    )
    .expect("consumer");
    fs::write(root.join("unrelated.ts"), "export function untouched() {}").expect("unrelated");
    let mut incremental = IncrementalWorkspace::new(root.to_path_buf()).expect("initial");
    assert!(resolve_type_reference(&incremental, "consumer.ts", "Foo").is_resolved());
    assert!(has_relation_from_file_to_name(
        &incremental.graph,
        "consumer.ts",
        EdgeKind::TypeReferences,
        "Foo"
    ));

    isolate_resolution_indexes(&mut incremental);
    fs::write(&provider_b, "export class Foo {}").expect("add equal candidate");
    let summary = incremental
        .apply_changes_selective(&[SemanticAction::Modified(provider_b)])
        .expect("incremental");
    let clean = IncrementalWorkspace::new(root.to_path_buf()).expect("clean");

    assert_selective_resolution(&summary, "consumer.ts", "unrelated.ts");
    assert_resolution_index_woke_consumer(&summary);
    assert!(resolve_type_reference(&clean, "consumer.ts", "Foo").is_ambiguous());
    assert!(resolve_type_reference(&incremental, "consumer.ts", "Foo").is_ambiguous());
    assert!(!has_relation_from_file_to_name(
        &clean.graph,
        "consumer.ts",
        EdgeKind::TypeReferences,
        "Foo"
    ));
    assert!(!has_relation_from_file_to_name(
        &incremental.graph,
        "consumer.ts",
        EdgeKind::TypeReferences,
        "Foo"
    ));
    assert_canonical_equivalent(&incremental, &clean);
}

#[test]
fn resolved_reference_becomes_unresolved_when_candidate_is_removed() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    let provider = root.join("a.ts");
    fs::write(&provider, "export class Foo {}").expect("provider");
    fs::write(
        root.join("consumer.ts"),
        "import { Foo } from './a'; export function consume(value: Foo) {}",
    )
    .expect("consumer");
    fs::write(root.join("unrelated.ts"), "export function untouched() {}").expect("unrelated");
    let mut incremental = IncrementalWorkspace::new(root.to_path_buf()).expect("initial");
    assert!(resolve_type_reference(&incremental, "consumer.ts", "Foo").is_resolved());
    assert!(has_relation_from_file_to_name(
        &incremental.graph,
        "consumer.ts",
        EdgeKind::TypeReferences,
        "Foo"
    ));

    isolate_resolution_indexes(&mut incremental);
    fs::write(&provider, "export class Other {}").expect("remove candidate");
    let summary = incremental
        .apply_changes_selective(&[SemanticAction::Modified(provider)])
        .expect("incremental");
    let clean = IncrementalWorkspace::new(root.to_path_buf()).expect("clean");

    assert_selective_resolution(&summary, "consumer.ts", "unrelated.ts");
    assert_resolution_index_woke_consumer(&summary);
    assert!(resolve_type_reference(&clean, "consumer.ts", "Foo").is_unresolved());
    assert!(resolve_type_reference(&incremental, "consumer.ts", "Foo").is_unresolved());
    assert!(!has_relation_from_file_to_name(
        &clean.graph,
        "consumer.ts",
        EdgeKind::TypeReferences,
        "Foo"
    ));
    assert!(!has_relation_from_file_to_name(
        &incremental.graph,
        "consumer.ts",
        EdgeKind::TypeReferences,
        "Foo"
    ));
    assert_canonical_equivalent(&incremental, &clean);
}

#[test]
fn ambiguous_reference_becomes_resolved_when_equal_candidate_is_removed() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    let provider_b = root.join("b.ts");
    fs::write(root.join("a.ts"), "export class Foo {}").expect("a");
    fs::write(&provider_b, "export class Foo {}").expect("b");
    fs::write(
        root.join("consumer.ts"),
        "import { Foo } from './a'; import { Foo } from './b'; export function consume(value: Foo) {}",
    )
    .expect("consumer");
    fs::write(root.join("unrelated.ts"), "export function untouched() {}").expect("unrelated");
    let mut incremental = IncrementalWorkspace::new(root.to_path_buf()).expect("initial");
    assert!(resolve_type_reference(&incremental, "consumer.ts", "Foo").is_ambiguous());
    assert!(!has_relation_from_file_to_name(
        &incremental.graph,
        "consumer.ts",
        EdgeKind::TypeReferences,
        "Foo"
    ));

    isolate_resolution_indexes(&mut incremental);
    fs::write(&provider_b, "export class Other {}").expect("remove equal candidate");
    let summary = incremental
        .apply_changes_selective(&[SemanticAction::Modified(provider_b)])
        .expect("incremental");
    let clean = IncrementalWorkspace::new(root.to_path_buf()).expect("clean");

    assert_selective_resolution(&summary, "consumer.ts", "unrelated.ts");
    assert_resolution_index_woke_consumer(&summary);
    assert!(resolve_type_reference(&clean, "consumer.ts", "Foo").is_resolved());
    assert!(resolve_type_reference(&incremental, "consumer.ts", "Foo").is_resolved());
    assert_positive_relation(
        &incremental,
        &clean,
        "consumer.ts",
        EdgeKind::TypeReferences,
        "Foo",
    );
}
