use std::fs;
use tempfile::tempdir;

use graphia::daemon::debounce::SemanticAction;
use graphia::incremental::IncrementalWorkspace;

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
