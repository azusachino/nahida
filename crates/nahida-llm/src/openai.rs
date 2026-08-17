//! The `OpenAI` Chat Completions wire format, as a second [`crate::Provider`].
//!
//! Exists for one concrete reason: the Z.ai coding plan's China-region
//! endpoint (`open.bigmodel.cn`) speaks this, not Anthropic Messages —
//! confirmed against `earendil-works/pi`'s own `zai-coding-cn` provider,
//! which uses this wire format exclusively for that endpoint. This module
//! translates in both directions so nothing above [`crate::provider::Provider`]
//! (in particular, `nahida-agent`'s loop) ever sees a non-canonical shape.
//!
//! Scoped to what nahida actually needs: tool calling and streaming text,
//! bearer auth only. No `reasoning_content`, no `max_completion_tokens`
//! variant, no grammar-constrained sampling, no provider-specific header
//! quirks — pi's own `openai-completions.ts` (1500+ lines) accumulates all of
//! that from real production traffic across dozens of providers; this is the
//! part of it a single coding-plan endpoint needs.

use std::collections::{HashMap, VecDeque};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::client::{Error, Profile, Result};
use crate::provider::{EventStream, Provider};
use crate::stream::{Delta, MessageDeltaInfo, MessageStartInfo, SseDecoder, StreamEvent};
use crate::types::{ContentBlock, Message, Request, Role, StopReason, ToolSpec, Usage};

pub struct OpenAiCompletionsProvider {
    http: reqwest::Client,
    token: String,
    profile: Profile,
}

impl OpenAiCompletionsProvider {
    pub fn new(token: impl Into<String>, profile: Profile) -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder().timeout(std::time::Duration::from_mins(10)).build()?,
            token: token.into(),
            profile,
        })
    }
}

#[async_trait]
impl Provider for OpenAiCompletionsProvider {
    fn profile(&self) -> &Profile {
        &self.profile
    }

    async fn stream(&self, req: &Request) -> Result<EventStream> {
        let body = encode_request(req, &self.profile);
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
        };

        Ok(Box::pin(futures_util::stream::unfold(state, |mut st| async move {
            loop {
                if let Some(event) = st.pending.pop_front() {
                    return Some((Ok(event), st));
                }

                if st.body_done {
                    return None;
                }

                match futures_util::StreamExt::next(&mut st.bytes).await {
                    Some(Ok(chunk)) => {
                        for frame in st.decoder.push(&chunk) {
                            if frame.trim() == "[DONE]" {
                                st.pending.push_back(StreamEvent::MessageStop);
                                continue;
                            }
                            match serde_json::from_str::<Chunk>(&frame) {
                                Ok(chunk) => st.pending.extend(st.translate.apply(chunk)),
                                Err(source) => {
                                    st.body_done = true;
                                    st.pending.push_back(StreamEvent::Error {
                                        error: crate::types::ApiErrorBody {
                                            r#type: "decode_error".to_string(),
                                            message: source.to_string(),
                                        },
                                    });
                                }
                            }
                        }
                    }
                    Some(Err(e)) => {
                        st.body_done = true;
                        return Some((Err(Error::Transport(e)), st));
                    }
                    None => st.body_done = true,
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
}

fn encode_request(req: &Request, profile: &Profile) -> OpenAiRequest {
    let mut messages = Vec::new();

    let system: String =
        req.system.iter().map(|b| b.text.as_str()).collect::<Vec<_>>().join("\n\n");
    if !system.is_empty() {
        messages.push(OpenAiMessage {
            role: "system",
            content: Some(system),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    for message in &req.messages {
        encode_message(message, &mut messages);
    }

    OpenAiRequest {
        model: req.model.clone(),
        messages,
        tools: req.tools.iter().map(encode_tool).collect(),
        // OpenAI-completions gateways vary on how generous a default is
        // sensible; the caller's own request already carries a real cap.
        max_tokens: req.max_tokens.min(profile.default_max_tokens.max(req.max_tokens)),
        stream: true,
        stream_options: StreamOptions { include_usage: true },
    }
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
                });
            }
        }
        Role::Assistant => {
            let mut text = String::new();
            let mut tool_calls = Vec::new();
            for block in &message.content {
                match block {
                    ContentBlock::Text { text: t, .. } => text.push_str(t),
                    ContentBlock::ToolUse { id, name, input } => {
                        tool_calls.push(OpenAiToolCall {
                            id: id.clone(),
                            r#type: "function",
                            function: OpenAiFunctionCall {
                                name: name.clone(),
                                arguments: input.to_string(),
                            },
                        });
                    }
                    // Thinking is Anthropic-only (see module doc); Unknown
                    // blocks were never meant to be replayed; an assistant
                    // turn never carries a ToolResult.
                    ContentBlock::Thinking { .. }
                    | ContentBlock::Unknown
                    | ContentBlock::ToolResult { .. } => {}
                }
            }
            out.push(OpenAiMessage {
                role: "assistant",
                content: if text.is_empty() { None } else { Some(text) },
                tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
                tool_call_id: None,
            });
        }
    }
}

fn encode_tool(tool: &ToolSpec) -> OpenAiTool {
    OpenAiTool {
        r#type: "function",
        function: OpenAiFunctionDef {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: tool.input_schema.clone(),
        },
    }
}

// ---- stream decoding: OpenAI SSE chunks -> canonical `StreamEvent`s ----

#[derive(Deserialize)]
struct Chunk {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
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
    tool_indices: HashMap<usize, usize>,
    next_index: usize,
}

impl Translate {
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
            if let Some(text) = choice.delta.content {
                let first_sight = self.text_index.is_none();
                let index = *self.text_index.get_or_insert_with(|| {
                    let i = self.next_index;
                    self.next_index += 1;
                    i
                });
                // `Accumulator` drops a delta for a block it never saw start
                // (`self.blocks.get_mut` returns `None` past the vec's
                // length) -- unlike a tool call, which arrives with its
                // `id`/`name` up front, OpenAI's first text delta *is* the
                // first sight of the block, so this has to synthesize the
                // start itself rather than reading it off the chunk.
                if first_sight {
                    out.push(StreamEvent::ContentBlockStart {
                        index,
                        content_block: ContentBlock::Text {
                            text: String::new(),
                            cache_control: None,
                        },
                    });
                }
                out.push(StreamEvent::ContentBlockDelta {
                    index,
                    delta: Delta::TextDelta { text },
                });
            }

            for tc in choice.delta.tool_calls {
                let first_sight = !self.tool_indices.contains_key(&tc.index);
                let index = *self.tool_indices.entry(tc.index).or_insert_with(|| {
                    let i = self.next_index;
                    self.next_index += 1;
                    i
                });
                if first_sight {
                    let id = tc.id.clone().unwrap_or_default();
                    let name =
                        tc.function.as_ref().and_then(|f| f.name.clone()).unwrap_or_default();
                    out.push(StreamEvent::ContentBlockStart {
                        index,
                        content_block: ContentBlock::ToolUse {
                            id,
                            name,
                            input: serde_json::json!({}),
                        },
                    });
                }
                if let Some(args) = tc.function.and_then(|f| f.arguments) {
                    out.push(StreamEvent::ContentBlockDelta {
                        index,
                        delta: Delta::InputJsonDelta { partial_json: args },
                    });
                }
            }

            if let Some(reason) = choice.finish_reason {
                out.push(StreamEvent::MessageDelta {
                    delta: MessageDeltaInfo {
                        stop_reason: Some(map_finish_reason(&reason)),
                        stop_details: None,
                    },
                    usage: None,
                });
            }
        }

        if let Some(u) = chunk.usage {
            out.push(StreamEvent::MessageDelta {
                delta: MessageDeltaInfo { stop_reason: None, stop_details: None },
                usage: Some(Usage {
                    input_tokens: u.prompt_tokens,
                    output_tokens: u.completion_tokens,
                    ..Default::default()
                }),
            });
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

    fn profile() -> Profile {
        Profile {
            name: "zai-coding-cn",
            base_url: "https://open.bigmodel.cn/api/coding/paas/v4".to_string(),
            dialect: Dialect::Compat,
            default_model: "glm-5.3".to_string(),
            default_max_tokens: 32_000,
        }
    }

    // ---- request encoding ----

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
        let encoded = encode_request(&req, &profile());

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
        let encoded = encode_request(&req, &profile());

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
        let encoded = encode_request(&req, &profile());

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
        let encoded = encode_request(&req, &profile());

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
