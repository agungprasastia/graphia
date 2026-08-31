use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::daemon::debounce::SemanticAction;
use crate::error::Result;
use crate::graph::Graph;
use crate::incremental::IncrementalWorkspace;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct GraphGeneration(pub u64);

impl GraphGeneration {
    #[must_use]
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DaemonHealth {
    Healthy,
    Updating,
    Dirty,
    Recovering,
    Failed,
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
    pub health: DaemonHealth,
    pub fallback_reconcile_count: usize,
}

pub struct LiveStateManager {
    repo_root: PathBuf,
    current_snapshot: Arc<RwLock<LiveSnapshot>>,
    workspace: Arc<RwLock<IncrementalWorkspace>>,
    health: Arc<RwLock<DaemonHealth>>,
}

impl LiveStateManager {
    /// Initialize manager with initial graph build or load.
    pub fn initialize(repo_root: &Path) -> Result<Self> {
        let ws = IncrementalWorkspace::new(repo_root.to_path_buf())?;
        let graph = ws.graph.clone();

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
            workspace: Arc::new(RwLock::new(ws)),
            health: Arc::new(RwLock::new(DaemonHealth::Healthy)),
        })
    }

    #[must_use]
    pub fn health(&self) -> DaemonHealth {
        *self.health.read().expect("health rwlock")
    }

    pub fn set_health(&self, h: DaemonHealth) {
        *self.health.write().expect("health rwlock") = h;
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

    /// Apply semantic action batch incrementally.
    pub fn apply_actions(&self, actions: &[SemanticAction]) -> Result<bool> {
        self.set_health(DaemonHealth::Updating);
        let mut ws = self.workspace.write().expect("workspace write lock");
        // Apply against a clone so a parse or filesystem error cannot corrupt the
        // last valid workspace while preserving its published generation.
        let mut candidate = ws.clone();
        match candidate.apply_changes(actions) {
            Ok(dirty) => {
                if dirty {
                    self.update_graph(candidate.graph.clone());
                    *ws = candidate;
                }
                self.set_health(DaemonHealth::Healthy);
                Ok(dirty)
            }
            Err(err) => {
                self.set_health(DaemonHealth::Dirty);
                Err(err)
            }
        }
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
        self.set_health(DaemonHealth::Recovering);
        let mut ws = self.workspace.write().expect("workspace write lock");
        ws.reconcile_full()?;
        let next_generation = self.update_graph(ws.graph.clone());
        self.set_health(DaemonHealth::Healthy);
        Ok(next_generation)
    }

    #[must_use]
    pub fn fallback_reconcile_count(&self) -> usize {
        self.workspace
            .read()
            .expect("workspace read lock")
            .fallback_reconcile_count
    }

    #[must_use]
    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }
}
