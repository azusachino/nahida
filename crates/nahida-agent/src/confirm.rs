//! Whether a tool call should be confirmed by a human before it runs.

use async_trait::async_trait;

/// Approves or denies a call flagged by [`crate::Tool::requires_confirmation`].
///
/// Checked sequentially, one call at a time, before the turn's approved calls
/// run concurrently — terminal interaction can't be parallelized the way tool
/// execution can. With no handler registered on the [`crate::Agent`] (the
/// default), a flagged call is never checked and runs exactly as it did
/// before gating existed.
#[async_trait]
pub trait Confirm: Send + Sync {
    async fn ask(&self, tool: &str, input: &serde_json::Value) -> bool;
}
