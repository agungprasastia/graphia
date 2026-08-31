use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use notify::Event;
use notify::event::{EventKind, ModifyKind, RenameMode};

use crate::daemon::watcher::{is_excluded_path, is_relevant_source_file};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticAction {
    Created(PathBuf),
    Modified(PathBuf),
    Removed(PathBuf),
    Renamed { from: PathBuf, to: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingKind {
    Create,
    Modify,
    Remove,
    RenameFrom,
    RenameTo,
}

#[derive(Debug, Clone)]
struct PendingEvent {
    kind: PendingKind,
    last_seen: Instant,
}

pub struct Debouncer {
    debounce_duration: Duration,
    pending: HashMap<PathBuf, PendingEvent>,
    root: PathBuf,
}

impl Debouncer {
    #[must_use]
    pub fn new(root: PathBuf, debounce_duration: Duration) -> Self {
        Self {
            debounce_duration,
            pending: HashMap::new(),
            root,
        }
    }

    /// Ingest a notify filesystem event.
    pub fn ingest_event(&mut self, event: Event) {
        let now = Instant::now();

        match event.kind {
            EventKind::Create(_) => {
                for path in event.paths {
                    self.record_path(path, PendingKind::Create, now);
                }
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
                for path in event.paths {
                    self.record_path(path, PendingKind::RenameFrom, now);
                }
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
                for path in event.paths {
                    self.record_path(path, PendingKind::RenameTo, now);
                }
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
                if event.paths.len() >= 2 {
                    self.record_path(event.paths[0].clone(), PendingKind::RenameFrom, now);
                    self.record_path(event.paths[1].clone(), PendingKind::RenameTo, now);
                }
            }
            EventKind::Modify(_) => {
                for path in event.paths {
                    self.record_path(path, PendingKind::Modify, now);
                }
            }
            EventKind::Remove(_) => {
                for path in event.paths {
                    self.record_path(path, PendingKind::Remove, now);
                }
            }
            _ => {}
        }
    }

    fn record_path(&mut self, path: PathBuf, kind: PendingKind, now: Instant) {
        // Strip or keep relative/canonical
        if is_excluded_path(&path) {
            return;
        }

        if let Some(existing) = self.pending.get_mut(&path) {
            existing.last_seen = now;
            // State transitions for coalescing
            match (existing.kind, kind) {
                (PendingKind::Create, PendingKind::Modify) => {
                    // Still Create
                }
                (PendingKind::Create, PendingKind::Remove) => {
                    // Created and then immediately removed before flush -> remove pending
                    self.pending.remove(&path);
                }
                (PendingKind::Remove, PendingKind::Create) => {
                    existing.kind = PendingKind::Modify;
                }
                (PendingKind::Modify, PendingKind::Remove) => {
                    existing.kind = PendingKind::Remove;
                }
                (PendingKind::RenameFrom, _) => {
                    existing.kind = kind;
                }
                (PendingKind::RenameTo, _) => {
                    existing.kind = kind;
                }
                _ => {
                    existing.kind = kind;
                }
            }
        } else {
            self.pending.insert(
                path,
                PendingEvent {
                    kind,
                    last_seen: now,
                },
            );
        }
    }

    /// Flush events whose debounce window has elapsed since last_seen.
    pub fn flush_ready(&mut self) -> Vec<SemanticAction> {
        let now = Instant::now();
        let mut ready_paths = Vec::new();

        for (path, item) in &self.pending {
            if now.duration_since(item.last_seen) >= self.debounce_duration {
                ready_paths.push(path.clone());
            }
        }

        if ready_paths.is_empty() {
            return Vec::new();
        }

        let mut ready_items = Vec::new();
        for path in ready_paths {
            if let Some(item) = self.pending.remove(&path) {
                ready_items.push((path, item));
            }
        }

        // Pair renames if possible
        let mut renames_from = Vec::new();
        let mut renames_to = Vec::new();
        let mut others = Vec::new();

        for (path, item) in ready_items {
            match item.kind {
                PendingKind::RenameFrom => renames_from.push(path),
                PendingKind::RenameTo => renames_to.push(path),
                _ => others.push((path, item.kind)),
            }
        }

        let mut actions = Vec::new();

        // Match 1-to-1 renames if exact single pair or matching filenames
        while !renames_from.is_empty() && !renames_to.is_empty() {
            let from = renames_from.remove(0);
            let to = renames_to.remove(0);
            if is_relevant_source_file(&from) || is_relevant_source_file(&to) {
                actions.push(SemanticAction::Renamed {
                    from: self.normalize_rel(&from),
                    to: self.normalize_rel(&to),
                });
            }
        }

        for from in renames_from {
            if is_relevant_source_file(&from) {
                actions.push(SemanticAction::Removed(self.normalize_rel(&from)));
            }
        }

        for to in renames_to {
            if is_relevant_source_file(&to) {
                actions.push(SemanticAction::Created(self.normalize_rel(&to)));
            }
        }

        for (path, kind) in others {
            let exists = path.exists();
            let is_relevant = is_relevant_source_file(&path);
            let rel = self.normalize_rel(&path);

            match kind {
                PendingKind::Create => {
                    if exists && is_relevant {
                        actions.push(SemanticAction::Created(rel));
                    }
                }
                PendingKind::Modify => {
                    if exists && is_relevant {
                        actions.push(SemanticAction::Modified(rel));
                    } else if !exists && is_relevant {
                        actions.push(SemanticAction::Removed(rel));
                    }
                }
                PendingKind::Remove if is_relevant => {
                    actions.push(SemanticAction::Removed(rel));
                }
                _ => {}
            }
        }

        actions
    }

    fn normalize_rel(&self, path: &Path) -> PathBuf {
        if let Ok(rel) = path.strip_prefix(&self.root) {
            rel.to_path_buf()
        } else {
            path.to_path_buf()
        }
    }

    #[must_use]
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_debounce_coalesces_rapid_modifies() {
        let root = PathBuf::from("/repo");
        let mut debouncer = Debouncer::new(root.clone(), Duration::from_millis(50));

        let file = root.join("src/lib.rs");
        let event = Event {
            kind: EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Content)),
            paths: vec![file.clone()],
            attrs: notify::event::EventAttributes::default(),
        };

        debouncer.ingest_event(event.clone());
        debouncer.ingest_event(event.clone());
        debouncer.ingest_event(event.clone());

        // Immediately checking flush_ready returns empty because debounce hasn't elapsed
        let flushed = debouncer.flush_ready();
        assert!(flushed.is_empty());

        sleep(Duration::from_millis(70));
        let flushed = debouncer.flush_ready();
        // Since dummy file doesn't exist on disk, modify maps to Removed or Modified depending on exists()
        // Wait, for dummy path, path.exists() is false, so it emitted Removed.
        assert_eq!(flushed.len(), 1);
    }
}
