use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use serde_json::to_vec_pretty;

use crate::daemon::debounce::Debouncer;
use crate::daemon::shutdown::ShutdownSignal;
use crate::daemon::state::{DaemonStatusInfo, LiveStateManager};
use crate::daemon::update::{QueueStatus, UpdateQueue};
use crate::daemon::watcher::create_watcher;
use crate::error::{GraphiaError, Result};

pub struct DaemonConfig {
    pub repo_root: PathBuf,
    pub debounce_duration: Duration,
    pub queue_capacity: usize,
    pub persistence_interval: Duration,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            repo_root: PathBuf::from("."),
            debounce_duration: Duration::from_millis(100),
            queue_capacity: 1000,
            persistence_interval: Duration::from_secs(5),
        }
    }
}

pub struct DaemonServer {
    config: DaemonConfig,
    state_manager: Arc<LiveStateManager>,
    shutdown_signal: ShutdownSignal,
}

impl DaemonServer {
    pub fn new(config: DaemonConfig) -> Result<Self> {
        let canonical_root = config
            .repo_root
            .canonicalize()
            .map_err(|e| GraphiaError::Io {
                path: config.repo_root.clone(),
                message: e.to_string(),
            })?;

        let mut conf = config;
        conf.repo_root = canonical_root;

        let state_manager = Arc::new(LiveStateManager::initialize(&conf.repo_root)?);
        let shutdown_signal = ShutdownSignal::new();

        Ok(Self {
            config: conf,
            state_manager,
            shutdown_signal,
        })
    }

    #[must_use]
    pub fn state_manager(&self) -> Arc<LiveStateManager> {
        self.state_manager.clone()
    }

    #[must_use]
    pub fn shutdown_signal(&self) -> ShutdownSignal {
        self.shutdown_signal.clone()
    }

    /// Run the daemon loop blocking until shutdown signal or error.
    pub fn run(&mut self) -> Result<()> {
        let (tx, rx) = channel();
        let _watcher = create_watcher(&self.config.repo_root, tx)?;

        let mut debouncer =
            Debouncer::new(self.config.repo_root.clone(), self.config.debounce_duration);
        let mut queue = UpdateQueue::new(self.config.queue_capacity);
        let mut last_persist = Instant::now();

        // Write initial status file
        self.write_status_file(queue.len(), queue.is_dirty())?;

        while !self.shutdown_signal.is_cancelled() {
            // Read events with a short timeout to allow debouncing & shutdown checks
            if let Ok(Ok(event)) = rx.recv_timeout(Duration::from_millis(25)) {
                debouncer.ingest_event(event);
            }

            // Flush debounced ready actions
            let ready_actions = debouncer.flush_ready();
            if !ready_actions.is_empty() {
                let status = queue.push_batch(ready_actions);
                if status == QueueStatus::OverflowDirty {
                    // Reconcile immediately
                    let _ = self.state_manager.reconcile();
                    queue.clear_dirty();
                    self.write_status_file(queue.len(), queue.is_dirty())?;
                }
            }

            // If we have items in queue, process them
            if !queue.is_empty() && !queue.is_dirty() {
                let _actions = queue.drain_all();
                // Perform incremental update
                let _ = self.state_manager.reconcile();
                self.write_status_file(queue.len(), queue.is_dirty())?;
            }

            // Periodic persistence / status refresh
            if last_persist.elapsed() >= self.config.persistence_interval {
                self.write_status_file(queue.len(), queue.is_dirty())?;
                last_persist = Instant::now();
            }
        }

        // Final graceful flush
        self.write_status_file(queue.len(), queue.is_dirty())?;
        self.remove_status_file();

        Ok(())
    }

    fn status_file_path(root: &Path) -> PathBuf {
        root.join(".graphia/daemon.json")
    }

    pub fn write_status_file(&self, pending_events: usize, dirty: bool) -> Result<()> {
        let snap = self.state_manager.read_snapshot();
        let status = DaemonStatusInfo {
            running: true,
            pid: std::process::id(),
            repo_root: self.config.repo_root.clone(),
            generation: snap.generation,
            node_count: snap.node_count,
            edge_count: snap.edge_count,
            last_update_ms: snap.timestamp_ms,
            dirty,
            pending_events,
        };

        let json = to_vec_pretty(&status).map_err(|e| GraphiaError::Storage {
            message: e.to_string(),
        })?;

        let status_path = Self::status_file_path(&self.config.repo_root);
        crate::storage::atomic_write(&status_path, &json)?;
        Ok(())
    }

    pub fn remove_status_file(&self) {
        let status_path = Self::status_file_path(&self.config.repo_root);
        let _ = fs::remove_file(status_path);
    }

    /// Read daemon status from a repository root without running a daemon instance.
    pub fn read_daemon_status(root: &Path) -> Result<Option<DaemonStatusInfo>> {
        let status_path = Self::status_file_path(root);
        if !status_path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&status_path).map_err(|e| GraphiaError::Io {
            path: status_path.clone(),
            message: e.to_string(),
        })?;

        let status: DaemonStatusInfo =
            serde_json::from_str(&content).map_err(|e| GraphiaError::Storage {
                message: e.to_string(),
            })?;

        Ok(Some(status))
    }
}
