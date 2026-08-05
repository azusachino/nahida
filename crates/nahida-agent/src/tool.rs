//! What a tool is, from the loop's point of view.

use async_trait::async_trait;
use nahida_llm::ToolSpec;

/// The result of running a tool.
///
/// A failure is **not** a Rust `Err`: it is a normal result with `is_error` set,
/// handed back to the model so it can adapt. Propagating tool failures as errors
/// would end the turn, which is exactly wrong — "file not found" is information
/// the model should act on, not a crash.
#[derive(Debug, Clone)]
pub struct ToolOutcome {
    pub content: String,
    pub is_error: bool,
}

impl ToolOutcome {
    pub fn ok(content: impl Into<String>) -> Self {
        Self { content: content.into(), is_error: false }
    }

    pub fn err(content: impl Into<String>) -> Self {
        Self { content: content.into(), is_error: true }
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;

    /// Shown to the model verbatim. Be prescriptive about *when* to call, not
    /// just what the tool does — that is what drives the decision to use it.
    fn description(&self) -> &str;

    /// JSON Schema for the input. The model tries to honour it; it is not a
    /// guarantee, so [`Tool::call`] still validates.
    fn input_schema(&self) -> serde_json::Value;

    async fn call(&self, input: serde_json::Value) -> ToolOutcome;

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name().to_string(),
            description: self.description().to_string(),
            input_schema: self.input_schema(),
            cache_control: None,
        }
    }
}

/// Pull a required string out of a tool's input.
pub fn required_str(input: &serde_json::Value, key: &str) -> Result<String, String> {
    input
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("missing required string parameter `{key}`"))
}

/// Pull an optional integer, rejecting a present-but-wrong-typed value rather
/// than silently falling back to the default.
pub fn optional_u64(input: &serde_json::Value, key: &str) -> Result<Option<u64>, String> {
    match input.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .map(Some)
            .ok_or_else(|| format!("parameter `{key}` must be a non-negative integer")),
    }
}

/// As [`optional_u64`], saturating at `usize::MAX` for counts and offsets.
pub fn optional_usize(input: &serde_json::Value, key: &str) -> Result<Option<usize>, String> {
    Ok(optional_u64(input, key)?.map(|n| usize::try_from(n).unwrap_or(usize::MAX)))
}
