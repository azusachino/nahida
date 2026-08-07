//! Blocking a tool call on a human's y/N before it runs.
//!
//! Only wired in for an interactive terminal (see `main.rs`) — with no one to
//! ask, a scripted or piped invocation runs ungated rather than hanging on a
//! read that will never come.

use std::io::Write as _;

use async_trait::async_trait;
use nahida_agent::Confirm;

use crate::render::summarize;

pub struct TerminalConfirm;

#[async_trait]
impl Confirm for TerminalConfirm {
    async fn ask(&self, tool: &str, input: &serde_json::Value) -> bool {
        print!("\n▸ {tool} {} — run it? [y/N] ", summarize(input));
        let _ = std::io::stdout().flush();

        let mut line = String::new();
        // A read failure (e.g. stdin closed mid-prompt) refuses rather than
        // guesses — silently running an unconfirmed call would defeat the
        // point of asking.
        if std::io::stdin().read_line(&mut line).is_err() {
            return false;
        }
        matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
    }
}
