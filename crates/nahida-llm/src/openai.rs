//! The `OpenAI` Chat Completions wire format, as a second [`crate::Provider`].
//!
//! Exists for one concrete reason: the Z.ai coding plan's China-region
//! endpoint (`open.bigmodel.cn`) speaks this, not Anthropic Messages —
//! confirmed against `earendil-works/pi`'s own `zai-coding-cn` provider,
//! which uses this wire format exclusively for that endpoint. This module
//! translates in both directions so nothing above [`crate::provider::Provider`]
//! (in particular, `nahida-agent`'s loop) ever sees a non-canonical shape.
//!
//! Scoped to what nahida actually needs: tool calling, streaming and the
//! explicitly selected Z.ai model options. The request/replay and Z.ai option
//! behavior is a narrow source-backed adaptation of `packages/ai/src/api/openai-completions.ts`
//! in `earendil-works/pi` (MIT, pinned at 42a3497d03ad17e308a2299fa824727894f2c0ec);
//! no general Pi compatibility layer is implied.

use std::collections::{BTreeMap, HashMap, VecDeque};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::client::{Error, Profile, Result};
use crate::provider::{EventStream, Provider};
use crate::stream::{Delta, MessageDeltaInfo, MessageStartInfo, SseDecoder, StreamEvent};
use crate::types::{ContentBlock, Message, Request, Role, StopReason, ToolSpec, Usage};

/// Explicit `OpenAI` Chat Completions wire capabilities for a selected model.
/// Configuration parsing and file loading remain the host crate's job.
/// `Default` preserves Nahida's historical request shape.
#[derive(Debug, Clone, Default)]
pub struct OpenAiOptions {
    pub supports_store: Option<bool>,
    pub supports_developer_role: Option<bool>,
    pub supports_reasoning_effort: Option<bool>,
    pub max_tokens_field: Option<OpenAiMaxTokensField>,
    pub thinking_format: Option<OpenAiThinkingFormat>,
    pub supports_strict_mode: Option<bool>,
    pub zai_tool_stream: Option<bool>,
    pub reasoning: bool,
    /// Explicit null values are retained and mean that the level is unsupported.
    pub thinking_level_map: Option<BTreeMap<String, Option<String>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAiMaxTokensField {
    MaxTokens,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAiThinkingFormat {
    Zai,
}

pub struct OpenAiCompletionsProvider {
    http: reqwest::Client,
    token: String,
    profile: Profile,
    options: OpenAiOptions,
}

impl OpenAiCompletionsProvider {
    pub fn new(token: impl Into<String>, profile: Profile) -> Result<Self> {
        Self::with_options(token, profile, OpenAiOptions::default())
    }

    pub fn with_options(
        token: impl Into<String>,
        profile: Profile,
        options: OpenAiOptions,
    ) -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder().timeout(std::time::Duration::from_mins(10)).build()?,
            token: token.into(),
            profile,
            options,
        })
    }
}

#[async_trait]
impl Provider for OpenAiCompletionsProvider {
    fn profile(&self) -> &Profile {
        &self.profile
    }

    async fn stream(&self, req: &Request) -> Result<EventStream> {
        let body = encode_request(req, &self.profile, &self.options)?;
        let base = self.profile.base_url.trim_end_matches('/');

        let resp = self
            .http
            .post(format!("{base}/chat/completions"))
            .header("authorization", format!("Bearer {}", self.token))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await?;
            return Err(api_error(status.as_u16(), &text));
        }

        let state = DecodeState {
            bytes: resp.bytes_stream(),
            decoder: SseDecoder::new(),
            pending: VecDeque::new(),
            translate: Translate::default(),
            body_done: false,
            got_done: false,
            failure: None,
        };

        Ok(Box::pin(futures_util::stream::unfold(state, |mut st| async move {
            loop {
                if let Some(error) = st.failure.take() {
                    return Some((Err(error), st));
                }
                if let Some(event) = st.pending.pop_front() {
                    return Some((Ok(event), st));
                }
                if st.body_done {
                    return None;
                }

                match futures_util::StreamExt::next(&mut st.bytes).await {
                    Some(Ok(chunk)) => st.process_bytes(&chunk),
                    Some(Err(e)) => {
                        st.body_done = true;
                        st.pending.clear();
                        st.failure = Some(Error::Transport(e));
                    }
                    None => {
                        st.body_done = true;
                        st.pending.clear();
                        st.failure = Some(Error::Stream {
                            kind: "incomplete_stream".into(),
                            message: st
                                .translate
                                .settlement_error()
                                .unwrap_or_else(|| "stream ended before [DONE]".into()),
                        });
                    }
                }
            }
        })))
    }
}

struct DecodeState<S> {
    bytes: S,
    decoder: SseDecoder,
    pending: VecDeque<StreamEvent>,
    translate: Translate,
    body_done: bool,
    got_done: bool,
    failure: Option<Error>,
}

impl<S> DecodeState<S> {
    fn process_bytes(&mut self, bytes: &[u8]) {
        for frame in self.decoder.push(bytes) {
            if self.got_done {
                self.pending.clear();
                self.body_done = true;
                self.failure = Some(Error::Stream {
                    kind: "trailing_data".into(),
                    message: "received an SSE frame after [DONE]".into(),
                });
                break;
            }
            if frame.trim() == "[DONE]" {
                self.got_done = true;
                self.body_done = true;
                if let Some(message) = self.translate.settlement_error() {
                    self.failure =
                        Some(Error::Stream { kind: "incomplete_stream".into(), message });
                } else {
                    self.pending.push_back(StreamEvent::MessageStop);
                }
                continue;
            }
            match serde_json::from_str::<Chunk>(&frame) {
                Ok(chunk) => {
                    if let Some(error) = chunk.error {
                        self.body_done = true;
                        self.failure = Some(Error::Stream {
                            kind: error.r#type.unwrap_or_else(|| "api_error".into()),
                            message: error.message,
                        });
                        break;
                    }
                    self.pending.extend(self.translate.apply(chunk));
                }
                Err(source) => {
                    self.body_done = true;
                    self.failure = Some(Error::Stream {
                        kind: "decode_error".into(),
                        message: source.to_string(),
                    });
                    break;
                }
            }
        }
    }
}

fn api_error(status: u16, body: &str) -> Error {
    #[derive(Deserialize)]
    struct Envelope {
        error: OpenAiErrorBody,
    }
    #[derive(Deserialize)]
    struct OpenAiErrorBody {
        #[serde(default)]
        r#type: Option<String>,
        message: String,
    }
    match serde_json::from_str::<Envelope>(body) {
        Ok(e) => Error::Api {
            status,
            kind: e.error.r#type.unwrap_or_else(|| "unknown".to_string()),
            message: e.error.message,
        },
        Err(_) => Error::Api {
            status,
            kind: "unknown".to_string(),
            message: body.chars().take(500).collect(),
        },
    }
}

// ---- request encoding: canonical `Request` -> OpenAI JSON ----

#[derive(Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<OpenAiTool>,
    max_tokens: u32,
    stream: bool,
    stream_options: StreamOptions,
    #[serde(skip_serializing_if = "Option::is_none")]
    store: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_stream: Option<bool>,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Serialize)]
struct OpenAiMessage {
    role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_details: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct OpenAiToolCall {
    id: String,
    r#type: &'static str,
    function: OpenAiFunctionCall,
}

#[derive(Serialize)]
struct OpenAiFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Serialize)]
struct OpenAiTool {
    r#type: &'static str,
    function: OpenAiFunctionDef,
}

#[derive(Serialize)]
struct OpenAiFunctionDef {
    name: String,
    description: String,
    parameters: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    strict: Option<bool>,
}

fn encode_request(
    req: &Request,
    profile: &Profile,
    options: &OpenAiOptions,
) -> Result<OpenAiRequest> {
    let mut messages = Vec::new();

    let system: String =
        req.system.iter().map(|b| b.text.as_str()).collect::<Vec<_>>().join("\n\n");
    if !system.is_empty() {
        messages.push(OpenAiMessage {
            role: if options.supports_developer_role == Some(true) {
                "developer"
            } else {
                "system"
            },
            content: Some(system),
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
            reasoning: None,
            reasoning_text: None,
            reasoning_details: None,
        });
    }

    for message in &req.messages {
        encode_message(message, &mut messages);
    }

    let tools =
        req.tools.iter().map(|tool| encode_tool(tool, options)).collect::<Result<Vec<_>>>()?;
    let requested_effort = req.output_config.and_then(|config| config.effort);
    let (thinking, reasoning_effort) =
        if options.thinking_format == Some(OpenAiThinkingFormat::Zai) && options.reasoning {
            match requested_effort {
                None => (Some(serde_json::json!({"type":"disabled"})), None),
                Some(effort) => {
                    let level = effort.as_str();
                    let mapped = options.thinking_level_map.as_ref().and_then(|map| map.get(level));
                    let effort = match mapped {
                        Some(None) => {
                            return Err(Error::Configuration(
                                "requested reasoning effort is unsupported by this model",
                            ));
                        }
                        Some(Some(value)) => value.clone(),
                        None => level.to_owned(),
                    };
                    (
                        Some(serde_json::json!({"type":"enabled","clear_thinking":false})),
                        options.supports_reasoning_effort.unwrap_or(false).then_some(effort),
                    )
                }
            }
        } else {
            (None, None)
        };
    Ok(OpenAiRequest {
        model: req.model.clone(),
        messages,
        tool_stream: (options.zai_tool_stream == Some(true) && !tools.is_empty()).then_some(true),
        tools,
        // Existing callers retain the original `max_tokens` and usage shape.
        max_tokens: req.max_tokens.min(profile.default_max_tokens.max(req.max_tokens)),
        stream: true,
        stream_options: StreamOptions { include_usage: true },
        store: options.supports_store.filter(|supports| *supports).map(|_| false),
        thinking,
        reasoning_effort,
    })
}

/// A canonical message can be a plain user prompt, an assistant turn (text +
/// tool calls, thinking dropped -- see the module doc), or a *batch* of tool
/// results riding in one user-role message (nahida's own rule: every result
/// goes back in a single message). `OpenAI` has no batched-tool-result shape:
/// each becomes its own `role: "tool"` message.
fn encode_message(message: &Message, out: &mut Vec<OpenAiMessage>) {
    match message.role {
        Role::User => {
            let mut text = String::new();
            for block in &message.content {
                match block {
                    ContentBlock::Text { text: t, .. } => {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(t);
                    }
                    ContentBlock::ToolResult { tool_use_id, content, .. } => {
                        out.push(OpenAiMessage {
                            role: "tool",
                            content: Some(content.clone()),
                            tool_calls: None,
                            tool_call_id: Some(tool_use_id.clone()),
                            reasoning_content: None,
                            reasoning: None,
                            reasoning_text: None,
                            reasoning_details: None,
                        });
                    }
                    _ => {}
                }
            }
            if !text.is_empty() {
                out.push(OpenAiMessage {
                    role: "user",
                    content: Some(text),
                    tool_calls: None,
                    tool_call_id: None,
                    reasoning_content: None,
                    reasoning: None,
                    reasoning_text: None,
                    reasoning_details: None,
                });
            }
        }
        Role::Assistant => out.push(encode_assistant_message(&message.content)),
    }
}

fn encode_assistant_message(content: &[ContentBlock]) -> OpenAiMessage {
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    let mut reasoning_field: Option<String> = None;
    let mut reasoning_text = String::new();
    let mut reasoning_details: Option<serde_json::Value> = None;
    for block in content {
        match block {
            ContentBlock::Text { text: t, .. } => text.push_str(t),
            ContentBlock::ToolUse { id, name, input } => tool_calls.push(OpenAiToolCall {
                id: id.clone(),
                r#type: "function",
                function: OpenAiFunctionCall { name: name.clone(), arguments: input.to_string() },
            }),
            ContentBlock::Thinking { thinking, signature } => {
                reasoning_text.push_str(thinking);
                if let Some(signature) = signature {
                    if let Some(raw) = signature.strip_prefix("nahida:reasoning-details:") {
                        reasoning_details = serde_json::from_str(raw).ok();
                    } else if matches!(
                        signature.as_str(),
                        "reasoning_content" | "reasoning" | "reasoning_text"
                    ) {
                        reasoning_field = Some(signature.clone());
                    }
                }
            }
            ContentBlock::Unknown | ContentBlock::ToolResult { .. } => {}
        }
    }
    OpenAiMessage {
        role: "assistant",
        content: if text.is_empty() { None } else { Some(text) },
        tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
        tool_call_id: None,
        reasoning_content: if reasoning_details.is_none()
            && reasoning_field.as_deref() == Some("reasoning_content")
        {
            Some(reasoning_text.clone())
        } else {
            None
        },
        reasoning: if reasoning_details.is_none() && reasoning_field.as_deref() == Some("reasoning")
        {
            Some(reasoning_text.clone())
        } else {
            None
        },
        reasoning_text: if reasoning_details.is_none()
            && reasoning_field.as_deref() == Some("reasoning_text")
        {
            Some(reasoning_text)
        } else {
            None
        },
        reasoning_details,
    }
}

fn encode_tool(tool: &ToolSpec, options: &OpenAiOptions) -> Result<OpenAiTool> {
    let strict = options.supports_strict_mode == Some(true);
    Ok(OpenAiTool {
        r#type: "function",
        function: OpenAiFunctionDef {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: if strict {
                normalize_strict_schema(&tool.input_schema)?
            } else {
                tool.input_schema.clone()
            },
            strict: strict.then_some(true),
        },
    })
}

// Narrow port of Pi's strict tool-schema conversion at
// `packages/ai/src/api/constrained-sampling.ts` (MIT, pinned Pi revision
// documented in this module): preserve schema meaning, making only optional
// properties nullable before requiring every property.
fn normalize_strict_schema(schema: &serde_json::Value) -> Result<serde_json::Value> {
    let mut normalized = schema.clone();
    normalize_strict_schema_node(&mut normalized)?;
    if normalized.get("type").and_then(serde_json::Value::as_str) != Some("object") {
        return Err(Error::Configuration("strict tool schema root must have type object"));
    }
    Ok(normalized)
}

const UNSUPPORTED_STRICT_SCHEMA_KEYS: &[&str] = &[
    "$ref",
    "$defs",
    "definitions",
    "allOf",
    "oneOf",
    "patternProperties",
    "dependentSchemas",
    "dependencies",
    "unevaluatedProperties",
    "propertyNames",
    "contains",
    "prefixItems",
    "not",
    "if",
    "then",
    "else",
];

fn normalize_strict_schema_node(schema: &mut serde_json::Value) -> Result<()> {
    let Some(object) = schema.as_object_mut() else {
        return Err(Error::Configuration("strict tool schema boolean schemas are unsupported"));
    };
    if UNSUPPORTED_STRICT_SCHEMA_KEYS.iter().any(|key| object.contains_key(*key)) {
        return Err(Error::Configuration("strict tool schema contains an unsupported keyword"));
    }
    normalize_strict_any_of(object)?;
    if let Some(items) = object.get_mut("items") {
        if items.is_array() {
            return Err(Error::Configuration("strict tool schema tuple arrays are unsupported"));
        }
        normalize_strict_schema_node(items)?;
    }
    let is_object = object.get("type").and_then(serde_json::Value::as_str) == Some("object");
    if object.contains_key("properties") && !is_object {
        return Err(Error::Configuration("strict tool schema properties require type object"));
    }
    if is_object {
        normalize_strict_object(schema)?;
    }
    Ok(())
}

fn normalize_strict_any_of(object: &mut serde_json::Map<String, serde_json::Value>) -> Result<()> {
    let Some(any_of) = object.get_mut("anyOf") else { return Ok(()) };
    let Some(variants) = any_of.as_array_mut() else {
        return Err(Error::Configuration("strict tool schema anyOf must be a nonempty array"));
    };
    if variants.is_empty() {
        return Err(Error::Configuration("strict tool schema anyOf must be a nonempty array"));
    }
    for variant in variants {
        if is_structured_schema(variant) {
            return Err(Error::Configuration(
                "strict tool schema object and array unions are unsupported",
            ));
        }
        normalize_strict_schema_node(variant)?;
    }
    Ok(())
}

fn normalize_strict_object(schema: &mut serde_json::Value) -> Result<()> {
    let object = schema.as_object_mut().expect("validated schema object");
    if object.get("additionalProperties").is_some_and(|value| value != false) {
        return Err(Error::Configuration("strict tool schema additionalProperties must be false"));
    }
    let had_properties = object.contains_key("properties");
    let properties_value =
        object.get("properties").cloned().unwrap_or_else(|| serde_json::json!({}));
    let Some(mut properties) = properties_value.as_object().cloned() else {
        return Err(Error::Configuration("strict tool schema properties must be a schema map"));
    };
    let names = properties.keys().cloned().collect::<Vec<_>>();
    let required = match object.get("required") {
        None => std::collections::BTreeSet::new(),
        Some(value) => {
            let Some(entries) = value.as_array() else {
                return Err(Error::Configuration(
                    "strict tool schema required must be a string array",
                ));
            };
            if entries.iter().any(|entry| !entry.is_string()) {
                return Err(Error::Configuration(
                    "strict tool schema required must be a string array",
                ));
            }
            let required = entries
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect::<std::collections::BTreeSet<_>>();
            if required.iter().any(|name| !properties.contains_key(name)) {
                return Err(Error::Configuration(
                    "strict tool schema required contains an unknown property",
                ));
            }
            required
        }
    };
    for (name, property) in &mut properties {
        let nullable = schema_allows_null(property);
        normalize_strict_schema_node(property)?;
        if !required.contains(name) && !nullable {
            *property = serde_json::json!({"anyOf": [property.clone(), {"type": "null"}]});
        }
    }
    if had_properties {
        object.insert("properties".into(), serde_json::Value::Object(properties));
    }
    object.insert(
        "required".into(),
        serde_json::Value::Array(names.into_iter().map(serde_json::Value::String).collect()),
    );
    object.insert("additionalProperties".into(), serde_json::Value::Bool(false));
    Ok(())
}

fn is_structured_schema(schema: &serde_json::Value) -> bool {
    let Some(object) = schema.as_object() else { return false };
    let has_type = match object.get("type") {
        Some(serde_json::Value::String(kind)) => matches!(kind.as_str(), "object" | "array"),
        Some(serde_json::Value::Array(types)) => {
            types.iter().any(|kind| matches!(kind.as_str(), Some("object" | "array")))
        }
        _ => false,
    };
    has_type || object.contains_key("properties") || object.contains_key("items")
}

fn schema_allows_null(schema: &serde_json::Value) -> bool {
    let Some(object) = schema.as_object() else { return false };
    match object.get("type") {
        Some(serde_json::Value::String(kind)) if kind == "null" => return true,
        Some(serde_json::Value::Array(types))
            if types.iter().any(|kind| kind.as_str() == Some("null")) =>
        {
            return true;
        }
        _ => {}
    }
    if object.get("const").is_some_and(serde_json::Value::is_null)
        || object
            .get("enum")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|values| values.iter().any(serde_json::Value::is_null))
    {
        return true;
    }
    object
        .get("anyOf")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|variants| variants.iter().any(schema_allows_null))
}

// ---- stream decoding: OpenAI SSE chunks -> canonical `StreamEvent`s ----

#[derive(Deserialize)]
struct Chunk {
    #[serde(default)]
    error: Option<OpenAiStreamError>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Deserialize)]
struct OpenAiStreamError {
    #[serde(default, rename = "type")]
    r#type: Option<String>,
    #[serde(default)]
    message: String,
}

#[derive(Deserialize, Default)]
struct Choice {
    #[serde(default)]
    delta: ChoiceDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
struct ChoiceDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
    #[serde(default)]
    reasoning_text: Option<String>,
    #[serde(default)]
    reasoning_details: Option<serde_json::Value>,
    #[serde(default)]
    tool_calls: Vec<ToolCallDelta>,
}

#[derive(Deserialize)]
struct ToolCallDelta {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<FunctionDelta>,
}

#[derive(Deserialize, Default)]
struct FunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
}

/// Folds `OpenAI`'s per-choice, per-delta chunks into the canonical
/// `ContentBlockStart`/`ContentBlockDelta` sequence [`crate::stream::Accumulator`]
/// already knows how to read — a text block (if any) is always canonical
/// index 0; each of `OpenAI`'s `tool_calls[].index` values gets its own
/// canonical index, assigned the first time that index is seen.
#[derive(Default)]
struct Translate {
    message_started: bool,
    text_index: Option<usize>,
    thinking_index: Option<usize>,
    thinking_started: bool,
    tool_indices: HashMap<usize, usize>,
    tool_ids: HashMap<usize, String>,
    tool_names: HashMap<usize, String>,
    tool_arguments: HashMap<usize, String>,
    finished: bool,
    next_index: usize,
}

impl Translate {
    fn settlement_error(&self) -> Option<String> {
        if !self.finished {
            return Some("stream ended before finish_reason".into());
        }
        for (index, id) in &self.tool_ids {
            if id.is_empty() || self.tool_names.get(index).is_none_or(String::is_empty) {
                return Some("tool call is missing an id or name".into());
            }
            let args = self.tool_arguments.get(index).map_or("", String::as_str);
            if !args.trim().is_empty() && serde_json::from_str::<serde_json::Value>(args).is_err() {
                return Some("tool call arguments are malformed JSON".into());
            }
        }
        None
    }

    fn apply(&mut self, chunk: Chunk) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        if !self.message_started {
            self.message_started = true;
            out.push(StreamEvent::MessageStart {
                message: MessageStartInfo {
                    id: chunk.id.clone().unwrap_or_default(),
                    model: chunk.model.clone().unwrap_or_default(),
                    usage: Usage::default(),
                },
            });
        }
        if let Some(choice) = chunk.choices.into_iter().next() {
            out.extend(self.apply_choice(choice));
        }
        if let Some(usage) = chunk.usage {
            out.push(StreamEvent::MessageDelta {
                delta: MessageDeltaInfo { stop_reason: None, stop_details: None },
                usage: Some(Usage {
                    input_tokens: usage.prompt_tokens,
                    output_tokens: usage.completion_tokens,
                    ..Default::default()
                }),
            });
        }
        out
    }

    fn apply_choice(&mut self, choice: Choice) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        let Choice { delta, finish_reason } = choice;
        let ChoiceDelta {
            content,
            reasoning_content,
            reasoning,
            reasoning_text,
            reasoning_details,
            tool_calls,
        } = delta;
        let (reasoning_field, reasoning_value) =
            if reasoning_content.as_deref().is_some_and(|s| !s.is_empty()) {
                (Some("reasoning_content"), reasoning_content)
            } else if reasoning.as_deref().is_some_and(|s| !s.is_empty()) {
                (Some("reasoning"), reasoning)
            } else if reasoning_text.as_deref().is_some_and(|s| !s.is_empty()) {
                (Some("reasoning_text"), reasoning_text)
            } else {
                (None, None)
            };
        out.extend(self.apply_reasoning(reasoning_field, reasoning_value, reasoning_details));
        if let Some(text) = content {
            out.extend(self.apply_text(text));
        }
        out.extend(self.apply_tool_calls(tool_calls));
        if let Some(reason) = finish_reason {
            self.finished = true;
            out.push(StreamEvent::MessageDelta {
                delta: MessageDeltaInfo {
                    stop_reason: Some(map_finish_reason(&reason)),
                    stop_details: None,
                },
                usage: None,
            });
        }
        out
    }

    fn apply_reasoning(
        &mut self,
        field: Option<&str>,
        text: Option<String>,
        details: Option<serde_json::Value>,
    ) -> Vec<StreamEvent> {
        if text.is_none() && details.is_none() {
            return Vec::new();
        }
        let index = *self.thinking_index.get_or_insert_with(|| {
            let index = self.next_index;
            self.next_index += 1;
            index
        });
        let mut out = Vec::new();
        if !self.thinking_started {
            self.thinking_started = true;
            out.push(StreamEvent::ContentBlockStart {
                index,
                content_block: ContentBlock::Thinking { thinking: String::new(), signature: None },
            });
        }
        if let Some(thinking) = text {
            out.push(StreamEvent::ContentBlockDelta {
                index,
                delta: Delta::ThinkingDelta { thinking },
            });
            out.push(StreamEvent::ContentBlockDelta {
                index,
                delta: Delta::SignatureDelta {
                    signature: field.unwrap_or("reasoning_content").to_owned(),
                },
            });
        }
        if let Some(details) = details {
            out.push(StreamEvent::ContentBlockDelta {
                index,
                delta: Delta::SignatureDelta {
                    signature: format!("nahida:reasoning-details:{details}"),
                },
            });
        }
        out
    }

    fn apply_text(&mut self, text: String) -> Vec<StreamEvent> {
        let first_sight = self.text_index.is_none();
        let index = *self.text_index.get_or_insert_with(|| {
            let index = self.next_index;
            self.next_index += 1;
            index
        });
        let mut out = Vec::new();
        if first_sight {
            out.push(StreamEvent::ContentBlockStart {
                index,
                content_block: ContentBlock::Text { text: String::new(), cache_control: None },
            });
        }
        out.push(StreamEvent::ContentBlockDelta { index, delta: Delta::TextDelta { text } });
        out
    }

    fn apply_tool_calls(&mut self, calls: Vec<ToolCallDelta>) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        for call in calls {
            let first_sight = !self.tool_indices.contains_key(&call.index);
            let index = *self.tool_indices.entry(call.index).or_insert_with(|| {
                let index = self.next_index;
                self.next_index += 1;
                index
            });
            if first_sight {
                let id = call.id.clone().unwrap_or_default();
                let name = call
                    .function
                    .as_ref()
                    .and_then(|function| function.name.clone())
                    .unwrap_or_default();
                self.tool_ids.insert(call.index, id.clone());
                self.tool_names.insert(call.index, name.clone());
                out.push(StreamEvent::ContentBlockStart {
                    index,
                    content_block: ContentBlock::ToolUse { id, name, input: serde_json::json!({}) },
                });
            }
            if let Some(function) = call.function
                && let Some(arguments) = function.arguments
            {
                self.tool_arguments.entry(call.index).or_default().push_str(&arguments);
                out.push(StreamEvent::ContentBlockDelta {
                    index,
                    delta: Delta::InputJsonDelta { partial_json: arguments },
                });
            }
        }
        out
    }
}

fn map_finish_reason(reason: &str) -> StopReason {
    match reason {
        "stop" => StopReason::EndTurn,
        "length" => StopReason::MaxTokens,
        "tool_calls" => StopReason::ToolUse,
        // Closest existing canonical meaning: content was blocked, so
        // `content` is not a complete answer -- the same contract
        // `StopReason::Refusal` already documents for Anthropic's safety
        // classifiers, even though the underlying mechanism differs.
        "content_filter" => StopReason::Refusal,
        _ => StopReason::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Dialect;
    use crate::stream::Accumulator;
    use crate::types::{Effort, OutputConfig, SystemBlock, Thinking};
    use futures_util::StreamExt as _;

    fn profile() -> Profile {
        Profile {
            name: "zai-coding-cn".into(),
            base_url: "https://open.bigmodel.cn/api/coding/paas/v4".to_string(),
            dialect: Dialect::Compat,
            default_model: "glm-5.3".to_string(),
            default_max_tokens: 32_000,
        }
    }

    fn zai_options() -> OpenAiOptions {
        OpenAiOptions {
            supports_store: Some(false),
            supports_developer_role: Some(false),
            supports_reasoning_effort: Some(true),
            max_tokens_field: Some(OpenAiMaxTokensField::MaxTokens),
            thinking_format: Some(OpenAiThinkingFormat::Zai),
            supports_strict_mode: Some(true),
            zai_tool_stream: Some(true),
            reasoning: true,
            thinking_level_map: Some(BTreeMap::from([
                ("low".into(), Some("low".into())),
                ("medium".into(), None),
                ("high".into(), Some("high".into())),
                ("xhigh".into(), None),
                ("max".into(), Some("max".into())),
            ])),
        }
    }

    // ---- request encoding ----

    #[test]
    fn selected_zai_options_encode_exact_request_and_strict_tool_schema() {
        let req = Request {
            model: "glm-5.3-flash".into(),
            max_tokens: 1024,
            system: vec![SystemBlock::new("sys")],
            messages: vec![Message::user(vec![ContentBlock::text("hi")])],
            tools: vec![ToolSpec {
                name: "read".into(),
                description: "Read".into(),
                input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
                cache_control: None,
            }],
            output_config: Some(OutputConfig { effort: Some(Effort::Low) }),
            thinking: None,
            stream: true,
        };
        let value = serde_json::to_value(encode_request(&req, &profile(), &zai_options()).unwrap())
            .unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "model":"glm-5.3-flash", "messages":[{"role":"system","content":"sys"},{"role":"user","content":"hi"}],
                "tools":[{"type":"function","function":{"name":"read","description":"Read","parameters":{"type":"object","properties":{"path":{"type":"string"}},"additionalProperties":false,"required":["path"]},"strict":true}}],
                "max_tokens":1024,"stream":true,"stream_options":{"include_usage":true},
                "thinking":{"type":"enabled","clear_thinking":false},"reasoning_effort":"low","tool_stream":true
            })
        );
    }

    #[test]
    fn strict_read_and_grep_parameters_make_only_optional_arguments_nullable() {
        let read = serde_json::json!({
            "type":"object",
            "properties":{
                "path":{"type":"string","description":"path"},
                "offset":{"type":"integer","description":"offset"},
                "limit":{"type":"integer","description":"limit"}
            },
            "required":["path"],"additionalProperties":false
        });
        let normalized = normalize_strict_schema(&read).unwrap();
        assert_eq!(normalized["required"], serde_json::json!(["limit", "offset", "path"]));
        assert_eq!(normalized["properties"]["path"], read["properties"]["path"]);
        for key in ["offset", "limit"] {
            assert_eq!(normalized["properties"][key]["anyOf"][0], read["properties"][key]);
            assert_eq!(
                normalized["properties"][key]["anyOf"][1],
                serde_json::json!({"type":"null"})
            );
        }

        let grep = serde_json::json!({
            "type":"object",
            "properties":{
                "pattern":{"type":"string"},"path":{"type":"string"},
                "glob":{"type":"string"},"ignore_case":{"type":"boolean"},
                "literal":{"type":"boolean"},"context":{"type":"integer"},"limit":{"type":"integer"}
            },
            "required":["pattern"],"additionalProperties":false
        });
        let normalized = normalize_strict_schema(&grep).unwrap();
        assert_eq!(normalized["required"].as_array().unwrap().len(), 7);
        assert_eq!(normalized["properties"]["pattern"], grep["properties"]["pattern"]);
        for key in ["path", "glob", "ignore_case", "literal", "context", "limit"] {
            assert_eq!(normalized["properties"][key]["anyOf"][0], grep["properties"][key]);
            assert_eq!(
                normalized["properties"][key]["anyOf"][1],
                serde_json::json!({"type":"null"})
            );
        }
    }

    #[test]
    fn strict_schema_recurses_through_nested_objects_and_array_items() {
        let schema = serde_json::json!({
            "type":"object","properties":{
                "required":{"type":"string"},
                "child":{"type":"object","properties":{"name":{"type":"string"},"count":{"type":"integer"}},"required":["name"],"additionalProperties":false},
                "entries":{"type":"array","items":{"type":"object","properties":{"value":{"type":"string"},"enabled":{"type":"boolean"}},"required":["value"],"additionalProperties":false}}
            },"required":["required"],"additionalProperties":false
        });
        let normalized = normalize_strict_schema(&schema).unwrap();
        assert_eq!(normalized["properties"]["required"], schema["properties"]["required"]);
        let child = &normalized["properties"]["child"]["anyOf"][0];
        assert_eq!(child["required"], serde_json::json!(["count", "name"]));
        assert_eq!(
            child["properties"]["name"],
            schema["properties"]["child"]["properties"]["name"]
        );
        assert_eq!(child["properties"]["count"]["anyOf"][1]["type"], "null");
        let item = &normalized["properties"]["entries"]["anyOf"][0]["items"];
        assert_eq!(item["required"], serde_json::json!(["enabled", "value"]));
        assert_eq!(item["properties"]["enabled"]["anyOf"][1]["type"], "null");
    }

    #[test]
    fn strict_schema_preserves_nullable_and_primitive_union_properties() {
        let schema = serde_json::json!({
            "type":"object","properties":{
                "type_array":{"type":["string","null"]},
                "any_of":{"anyOf":[{"type":"integer"},{"type":"null"}]},
                "enum_null":{"type":"string","enum":["a",null]},
                "primitive_union":{"anyOf":[{"type":"string"},{"type":"integer"}]}
            },"required":[],"additionalProperties":false
        });
        let normalized = normalize_strict_schema(&schema).unwrap();
        for key in ["type_array", "any_of", "enum_null"] {
            assert_eq!(normalized["properties"][key], schema["properties"][key]);
        }
        let union = &normalized["properties"]["primitive_union"]["anyOf"];
        assert_eq!(union[0], schema["properties"]["primitive_union"]);
        assert_eq!(union[1], serde_json::json!({"type":"null"}));
    }

    #[test]
    fn strict_schema_rejects_unsupported_or_malformed_structures() {
        let invalid = [
            serde_json::json!({"type":"object","properties":{"x":true}}),
            serde_json::json!({"type":"object","properties":{"x":{"$ref":"#/defs/x"}}}),
            serde_json::json!({"type":"object","properties":{"x":{"anyOf":[{"type":"object"},{"type":"string"}]}}}),
            serde_json::json!({"type":"object","properties":{"x":{"type":"array","items":[{"type":"string"}]}}}),
            serde_json::json!({"type":"object","properties":{"x":{"type":"string"}},"required":["missing"]}),
            serde_json::json!({"type":"object","properties":{"x":{"type":"string"}},"additionalProperties":true}),
            serde_json::json!({"type":"string"}),
        ];
        for schema in invalid {
            assert!(
                normalize_strict_schema(&schema).is_err(),
                "accepted unsupported schema: {schema}"
            );
        }
    }

    #[test]
    fn unsupported_null_reasoning_effort_fails_during_request_encoding() {
        let mut req = Request {
            model: "glm-5.3-flash".into(),
            max_tokens: 1024,
            system: vec![],
            messages: vec![Message::user(vec![ContentBlock::text("hi")])],
            tools: vec![],
            output_config: Some(OutputConfig { effort: Some(Effort::Medium) }),
            thinking: None,
            stream: true,
        };
        assert!(matches!(
            encode_request(&req, &profile(), &zai_options()),
            Err(Error::Configuration(_))
        ));
        req.output_config = None;
        let encoded = encode_request(&req, &profile(), &zai_options()).unwrap();
        let value = serde_json::to_value(encoded).unwrap();
        assert_eq!(value["thinking"], serde_json::json!({"type":"disabled"}));
        assert!(value.get("reasoning_effort").is_none());
    }

    #[test]
    fn reasoning_metadata_is_replayed_once_with_assistant_tool_calls() {
        let details = serde_json::json!([{"type":"reasoning.text","text":"internal"}]);
        let signature = format!("nahida:reasoning-details:{details}");
        let req = Request {
            model: "glm-5.3-flash".into(),
            max_tokens: 1024,
            system: vec![],
            messages: vec![Message::assistant(vec![
                ContentBlock::Thinking {
                    thinking: "considering".into(),
                    signature: Some(signature),
                },
                ContentBlock::ToolUse {
                    id: "call_1".into(),
                    name: "read".into(),
                    input: serde_json::json!({"path":"x"}),
                },
            ])],
            tools: vec![],
            output_config: None,
            thinking: None,
            stream: true,
        };
        let value = serde_json::to_value(encode_request(&req, &profile(), &zai_options()).unwrap())
            .unwrap();
        assert_eq!(value["messages"][0]["reasoning_details"], details);
        assert_eq!(value["messages"][0]["tool_calls"][0]["id"], "call_1");
        assert!(value["messages"][0].get("reasoning_content").is_none());
    }

    #[test]
    fn streamed_reasoning_becomes_neutral_thinking_and_replays_raw_field() {
        let mut translate = Translate::default();
        let mut accumulator = Accumulator::new();
        for event in
            translate.apply(chunk(r#"{"choices":[{"delta":{"reasoning_content":"think"}}]}"#))
        {
            accumulator.apply(&event);
        }
        for event in
            translate.apply(chunk(r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#))
        {
            accumulator.apply(&event);
        }
        let response = accumulator.finish();
        match &response.content[0] {
            ContentBlock::Thinking { thinking, signature } => {
                assert_eq!(thinking, "think");
                assert_eq!(signature.as_deref(), Some("reasoning_content"));
            }
            other => panic!("unexpected block: {other:?}"),
        }
        let request = Request {
            model: "glm-5.3-flash".into(),
            max_tokens: 1024,
            system: vec![],
            messages: vec![Message::assistant(response.replayable())],
            tools: vec![],
            output_config: None,
            thinking: None,
            stream: true,
        };
        let value =
            serde_json::to_value(encode_request(&request, &profile(), &zai_options()).unwrap())
                .unwrap();
        assert_eq!(value["messages"][0]["reasoning_content"], "think");
    }

    #[tokio::test]
    async fn truncated_tool_stream_errors_without_successful_response() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 4096];
            let _ = socket.read(&mut request).await.unwrap();
            let body = concat!(
                r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"write","arguments":"{\"path\":\"x\"}"}}]}}]}"#,
                "\n\n",
                r#"data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
                "\n\n"
            );
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{body}").as_bytes()).await.unwrap();
        });
        let mut profile = profile();
        profile.base_url = format!("http://{address}");
        let provider = OpenAiCompletionsProvider::new("synthetic", profile).unwrap();
        let request = Request {
            model: "glm-5.3-flash".into(),
            max_tokens: 100,
            system: vec![],
            messages: vec![Message::user(vec![ContentBlock::text("test")])],
            tools: vec![],
            output_config: None,
            thinking: None,
            stream: true,
        };
        let mut events = provider.stream(&request).await.unwrap();
        let mut saw_error = false;
        let mut saw_success_stop = false;
        while let Some(event) = events.next().await {
            match event {
                Err(Error::Stream { .. }) => {
                    saw_error = true;
                    break;
                }
                Ok(StreamEvent::MessageStop) => saw_success_stop = true,
                Ok(_) => {}
                Err(error) => panic!("unexpected stream error: {error}"),
            }
        }
        assert!(saw_error, "truncated response must fail");
        assert!(!saw_success_stop, "truncated response must not report success");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn malformed_tool_stream_errors_without_successful_response() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 4096];
            let _ = socket.read(&mut request).await.unwrap();
            let body = concat!(
                r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"write","arguments":"{bad"}}]}}]}"#,
                "\n\n",
                r#"data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
                "\n\n",
                "data: [DONE]\n\n"
            );
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{body}").as_bytes()).await.unwrap();
        });
        let mut profile = profile();
        profile.base_url = format!("http://{address}");
        let provider = OpenAiCompletionsProvider::new("synthetic", profile).unwrap();
        let request = Request {
            model: "glm-5.3-flash".into(),
            max_tokens: 100,
            system: vec![],
            messages: vec![Message::user(vec![ContentBlock::text("test")])],
            tools: vec![],
            output_config: None,
            thinking: None,
            stream: true,
        };
        let mut events = provider.stream(&request).await.unwrap();
        let mut saw_error = false;
        let mut saw_success_stop = false;
        while let Some(event) = events.next().await {
            match event {
                Err(Error::Stream { .. }) => {
                    saw_error = true;
                    break;
                }
                Ok(StreamEvent::MessageStop) => saw_success_stop = true,
                Ok(_) => {}
                Err(error) => panic!("unexpected stream error: {error}"),
            }
        }
        assert!(saw_error, "malformed tool JSON must fail settlement");
        assert!(!saw_success_stop, "malformed tool JSON must not report success");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn text_progress_is_live_and_done_settles_before_http_eof() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        async fn write_chunk(socket: &mut tokio::net::TcpStream, payload: &str) {
            socket
                .write_all(format!("{:X}\r\n{payload}\r\n", payload.len()).as_bytes())
                .await
                .unwrap();
            socket.flush().await.unwrap();
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 4096];
            let _ = socket.read(&mut request).await.unwrap();
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: keep-alive\r\n\r\n").await.unwrap();
            write_chunk(
                &mut socket,
                r#"data: {"choices":[{"delta":{"content":"pulse"}}]}

"#,
            )
            .await;
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            write_chunk(&mut socket, "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n").await;
            // Keep the response body open after DONE. A conforming consumer has
            // already received MessageStop and EOF from the provider stream.
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
            socket.write_all(b"0\r\n\r\n").await.unwrap();
        });
        let mut profile = profile();
        profile.base_url = format!("http://{address}");
        let provider = OpenAiCompletionsProvider::new("synthetic", profile).unwrap();
        let request = Request {
            model: "glm-5.3-flash".into(),
            max_tokens: 100,
            system: vec![],
            messages: vec![Message::user(vec![ContentBlock::text("test")])],
            tools: vec![],
            output_config: None,
            thinking: None,
            stream: true,
        };
        let mut events = provider.stream(&request).await.unwrap();
        let progress = tokio::time::timeout(std::time::Duration::from_millis(180), async {
            while let Some(event) = events.next().await {
                match event {
                    Ok(StreamEvent::ContentBlockDelta {
                        delta: Delta::TextDelta { text }, ..
                    }) if text == "pulse" => return true,
                    Ok(_) => {}
                    Err(error) => panic!("unexpected stream error: {error}"),
                }
            }
            false
        })
        .await
        .expect("text progress should arrive before server releases finish");
        assert!(progress);

        let mut saw_stop = false;
        let completed = tokio::time::timeout(std::time::Duration::from_millis(500), async {
            while let Some(event) = events.next().await {
                match event {
                    Ok(StreamEvent::MessageStop) => saw_stop = true,
                    Ok(_) => {}
                    Err(error) => panic!("unexpected stream error: {error}"),
                }
            }
        })
        .await;
        assert!(completed.is_ok(), "DONE must settle without waiting for HTTP EOF");
        assert!(saw_stop, "valid [DONE] should emit MessageStop");
        server.await.unwrap();
    }

    #[test]
    fn malformed_tool_json_and_missing_finish_are_unsettled() {
        let mut translate = Translate::default();
        translate.apply(chunk(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"read","arguments":"{bad"}}]}}]}"#));
        translate.apply(chunk(r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#));
        assert!(translate.settlement_error().unwrap().contains("malformed"));
        let partial = Translate::default();
        assert!(partial.settlement_error().unwrap().contains("finish_reason"));
    }

    #[test]
    fn a_plain_user_turn_encodes_as_system_and_user_messages() {
        let req = Request {
            model: "glm-5.3".to_string(),
            max_tokens: 1024,
            system: vec![SystemBlock::new("be terse")],
            messages: vec![Message::user(vec![ContentBlock::text("hi")])],
            tools: vec![],
            output_config: None,
            thinking: None,
            stream: true,
        };
        let encoded = encode_request(&req, &profile(), &OpenAiOptions::default()).unwrap();

        assert_eq!(encoded.messages.len(), 2);
        assert_eq!(encoded.messages[0].role, "system");
        assert_eq!(encoded.messages[0].content.as_deref(), Some("be terse"));
        assert_eq!(encoded.messages[1].role, "user");
        assert_eq!(encoded.messages[1].content.as_deref(), Some("hi"));
        assert!(encoded.stream_options.include_usage);
    }

    #[test]
    fn anthropic_only_fields_are_silently_dropped_not_forwarded() {
        // `effort`/`thinking` have no OpenAI Chat Completions equivalent and
        // are documented Anthropic-only (README's --effort flag help) --
        // OpenAiRequest has no field for either, so this is really just
        // confirming encoding a request that carries them doesn't panic or
        // pick up stray content.
        let req = Request {
            model: "glm-5.3".to_string(),
            max_tokens: 1024,
            system: vec![],
            messages: vec![Message::assistant(vec![
                ContentBlock::Thinking { thinking: "hmm".to_string(), signature: None },
                ContentBlock::text("ok"),
            ])],
            tools: vec![],
            output_config: Some(OutputConfig { effort: Some(Effort::High) }),
            thinking: Some(Thinking::summarized()),
            stream: true,
        };
        let encoded = encode_request(&req, &profile(), &OpenAiOptions::default()).unwrap();

        assert_eq!(encoded.messages.len(), 1);
        assert_eq!(encoded.messages[0].role, "assistant");
        assert_eq!(encoded.messages[0].content.as_deref(), Some("ok"));
    }

    #[test]
    fn an_assistant_tool_use_encodes_as_a_tool_call_with_json_string_arguments() {
        let req = Request {
            model: "glm-5.3".to_string(),
            max_tokens: 1024,
            system: vec![],
            messages: vec![Message::assistant(vec![ContentBlock::ToolUse {
                id: "call_1".to_string(),
                name: "read".to_string(),
                input: serde_json::json!({"path": "a.rs"}),
            }])],
            tools: vec![],
            output_config: None,
            thinking: None,
            stream: true,
        };
        let encoded = encode_request(&req, &profile(), &OpenAiOptions::default()).unwrap();

        assert_eq!(encoded.messages.len(), 1);
        assert!(encoded.messages[0].content.is_none(), "no text alongside the call");
        let calls = encoded.messages[0].tool_calls.as_ref().expect("tool_calls");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].function.name, "read");
        assert_eq!(calls[0].function.arguments, r#"{"path":"a.rs"}"#);
    }

    #[test]
    fn batched_tool_results_become_separate_tool_role_messages() {
        // nahida's own rule (AGENTS.md #3): every tool result rides in one
        // *canonical* user message. OpenAI has no batched shape for that --
        // each becomes its own `role: "tool"` message.
        let req = Request {
            model: "glm-5.3".to_string(),
            max_tokens: 1024,
            system: vec![],
            messages: vec![Message::user(vec![
                ContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: "4 lines".to_string(),
                    is_error: false,
                    cache_control: None,
                },
                ContentBlock::ToolResult {
                    tool_use_id: "call_2".to_string(),
                    content: "no such file".to_string(),
                    is_error: true,
                    cache_control: None,
                },
            ])],
            tools: vec![],
            output_config: None,
            thinking: None,
            stream: true,
        };
        let encoded = encode_request(&req, &profile(), &OpenAiOptions::default()).unwrap();

        assert_eq!(encoded.messages.len(), 2);
        for m in &encoded.messages {
            assert_eq!(m.role, "tool");
        }
        assert_eq!(encoded.messages[0].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(encoded.messages[0].content.as_deref(), Some("4 lines"));
        assert_eq!(encoded.messages[1].tool_call_id.as_deref(), Some("call_2"));
    }

    // ---- stream decoding ----

    fn chunk(json: &str) -> Chunk {
        serde_json::from_str(json).expect("chunk parses")
    }

    #[test]
    fn a_text_delta_synthesizes_its_own_content_block_start() {
        let mut t = Translate::default();
        let events = t.apply(chunk(
            r#"{"id":"c1","model":"glm-5.3","choices":[{"delta":{"content":"hi"}}]}"#,
        ));
        assert!(matches!(events[0], StreamEvent::MessageStart { .. }));
        assert!(matches!(
            events[1],
            StreamEvent::ContentBlockStart { index: 0, content_block: ContentBlock::Text { .. } }
        ));
        assert!(matches!(events[2], StreamEvent::ContentBlockDelta { index: 0, .. }));

        // A second delta for the same choice must not re-synthesize a start.
        let more = t.apply(chunk(r#"{"choices":[{"delta":{"content":" there"}}]}"#));
        assert_eq!(more.len(), 1);
        assert!(matches!(more[0], StreamEvent::ContentBlockDelta { index: 0, .. }));
    }

    #[test]
    fn a_tool_calls_delta_starts_on_first_sight_then_only_streams_arguments() {
        let mut t = Translate::default();
        let start = t.apply(chunk(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read","arguments":""}}]}}]}"#,
        ));
        let has_start = start.iter().any(|e| {
            matches!(e, StreamEvent::ContentBlockStart { content_block: ContentBlock::ToolUse { id, name, .. }, .. } if id == "call_1" && name == "read")
        });
        assert!(has_start, "{start:?}");

        let fragment = t.apply(chunk(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\""}}]}}]}"#,
        ));
        assert!(matches!(
            fragment[0],
            StreamEvent::ContentBlockDelta { index: 0, delta: Delta::InputJsonDelta { .. } }
        ));
        // No second ContentBlockStart for the same tool_calls index.
        assert_eq!(fragment.len(), 1);
    }

    #[test]
    fn finish_reason_tool_calls_maps_to_canonical_tool_use() {
        let mut t = Translate::default();
        let events = t.apply(chunk(r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#));
        let stop = events.iter().find_map(|e| match e {
            StreamEvent::MessageDelta { delta, .. } => delta.stop_reason,
            _ => None,
        });
        assert_eq!(stop, Some(StopReason::ToolUse));
    }

    #[test]
    fn a_trailing_usage_only_chunk_reports_both_token_counts() {
        let mut t = Translate::default();
        let events =
            t.apply(chunk(r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":7}}"#));
        let usage = events.iter().find_map(|e| match e {
            StreamEvent::MessageDelta { usage, .. } => *usage,
            _ => None,
        });
        let usage = usage.expect("a usage-carrying delta");
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 7);
    }

    /// End to end: a full streamed tool-call turn, translated and then folded
    /// by the same `Accumulator` the Anthropic path uses -- proving the two
    /// systems compose, not just that each half looks right in isolation.
    #[test]
    fn a_translated_tool_call_turn_folds_into_the_right_response() {
        let chunks = [
            r#"{"id":"c1","model":"glm-5.3","choices":[{"delta":{"content":"Looking."}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read","arguments":""}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\""}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":": \"a.rs\"}"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":7}}"#,
        ];

        let mut translate = Translate::default();
        let mut acc = Accumulator::new();
        for c in chunks {
            for event in translate.apply(chunk(c)) {
                acc.apply(&event);
            }
        }
        let resp = acc.finish();

        assert_eq!(resp.stop_reason, Some(StopReason::ToolUse));
        assert_eq!(resp.text(), "Looking.");
        let uses = resp.tool_uses();
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].1, "read");
        assert_eq!(uses[0].2["path"], "a.rs");
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.usage.output_tokens, 7);
    }

    #[test]
    fn unrecognized_finish_reason_maps_to_unknown_not_a_panic() {
        assert_eq!(map_finish_reason("something_new"), StopReason::Unknown);
        assert_eq!(map_finish_reason("content_filter"), StopReason::Refusal);
        assert_eq!(map_finish_reason("stop"), StopReason::EndTurn);
        assert_eq!(map_finish_reason("length"), StopReason::MaxTokens);
    }
}
