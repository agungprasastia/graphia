use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::graph::Graph;
use crate::incremental::update_repository;
use crate::storage::load_graph_binary;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct GraphGeneration(pub u64);

impl GraphGeneration {
    #[must_use]
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LiveSnapshot {
    pub generation: GraphGeneration,
    pub timestamp_ms: u64,
    pub node_count: usize,
    pub edge_count: usize,
    #[serde(skip)]
    pub graph: Arc<Graph>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatusInfo {
    pub running: bool,
    pub pid: u32,
    pub repo_root: PathBuf,
    pub generation: GraphGeneration,
    pub node_count: usize,
    pub edge_count: usize,
    pub last_update_ms: u64,
    pub dirty: bool,
    pub pending_events: usize,
}

pub struct LiveStateManager {
    repo_root: PathBuf,
    current_snapshot: Arc<RwLock<LiveSnapshot>>,
}

impl LiveStateManager {
    /// Initialize manager with initial graph build or load.
    pub fn initialize(repo_root: &Path) -> Result<Self> {
        let graph = if repo_root.join(".graphia/index.bin").exists() {
            match load_graph_binary(&repo_root.join(".graphia/index.bin")) {
                Ok(g) => g,
                Err(_) => update_repository(repo_root)?,
            }
        } else {
            update_repository(repo_root)?
        };

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let node_count = graph.node_count();
        let edge_count = graph.edge_count();

        let initial_snapshot = LiveSnapshot {
            generation: GraphGeneration(1),
            timestamp_ms: now_ms,
            node_count,
            edge_count,
            graph: Arc::new(graph),
        };

        Ok(Self {
            repo_root: repo_root.to_path_buf(),
            current_snapshot: Arc::new(RwLock::new(initial_snapshot)),
        })
    }

    /// Read atomic live snapshot.
    #[must_use]
    pub fn read_snapshot(&self) -> Arc<LiveSnapshot> {
        let guard = self
            .current_snapshot
            .read()
            .expect("poisoned live snapshot rwlock");
        Arc::new(guard.clone())
    }

    /// Update live graph atomically and increment generation.
    pub fn update_graph(&self, new_graph: Graph) -> GraphGeneration {
        let mut guard = self
            .current_snapshot
            .write()
            .expect("poisoned live snapshot rwlock");

        let next_gen = guard.generation.next();
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        *guard = LiveSnapshot {
            generation: next_gen,
            timestamp_ms: now_ms,
            node_count: new_graph.node_count(),
            edge_count: new_graph.edge_count(),
            graph: Arc::new(new_graph),
        };

        next_gen
    }

    /// Run full or incremental reconciliation against filesystem.
    pub fn reconcile(&self) -> Result<GraphGeneration> {
        let graph = update_repository(&self.repo_root)?;
        Ok(self.update_graph(graph))
    }

    #[must_use]
    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_generation_monotonicity_and_snapshot_isolation() {
        let dir = tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.rs"), "pub fn a() {}").expect("write file");

        let manager = LiveStateManager::initialize(dir.path()).expect("init");
        let snap1 = manager.read_snapshot();
        assert_eq!(snap1.generation, GraphGeneration(1));
        assert_eq!(snap1.node_count, 2); // file + function

        let graph2 = Graph::new(Vec::new(), Vec::new());
        let gen2 = manager.update_graph(graph2);
        assert_eq!(gen2, GraphGeneration(2));

        // Reader holding snap1 still sees original snapshot intact
        assert_eq!(snap1.generation, GraphGeneration(1));
        assert_eq!(snap1.node_count, 2);

        // New reader sees updated generation
        let snap2 = manager.read_snapshot();
        assert_eq!(snap2.generation, GraphGeneration(2));
        assert_eq!(snap2.node_count, 0);
    }
}
