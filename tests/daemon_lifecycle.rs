use std::fs;
use tempfile::tempdir;

use graphia::daemon::debounce::SemanticAction;
use graphia::daemon::server::{DaemonConfig, DaemonServer};
use graphia::daemon::state::DaemonHealth;

#[test]
fn test_daemon_lifecycle_health_and_delta_processing() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();

    let file_a = root.join("a.rs");
    fs::write(&file_a, "pub fn foo() {}").expect("write a.rs");

    let config = DaemonConfig {
        repo_root: root.to_path_buf(),
        debounce_duration: std::time::Duration::from_millis(50),
        queue_capacity: 100,
        persistence_interval: std::time::Duration::from_millis(50),
    };

    let server = DaemonServer::new(config).expect("server init");
    let manager = server.state_manager();

    assert_eq!(manager.health(), DaemonHealth::Healthy);
    let initial_gen = manager.read_snapshot().generation;

    // Apply incremental semantic action directly
    fs::write(&file_a, "pub fn foo_updated() {}").expect("modify a.rs");
    let dirty = manager
        .apply_actions(&[SemanticAction::Modified(file_a)])
        .expect("apply action");
    assert!(dirty);

    let next_gen = manager.read_snapshot().generation;
    assert!(next_gen > initial_gen);
    assert_eq!(manager.health(), DaemonHealth::Healthy);
}

#[test]
fn failed_update_preserves_snapshot_and_generation() {
    let temp = tempdir().expect("tempdir");
    let file = temp.path().join("broken.rs");
    fs::write(&file, "pub fn stable() {}").expect("write source");
    let manager = DaemonServer::new(DaemonConfig {
        repo_root: temp.path().to_path_buf(),
        ..DaemonConfig::default()
    })
    .expect("server init")
    .state_manager();
    let before = manager.read_snapshot();
    fs::remove_file(&file).expect("remove source");
    fs::create_dir(&file).expect("replace source with directory");
    let result = manager.apply_actions(&[SemanticAction::Modified(file)]);
    assert!(result.is_err());
    assert_eq!(manager.read_snapshot().generation, before.generation);
    assert_eq!(manager.health(), DaemonHealth::Dirty);
    assert!(manager.last_error().is_some());
}
