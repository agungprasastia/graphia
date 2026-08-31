use std::collections::VecDeque;

use crate::daemon::debounce::SemanticAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueStatus {
    Ok,
    OverflowDirty,
}

pub struct UpdateQueue {
    capacity: usize,
    queue: VecDeque<SemanticAction>,
    dirty: bool,
}

impl UpdateQueue {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            queue: VecDeque::with_capacity(capacity),
            dirty: false,
        }
    }

    /// Push batch of semantic actions to queue. If capacity exceeded, marks state dirty.
    pub fn push_batch(&mut self, actions: Vec<SemanticAction>) -> QueueStatus {
        if self.dirty {
            return QueueStatus::OverflowDirty;
        }

        if self.queue.len() + actions.len() > self.capacity {
            self.dirty = true;
            self.queue.clear();
            return QueueStatus::OverflowDirty;
        }

        for action in actions {
            self.queue.push_back(action);
        }
        QueueStatus::Ok
    }

    /// Push single action.
    pub fn push(&mut self, action: SemanticAction) -> QueueStatus {
        self.push_batch(vec![action])
    }

    /// Pop next available action.
    pub fn pop(&mut self) -> Option<SemanticAction> {
        self.queue.pop_front()
    }

    /// Drain all queued actions.
    pub fn drain_all(&mut self) -> Vec<SemanticAction> {
        self.queue.drain(..).collect()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Clear dirty flag after a full reconciliation scan.
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
        self.queue.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_update_queue_overflow_marks_dirty() {
        let mut q = UpdateQueue::new(2);
        assert_eq!(
            q.push(SemanticAction::Created(PathBuf::from("a.rs"))),
            QueueStatus::Ok
        );
        assert_eq!(
            q.push(SemanticAction::Created(PathBuf::from("b.rs"))),
            QueueStatus::Ok
        );
        assert_eq!(q.len(), 2);
        assert!(!q.is_dirty());

        // Exceed capacity
        assert_eq!(
            q.push(SemanticAction::Created(PathBuf::from("c.rs"))),
            QueueStatus::OverflowDirty
        );
        assert!(q.is_dirty());
        assert!(q.is_empty()); // Queue cleared on overflow to prevent stale partial application

        q.clear_dirty();
        assert!(!q.is_dirty());
    }
}
