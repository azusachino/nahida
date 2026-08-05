//! Cooperative cancellation.
//!
//! A flag rather than dropping the future, because dropping mid-turn loses the
//! transcript: the assistant message was never appended, so a resumed session
//! would have a `tool_use` with no matching result. The loop checks between
//! stream events and between turns, which are the points where stopping leaves
//! the transcript consistent.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// Clear the flag so the same handle can be reused for the next prompt.
    pub fn reset(&self) {
        self.0.store(false, Ordering::Relaxed);
    }
}
