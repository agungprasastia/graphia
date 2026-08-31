use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone)]
pub struct ShutdownSignal {
    cancelled: Arc<AtomicBool>,
}

impl Default for ShutdownSignal {
    fn default() -> Self {
        Self::new()
    }
}

impl ShutdownSignal {
    #[must_use]
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn trigger(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shutdown_signal() {
        let signal = ShutdownSignal::new();
        assert!(!signal.is_cancelled());
        let clone = signal.clone();
        clone.trigger();
        assert!(signal.is_cancelled());
    }
}
