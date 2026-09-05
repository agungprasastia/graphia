use std::fs;

use graphia::incremental::{ChangeKind, classify_changes, update_repository};
use graphia::scan::scan_repo;
use graphia::storage::{build_graph_from_repo, load_graph_json, save_graph_json};
use tempfile::tempdir;

#[test]
fn incremental_update_matches_clean_rebuild_after_file_changes() {
    let root = tempdir().expect("tempdir");
    fs::write(root.path().join("a.rs"), "use b::b; fn a() { b(); }").expect("write a");
    fs::write(root.path().join("b.rs"), "pub fn b() {}").expect("write b");

    update_repository(root.path()).expect("initial update");
    fs::write(root.path().join("b.rs"), "pub fn changed() {}").expect("modify b");
    fs::write(root.path().join("c.rs"), "pub fn c() {}").expect("add c");
    fs::remove_file(root.path().join("a.rs")).expect("delete a");

    let incremental = update_repository(root.path()).expect("incremental update");
    let clean = build_graph_from_repo(root.path()).expect("clean rebuild");
    assert_eq!(incremental, clean);
}

#[test]
fn classification_uses_content_hash_not_mtime() {
    let root = tempdir().expect("tempdir");
    fs::write(root.path().join("a.rs"), "fn a() {}").expect("write a");
    let first = scan_repo(root.path()).expect("scan");
    let metadata = graphia::storage::metadata_for_files(&first).expect("metadata");
    fs::write(root.path().join("a.rs"), "fn changed() {}").expect("modify a");
    let second = scan_repo(root.path()).expect("scan");
    let changes = classify_changes(&metadata.files, &second).expect("classify");
    assert_eq!(changes[0].kind, ChangeKind::Modified);
}

#[test]
fn persisted_incremental_graph_is_canonical_json() {
    let root = tempdir().expect("tempdir");
    fs::write(root.path().join("a.rs"), "fn a() {}").expect("write a");
    update_repository(root.path()).expect("update");
    assert!(!root.path().join("graph.json").exists());
    let graph = load_graph_json(&root.path().join(".graphia/graph.json")).expect("load graph");
    let copy = root.path().join("copy.json");
    save_graph_json(&graph, &copy).expect("save copy");
    assert_eq!(
        fs::read(root.path().join(".graphia/graph.json")).expect("read graph"),
        fs::read(copy).expect("read copy")
    );
}

#[test]
fn unchanged_update_reuses_valid_cache_and_schema_mismatch_rebuilds() {
    let root = tempdir().expect("tempdir");
    fs::write(root.path().join("a.rs"), "fn a() {}").expect("write a");
    update_repository(root.path()).expect("initial update");
    let first =
        load_graph_json(&root.path().join(".graphia/graph.json")).expect("load first graph");
    let cache = root.path().join(".graphia/parsed.json");
    let mut cache_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&cache).expect("read cache"))
            .expect("parse cache");
    cache_json["files"][0]["parsed"]["symbols"][0]["name"] = serde_json::json!("cache_only");
    fs::write(
        &cache,
        serde_json::to_vec(&cache_json).expect("serialize cache"),
    )
    .expect("write cache");
    let reused = update_repository(root.path()).expect("reuse valid cache");
    assert!(reused.nodes.iter().any(|node| node.name == "cache_only"));
    cache_json["schema_version"] = serde_json::json!(999);
    fs::write(
        &cache,
        serde_json::to_vec(&cache_json).expect("serialize cache"),
    )
    .expect("corrupt cache schema");
    let rebuilt = update_repository(root.path()).expect("rebuild mismatched cache");
    assert_eq!(rebuilt, first);
}

#[test]
fn malformed_duplicate_cache_paths_are_ignored() {
    let root = tempdir().expect("tempdir");
    fs::write(root.path().join("a.rs"), "fn a() {}").expect("write a");
    let expected = update_repository(root.path()).expect("initial update");
    let cache = root.path().join(".graphia/parsed.json");
    let mut cache_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&cache).expect("read cache"))
            .expect("parse cache");
    let duplicate = cache_json["files"][0].clone();
    cache_json["files"]
        .as_array_mut()
        .expect("cache files")
        .push(duplicate);
    fs::write(
        &cache,
        serde_json::to_vec(&cache_json).expect("serialize cache"),
    )
    .expect("write cache");
    let rebuilt = update_repository(root.path()).expect("ignore malformed cache");
    assert_eq!(rebuilt, expected);
}

#[test]
fn malformed_cache_json_triggers_clean_reparse() {
    let root = tempdir().expect("tempdir");
    fs::write(root.path().join("a.rs"), "fn a() {}").expect("write a");
    let expected = build_graph_from_repo(root.path()).expect("clean graph");
    fs::create_dir_all(root.path().join(".graphia")).expect("create storage directory");
    fs::write(root.path().join(".graphia/parsed.json"), b"not json")
        .expect("write malformed cache");
    let rebuilt = update_repository(root.path()).expect("recover malformed cache");
    assert_eq!(rebuilt, expected);
}

#[test]
fn incremental_and_clean_persisted_bytes_match() {
    let incremental_root = tempdir().expect("incremental tempdir");
    let clean_root = tempdir().expect("clean tempdir");
    for root in [incremental_root.path(), clean_root.path()] {
        fs::write(root.join("a.rs"), "use b::b; fn a() { b(); }").expect("write a");
        fs::write(root.join("b.rs"), "pub fn b() {}").expect("write b");
    }
    update_repository(incremental_root.path()).expect("initial update");
    fs::write(incremental_root.path().join("b.rs"), "pub fn changed() {}")
        .expect("modify incremental");
    fs::remove_file(incremental_root.path().join("a.rs")).expect("delete incremental");
    fs::write(incremental_root.path().join("c.rs"), "pub fn c() {}").expect("add incremental");
    update_repository(incremental_root.path()).expect("incremental update");

    fs::remove_file(clean_root.path().join("a.rs")).expect("delete clean");
    fs::write(clean_root.path().join("b.rs"), "pub fn changed() {}").expect("modify clean");
    fs::write(clean_root.path().join("c.rs"), "pub fn c() {}").expect("add clean");
    let clean_graph = build_graph_from_repo(clean_root.path()).expect("clean rebuild");
    graphia::storage::save_graph_json(&clean_graph, &clean_root.path().join("graph.json"))
        .expect("save clean JSON");
    graphia::storage::save_graph_binary(
        &clean_graph,
        &clean_root.path().join(".graphia/index.bin"),
    )
    .expect("save clean binary");
    assert_eq!(
        fs::read(incremental_root.path().join(".graphia/graph.json"))
            .expect("read incremental JSON"),
        fs::read(clean_root.path().join("graph.json")).expect("read clean JSON")
    );
    assert_eq!(
        fs::read(incremental_root.path().join(".graphia/index.bin"))
            .expect("read incremental binary"),
        fs::read(clean_root.path().join(".graphia/index.bin")).expect("read clean binary")
    );
}

#[test]
fn clean_build_removes_stale_cache_and_metadata_schema_classifies_cleanly() {
    let root = tempdir().expect("tempdir");
    fs::write(root.path().join("a.rs"), "fn a() {}").expect("write a");
    update_repository(root.path()).expect("initial update");
    let cache = root.path().join(".graphia/parsed.json");
    assert!(cache.exists());
    let mut metadata: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.path().join(".graphia/metadata.json")).expect("read metadata"),
    )
    .expect("parse metadata");
    metadata["schema_version"] = serde_json::json!(999);
    fs::write(
        root.path().join(".graphia/metadata.json"),
        serde_json::to_vec(&metadata).expect("serialize metadata"),
    )
    .expect("corrupt metadata schema");
    let (_, changes) = graphia::storage::build_or_update(root.path(), true).expect("clean build");
    let rebuilt_cache: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(cache).expect("read rebuilt cache"))
            .expect("parse rebuilt cache");
    assert_eq!(rebuilt_cache["schema_version"], serde_json::json!(2));
    assert!(changes.iter().any(|change| {
        change.path == "a.rs" && change.change == graphia::storage::FileChange::Added
    }));
}
