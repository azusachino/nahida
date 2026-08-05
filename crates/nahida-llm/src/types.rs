//! Wire types for the Anthropic Messages API (`POST /v1/messages`).
//!
//! These mirror the JSON shapes exactly. Two things are deliberately *absent*,
//! because sending them to `claude-opus-5` is a 400:
//!
//! - `temperature` / `top_p` / `top_k` — removed on Opus 4.7 and later.
//! - `thinking: {type: "enabled", budget_tokens: N}` — removed on the same
//!   models. Depth is controlled by [`Effort`] instead.
//!
//! Leaving them out of the struct means the compiler enforces the constraint
//! rather than the API rejecting it at runtime.

use serde::{Deserialize, Serialize};

/// Every request must carry this. It is an API-shape version, not a model version.
pub const API_VERSION: &str = "2023-06-01";

/// The default model. See <https://platform.claude.com/docs/en/about-claude/models/overview>.
pub const DEFAULT_MODEL: &str = "claude-opus-5";

// Takes `&bool` because that is the signature serde's `skip_serializing_if`
// requires.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// A single block of content. The API is block-structured in both directions:
/// one assistant turn can carry text, thinking, and several `tool_use` blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    /// On Opus 5 the raw chain of thought is never returned. With the default
    /// `display: "omitted"` these arrive with `thinking` empty; ask for
    /// [`ThinkingDisplay::Summarized`] to get readable text.
    ///
    /// `signature` is opaque and must survive a round trip unchanged — the API
    /// rejects thinking blocks whose content was modified, so we keep the field
    /// even though nothing here reads it.
    Thinking {
        thinking: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    /// The model asking us to run a tool. `input` is whatever the tool's schema
    /// declared; validating it is the tool's job, not the transport's.
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Our answer to a `ToolUse`. `tool_use_id` must match, and every pending
    /// call must get one — a dropped result is a stuck conversation.
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default, skip_serializing_if = "is_false")]
        is_error: bool,
    },
    /// Anything the API adds that we do not model (`redacted_thinking`,
    /// `server_tool_use`, …). Present so a new block type is a shrug rather than
    /// a deserialization failure; the agent drops these before replaying a turn.
    #[serde(other)]
    Unknown,
}

impl ContentBlock {
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text { text: s.into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn user(content: Vec<ContentBlock>) -> Self {
        Self { role: Role::User, content }
    }

    pub fn assistant(content: Vec<ContentBlock>) -> Self {
        Self { role: Role::Assistant, content }
    }
}

/// Marks a prompt-cache breakpoint.
///
/// Caching is a *prefix* match: render order is `tools` → `system` → `messages`,
/// so a breakpoint on the last system block caches the tools and the system
/// prompt together. Any byte that changes ahead of a breakpoint invalidates it —
/// which is why nothing here interpolates a timestamp.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CacheControl {
    #[serde(rename = "type")]
    pub kind: CacheKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheKind {
    Ephemeral,
}

impl CacheControl {
    pub fn ephemeral() -> Self {
        Self { kind: CacheKind::Ephemeral }
    }
}

/// A system-prompt block. Sent as an array (not a bare string) so a
/// [`CacheControl`] breakpoint is expressible.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemBlock {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

impl SystemBlock {
    pub fn new(text: impl Into<String>) -> Self {
        Self { kind: "text", text: text.into(), cache_control: None }
    }

    /// Mark this block as a cache breakpoint. Below the model's minimum
    /// cacheable prefix (512 tokens on Opus 5) this is silently a no-op —
    /// check `usage.cache_read_input_tokens` rather than assuming a hit.
    #[must_use]
    pub fn cached(mut self) -> Self {
        self.cache_control = Some(CacheControl::ephemeral());
        self
    }
}

/// A tool as the *model* sees it: a name, a description, and a JSON Schema.
/// The description is load-bearing — it is how the model decides when to call.
#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl Effort {
    /// The wire value. Note `xhigh`, not `x-high`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
        }
    }
}

impl std::str::FromStr for Effort {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "xhigh" => Ok(Self::XHigh),
            "max" => Ok(Self::Max),
            other => Err(format!("unknown effort `{other}` (low|medium|high|xhigh|max)")),
        }
    }
}

/// `effort` lives inside `output_config`, not at the top level.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct OutputConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingDisplay {
    /// The default on Opus 5: thinking blocks arrive with empty text.
    Omitted,
    /// A readable summary. Worth setting for anything that shows progress to a
    /// human — otherwise a long think looks like a hang.
    Summarized,
}

/// Thinking config. `adaptive` is the only mode we express: the model decides
/// how much to think, and [`Effort`] scales it.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Thinking {
    #[serde(rename = "type")]
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<ThinkingDisplay>,
}

impl Thinking {
    pub fn summarized() -> Self {
        Self { kind: "adaptive", display: Some(ThinkingDisplay::Summarized) }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Request {
    pub model: String,
    /// A hard cap on thinking **plus** response text, and the model cannot see
    /// it. Size it generously when streaming; truncation shows up as
    /// [`StopReason::MaxTokens`].
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub system: Vec<SystemBlock>,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_config: Option<OutputConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Thinking>,
    pub stream: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// The model is done talking.
    EndTurn,
    /// Hit `max_tokens`. The response is truncated, not complete.
    MaxTokens,
    StopSequence,
    /// The model wants tools run. This is the one that continues the loop.
    ToolUse,
    /// A server-side tool loop paused. Resend the transcript to resume.
    PauseTurn,
    /// Safety classifiers declined. `content` is empty or partial — never a
    /// complete answer. Opus 5 ships elevated cybersecurity safeguards, so this
    /// is a real branch, not a theoretical one.
    Refusal,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StopDetails {
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub explanation: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    /// Tokens written to cache this request (~1.25x input price).
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    /// Tokens served from cache (~0.1x). Zero across repeated identical
    /// prefixes means something is invalidating the cache.
    #[serde(default)]
    pub cache_read_input_tokens: u64,
}

impl Usage {
    /// Total prompt size. `input_tokens` alone is only the *uncached* remainder.
    pub fn prompt_tokens(&self) -> u64 {
        self.input_tokens + self.cache_creation_input_tokens + self.cache_read_input_tokens
    }

    pub fn add(&mut self, other: Self) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_creation_input_tokens += other.cache_creation_input_tokens;
        self.cache_read_input_tokens += other.cache_read_input_tokens;
    }
}

/// One complete assistant turn.
#[derive(Debug, Clone, Deserialize)]
pub struct Response {
    pub id: String,
    pub model: String,
    pub content: Vec<ContentBlock>,
    pub stop_reason: Option<StopReason>,
    /// Populated **only** when `stop_reason` is [`StopReason::Refusal`]. Branch
    /// on `stop_reason`, never on the presence of this field.
    #[serde(default)]
    pub stop_details: Option<StopDetails>,
    #[serde(default)]
    pub usage: Usage,
}

impl Response {
    /// All text blocks joined. Empty on a refusal — check `stop_reason` first.
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// The tool calls this turn is waiting on.
    pub fn tool_uses(&self) -> Vec<(&str, &str, &serde_json::Value)> {
        self.content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::ToolUse { id, name, input } => {
                    Some((id.as_str(), name.as_str(), input))
                }
                _ => None,
            })
            .collect()
    }

    /// The turn as it should be replayed in the next request. Thinking blocks
    /// are kept verbatim (the API rejects modified ones); block types we do not
    /// model are dropped, since echoing `{"type":"Unknown"}` would be invalid.
    pub fn replayable(&self) -> Vec<ContentBlock> {
        self.content.iter().filter(|b| !matches!(b, ContentBlock::Unknown)).cloned().collect()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiErrorBody {
    #[serde(default)]
    pub r#type: String,
    #[serde(default)]
    pub message: String,
}
