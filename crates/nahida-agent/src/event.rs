//! What the loop reports as it goes.
//!
//! The loop does not print. It emits these, and the caller decides what a
//! terminal, a log, or a test makes of them — which is why the CLI can render
//! streaming text without the loop knowing a terminal exists.

use nahida_llm::{StopReason, Usage};

#[derive(Debug, Clone)]
pub enum AgentEvent {
    TurnStart {
        turn: u32,
    },
    /// A fragment of visible text. Arrives token-by-token.
    Text {
        delta: String,
    },
    /// A fragment of summarized reasoning; only ever non-empty when the agent
    /// was built with `show_thinking(true)`.
    Thinking {
        delta: String,
    },
    ToolCall {
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        name: String,
        is_error: bool,
        content: String,
    },
    TurnEnd {
        usage: Usage,
    },
    /// The transcript is about to be summarized and replaced. Fires only
    /// between turns, never mid-tool-exchange.
    Compacting,
    /// The replacement landed; `transcript` is now the summary.
    Compacted,
    Done {
        stop_reason: Option<StopReason>,
    },
}
