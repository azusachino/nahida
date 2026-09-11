//! What the loop reports as it goes.
//!
//! The loop does not print. It emits these, and the caller decides what a
//! terminal, a log, or a test makes of them — which is why the CLI can render
//! streaming text without the loop knowing a terminal exists.

use nahida_llm::{StopReason, Usage};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
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
        /// Matches `tool_use_id` on the `ToolResult` this call is answered
        /// by. Same name `ContentBlock::ToolUse` uses for the wire shape.
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        /// Matches the `id` on the `ToolCall` this result answers. Needed
        /// because `dispatch` now fires this event in real completion order,
        /// not call order — two calls to the *same* tool name are otherwise
        /// unpairable from the event stream alone (`--json` consumers in
        /// particular have nothing else to key on).
        tool_use_id: String,
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
    /// A request failed with a transient error and is about to be retried,
    /// after a backoff of `delay_ms`.
    Retrying {
        attempt: u32,
        max_attempts: u32,
        delay_ms: u64,
        reason: String,
    },
    Done {
        stop_reason: Option<StopReason>,
    },
}
