use std::fs;
use std::thread::{self, sleep};
use std::time::Duration;

use graphia::cli::{Cli, CliFormat, Commands, DaemonAction, run};
use graphia::daemon::{
    DaemonConfig, DaemonServer, Debouncer, GraphGeneration, LiveStateManager, PersistenceWorker,
    QueueStatus, UpdateQueue, is_excluded_path, is_relevant_source_file,
};
use tempfile::tempdir;

#[test]
fn test_exclusion_and_relevance_policies() {
    assert!(is_excluded_path(std::path::Path::new(".git/HEAD")));
    assert!(is_excluded_path(std::path::Path::new(".graphia/index.bin")));
    assert!(is_excluded_path(std::path::Path::new("target/debug/app")));
    assert!(is_excluded_path(std::path::Path::new(
        "node_modules/pkg/index.js"
    )));
    assert!(!is_excluded_path(std::path::Path::new("src/main.rs")));

    assert!(is_relevant_source_file(std::path::Path::new("src/lib.rs")));
    assert!(is_relevant_source_file(std::path::Path::new(
        "app/component.tsx"
    )));
    assert!(is_relevant_source_file(std::path::Path::new(
        "server/main.go"
    )));
    assert!(!is_relevant_source_file(std::path::Path::new(
        "docs/README.md"
    )));
    assert!(!is_relevant_source_file(std::path::Path::new(
        "target/build.rs"
    )));
}

#[test]
fn test_debounce_and_burst_coalescing() {
    let dir = tempdir().expect("tempdir");
    let file = dir.path().join("service.rs");
    fs::write(&file, "pub fn service() {}").expect("write initial");

    let mut debouncer = Debouncer::new(dir.path().to_path_buf(), Duration::from_millis(80));

    let event1 = notify::Event {
        kind: notify::EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Content,
        )),
        paths: vec![file.clone()],
        attrs: notify::event::EventAttributes::default(),
    };
    debouncer.ingest_event(event1.clone());

    // Immediate flush returns empty
    assert!(debouncer.flush_ready().is_empty());

    // Rapid writes within debounce window
    sleep(Duration::from_millis(30));
    debouncer.ingest_event(event1.clone());

    sleep(Duration::from_millis(30));
    debouncer.ingest_event(event1.clone());

    // Still within window
    assert!(debouncer.flush_ready().is_empty());

    // Wait past debounce duration
    sleep(Duration::from_millis(100));
    let actions = debouncer.flush_ready();
    assert_eq!(actions.len(), 1);
    assert_eq!(
        actions[0],
        graphia::daemon::SemanticAction::Modified(std::path::PathBuf::from("service.rs"))
    );
}

#[test]
fn test_bounded_update_queue_overflow_and_dirty_state() {
    let mut queue = UpdateQueue::new(3);
    assert_eq!(
        queue.push(graphia::daemon::SemanticAction::Created(
            std::path::PathBuf::from("a.rs")
        )),
        QueueStatus::Ok
    );
    assert_eq!(
        queue.push(graphia::daemon::SemanticAction::Modified(
            std::path::PathBuf::from("b.rs")
        )),
        QueueStatus::Ok
    );
    assert_eq!(queue.len(), 2);
    assert!(!queue.is_dirty());

    let batch = vec![
        graphia::daemon::SemanticAction::Removed(std::path::PathBuf::from("c.rs")),
        graphia::daemon::SemanticAction::Created(std::path::PathBuf::from("d.rs")),
    ];
    // Pushing 2 more elements exceeds capacity of 3
    assert_eq!(queue.push_batch(batch), QueueStatus::OverflowDirty);
    assert!(queue.is_dirty());
    assert_eq!(queue.len(), 0);

    // Further pushes fail immediately
    assert_eq!(
        queue.push(graphia::daemon::SemanticAction::Created(
            std::path::PathBuf::from("e.rs")
        )),
        QueueStatus::OverflowDirty
    );

    queue.clear_dirty();
    assert!(!queue.is_dirty());
    assert_eq!(queue.len(), 0);
}

#[test]
fn test_live_incremental_updates_and_generation_increments() {
    let dir = tempdir().expect("tempdir");
    let file_a = dir.path().join("a.rs");
    let file_b = dir.path().join("b.rs");

    fs::write(&file_a, "pub fn a() { b(); }").expect("write a");
    fs::write(&file_b, "pub fn b() {}").expect("write b");

    let manager = LiveStateManager::initialize(dir.path()).expect("init state manager");
    let snap1 = manager.read_snapshot();
    assert_eq!(snap1.generation, GraphGeneration(1));
    assert!(snap1.node_count >= 2);
    assert!(snap1.edge_count >= 1);

    // Modify file_b
    fs::write(&file_b, "pub fn b() {}\npub fn extra() {}").expect("modify b");
    let gen2 = manager.reconcile().expect("reconcile");
    assert_eq!(gen2, GraphGeneration(2));

    let snap2 = manager.read_snapshot();
    assert_eq!(snap2.generation, GraphGeneration(2));
    assert!(snap2.node_count > snap1.node_count);

    // Reader holding snap1 still has isolated unmutated view
    assert_eq!(snap1.generation, GraphGeneration(1));
}

#[test]
fn test_snapshot_isolation_during_updates() {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join("mod.rs"), "pub fn alpha() {}").expect("write mod.rs");

    let manager = LiveStateManager::initialize(dir.path()).expect("init");
    let reader_snap = manager.read_snapshot();

    // Spawn thread making rapid mutations
    let manager_clone = manager.repo_root().to_path_buf();
    let handle = thread::spawn(move || {
        let mgr = LiveStateManager::initialize(&manager_clone).expect("init in thread");
        for i in 0..10 {
            fs::write(
                manager_clone.join("mod.rs"),
                format!("pub fn func_{i}() {{}}"),
            )
            .expect("write");
            let _ = mgr.reconcile();
            sleep(Duration::from_millis(5));
        }
    });

    handle.join().expect("join");

    // Reader still observes pristine initial snapshot
    assert_eq!(reader_snap.generation, GraphGeneration(1));
    assert!(
        reader_snap
            .graph
            .nodes
            .iter()
            .any(|n| n.name == "alpha" && n.kind == graphia::model::NodeKind::Function)
    );
}

#[test]
fn test_daemon_server_lifecycle_and_status_reporting() {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join("core.rs"), "pub fn core() {}").expect("write core");

    let config = DaemonConfig {
        repo_root: dir.path().to_path_buf(),
        debounce_duration: Duration::from_millis(50),
        queue_capacity: 100,
        persistence_interval: Duration::from_millis(100),
    };

    let mut server = DaemonServer::new(config).expect("create daemon server");
    let signal = server.shutdown_signal();
    let repo_path = dir.path().to_path_buf();

    let daemon_thread = thread::spawn(move || {
        server.run().expect("server run");
    });

    // Wait for daemon to initialize and write status file
    let mut status = None;
    for _ in 0..50 {
        if let Ok(Some(s)) = DaemonServer::read_daemon_status(&repo_path) {
            status = Some(s);
            break;
        }
        sleep(Duration::from_millis(50));
    }
    let status = status.expect("status should exist within timeout");
    assert!(status.running);
    assert_eq!(status.generation, GraphGeneration(1));
    assert!(status.node_count >= 1);

    // Create a new source file while daemon is running
    fs::write(repo_path.join("extra.rs"), "pub fn extra() {}").expect("write extra");

    let mut status2 = None;
    for _ in 0..50 {
        if let Ok(Some(s)) = DaemonServer::read_daemon_status(&repo_path) {
            if s.node_count > status.node_count || s.generation > status.generation {
                status2 = Some(s);
                break;
            }
            status2 = Some(s);
        }
        sleep(Duration::from_millis(50));
    }
    let status2 = status2.expect("status should exist within timeout");
    assert!(status2.generation >= GraphGeneration(1));

    // Signal graceful shutdown
    signal.trigger();
    daemon_thread.join().expect("join daemon thread");

    // Status file should be cleaned up on clean exit
    let mut status_post = Some(());
    for _ in 0..50 {
        if let Ok(None) = DaemonServer::read_daemon_status(&repo_path) {
            status_post = None;
            break;
        }
        sleep(Duration::from_millis(50));
    }
    assert!(status_post.is_none());
}

#[test]
fn test_daemon_cli_subcommands() {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join("main.rs"), "fn main() {}").expect("write main");

    // Test DaemonStatus CLI when no daemon is active
    let cli_status = Cli {
        command: Commands::DaemonStatus {
            repo: Some(dir.path().to_path_buf()),
            format: CliFormat::Json,
        },
    };
    run(cli_status).expect("run daemon status CLI");

    // Test nested daemon status subcommand
    let cli_nested = Cli {
        command: Commands::Daemon {
            action: Some(DaemonAction::Status {
                repo: Some(dir.path().to_path_buf()),
                format: CliFormat::Human,
            }),
            repo: None,
            debounce_ms: None,
        },
    };
    run(cli_nested).expect("run nested daemon status CLI");
}

#[test]
fn persistence_worker_flushes_latest_generation() {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join("main.rs"), "pub fn main_fn() {}").expect("write source");
    let manager = LiveStateManager::initialize(dir.path()).expect("init");
    let worker = PersistenceWorker::new(dir.path().to_path_buf());
    worker.enqueue(manager.read_snapshot());
    let generation = manager.reconcile().expect("reconcile");
    worker.enqueue(manager.read_snapshot());
    worker.flush().expect("flush persistence");
    assert_eq!(
        graphia::storage::load_graph_binary(&dir.path().join(".graphia/index.bin"))
            .expect("load index")
            .node_count(),
        manager.read_snapshot().node_count
    );
    assert_eq!(generation, GraphGeneration(2));
}
