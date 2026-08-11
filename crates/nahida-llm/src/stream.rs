//! Server-Sent Events: framing, event types, and folding a stream back into a
//! single [`Response`].
//!
//! The accumulator is the part worth understanding. A streamed `tool_use` block
//! does not arrive as JSON — it arrives as a series of `partial_json` string
//! fragments that only parse once concatenated. Anything that tries to
//! `serde_json::from_str` a fragment on arrival will fail on every tool call
//! with more than one delta.

use serde::Deserialize;

use crate::types::{ApiErrorBody, ContentBlock, Response, StopDetails, StopReason, Usage};

/// Incremental SSE frame decoder.
///
/// A frame is lines terminated by a blank line. We read only `data:` lines and
/// ignore `event:`, because Anthropic repeats the event name in the JSON `type`
/// field — one source of truth instead of two that can disagree.
///
/// This buffers **bytes**, not text, on purpose: an HTTP chunk can end in the
/// middle of a multibyte character, so decoding each chunk as UTF-8 on arrival
/// corrupts any response containing non-ASCII. Frame boundaries are safe to find
/// in bytes because no UTF-8 continuation byte can be `\n` or `\r`.
#[derive(Debug, Default)]
pub struct SseDecoder {
    buf: Vec<u8>,
}

/// Offset and length of the first frame terminator, handling both `\n\n` and
/// the `\r\n\r\n` the spec also permits.
fn find_terminator(buf: &[u8]) -> Option<(usize, usize)> {
    let lf = buf.windows(2).position(|w| w == b"\n\n").map(|i| (i, 2));
    let crlf = buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| (i, 4));
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a chunk of the response body. Returns the `data` payload of every
    /// frame that is now complete; incomplete frames stay buffered.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(chunk);

        let mut out = Vec::new();
        while let Some((end, sep)) = find_terminator(&self.buf) {
            let frame = String::from_utf8_lossy(&self.buf[..end]).into_owned();
            self.buf.drain(..end + sep);

            let mut data = String::new();
            for line in frame.lines() {
                let line = line.strip_suffix('\r').unwrap_or(line);
                if let Some(rest) = line.strip_prefix("data:") {
                    if !data.is_empty() {
                        data.push('\n');
                    }
                    data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
                }
            }
            if !data.is_empty() {
                out.push(data);
            }
        }
        out
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageStartInfo {
    pub id: String,
    pub model: String,
    #[serde(default)]
    pub usage: Usage,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageDeltaInfo {
    #[serde(default)]
    pub stop_reason: Option<StopReason>,
    #[serde(default)]
    pub stop_details: Option<StopDetails>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Delta {
    TextDelta {
        text: String,
    },
    ThinkingDelta {
        thinking: String,
    },
    /// A fragment of a tool call's JSON input. Concatenate, then parse.
    InputJsonDelta {
        partial_json: String,
    },
    SignatureDelta {
        signature: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    MessageStart {
        message: MessageStartInfo,
    },
    ContentBlockStart {
        index: usize,
        content_block: ContentBlock,
    },
    ContentBlockDelta {
        index: usize,
        delta: Delta,
    },
    ContentBlockStop {
        index: usize,
    },
    MessageDelta {
        delta: MessageDeltaInfo,
        #[serde(default)]
        usage: Option<Usage>,
    },
    MessageStop,
    /// Keepalive. Long thinking turns send these; they carry nothing.
    Ping,
    Error {
        error: ApiErrorBody,
    },
    #[serde(other)]
    Unknown,
}

/// A block still being assembled.
#[derive(Debug)]
enum Partial {
    Text(String),
    Thinking {
        text: String,
        signature: Option<String>,
    },
    ToolUse {
        id: String,
        name: String,
        json: String,
    },
    /// A block type we do not model; deltas for it are discarded.
    Skip,
}

/// Folds [`StreamEvent`]s into one [`Response`].
///
/// Feed every event, in order, then call [`Accumulator::finish`].
#[derive(Debug, Default)]
pub struct Accumulator {
    id: String,
    model: String,
    blocks: Vec<Partial>,
    stop_reason: Option<StopReason>,
    stop_details: Option<StopDetails>,
    usage: Usage,
}

impl Accumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(&mut self, event: &StreamEvent) {
        match event {
            StreamEvent::MessageStart { message } => {
                self.id.clone_from(&message.id);
                self.model.clone_from(&message.model);
                // Input and cache token counts are only reported here.
                self.usage.add(message.usage);
            }
            StreamEvent::ContentBlockStart { index, content_block } => {
                let partial = match content_block {
                    ContentBlock::Text { text, .. } => Partial::Text(text.clone()),
                    ContentBlock::Thinking { thinking, signature } => {
                        Partial::Thinking { text: thinking.clone(), signature: signature.clone() }
                    }
                    ContentBlock::ToolUse { id, name, .. } => {
                        Partial::ToolUse { id: id.clone(), name: name.clone(), json: String::new() }
                    }
                    _ => Partial::Skip,
                };
                // Indices arrive in order, but do not assume it: grow to fit.
                while self.blocks.len() <= *index {
                    self.blocks.push(Partial::Skip);
                }
                self.blocks[*index] = partial;
            }
            StreamEvent::ContentBlockDelta { index, delta } => {
                let Some(slot) = self.blocks.get_mut(*index) else { return };
                match (slot, delta) {
                    (Partial::Text(buf), Delta::TextDelta { text }) => buf.push_str(text),
                    (Partial::Thinking { text, .. }, Delta::ThinkingDelta { thinking }) => {
                        text.push_str(thinking);
                    }
                    (
                        Partial::Thinking { signature, .. },
                        Delta::SignatureDelta { signature: s },
                    ) => {
                        *signature = Some(s.clone());
                    }
                    (Partial::ToolUse { json, .. }, Delta::InputJsonDelta { partial_json }) => {
                        json.push_str(partial_json);
                    }
                    _ => {}
                }
            }
            StreamEvent::MessageDelta { delta, usage } => {
                if delta.stop_reason.is_some() {
                    self.stop_reason = delta.stop_reason;
                }
                if let Some(d) = &delta.stop_details {
                    self.stop_details = Some(d.clone());
                }
                if let Some(u) = usage {
                    // Output tokens are reported here, incrementally.
                    self.usage.output_tokens = u.output_tokens;
                }
            }
            StreamEvent::ContentBlockStop { .. }
            | StreamEvent::MessageStop
            | StreamEvent::Ping
            | StreamEvent::Error { .. }
            | StreamEvent::Unknown => {}
        }
    }

    pub fn finish(self) -> Response {
        let content = self
            .blocks
            .into_iter()
            .filter_map(|p| match p {
                Partial::Text(text) => Some(ContentBlock::Text { text, cache_control: None }),
                Partial::Thinking { text, signature } => {
                    Some(ContentBlock::Thinking { thinking: text, signature })
                }
                Partial::ToolUse { id, name, json } => {
                    // A tool with no parameters streams no deltas at all, so the
                    // buffer is empty rather than "{}" — that is not an error.
                    let input = if json.trim().is_empty() {
                        serde_json::json!({})
                    } else {
                        serde_json::from_str(&json)
                            .unwrap_or_else(|_| serde_json::json!({ "__unparsed": json }))
                    };
                    Some(ContentBlock::ToolUse { id, name, input })
                }
                Partial::Skip => None,
            })
            .collect();

        Response {
            id: self.id,
            model: self.model,
            content,
            stop_reason: self.stop_reason,
            stop_details: self.stop_details,
            usage: self.usage,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_frame_split_across_chunks() {
        let mut d = SseDecoder::new();
        assert!(d.push(b"event: ping\ndata: {\"ty").is_empty());
        assert!(d.push(b"pe\":\"ping\"}").is_empty());
        let frames = d.push(b"\n\n");
        assert_eq!(frames, vec![r#"{"type":"ping"}"#]);
    }

    #[test]
    fn decodes_several_frames_in_one_chunk() {
        let mut d = SseDecoder::new();
        let frames = d.push(b"data: {\"a\":1}\n\ndata: {\"b\":2}\n\n");
        assert_eq!(frames.len(), 2);
    }

    #[test]
    fn accepts_crlf_framing() {
        let mut d = SseDecoder::new();
        let frames = d.push(b"event: ping\r\ndata: {\"type\":\"ping\"}\r\n\r\n");
        assert_eq!(frames, vec![r#"{"type":"ping"}"#]);
    }

    /// The reason the buffer holds bytes: this chunk boundary splits a
    /// three-byte character in half.
    #[test]
    fn survives_a_chunk_boundary_inside_a_multibyte_char() {
        let text = "日本語";
        let payload = format!("data: {{\"t\":\"{text}\"}}\n\n");
        let bytes = payload.as_bytes();
        let split = payload.find(text).expect("marker present") + 1;

        let mut d = SseDecoder::new();
        assert!(d.push(&bytes[..split]).is_empty());
        let frames = d.push(&bytes[split..]);
        assert_eq!(frames.len(), 1);
        assert!(frames[0].contains(text), "got {}", frames[0]);
    }

    #[test]
    fn concatenates_tool_input_fragments() {
        let events: Vec<StreamEvent> = [
            r#"{"type":"message_start","message":{"id":"m1","model":"claude-opus-5","usage":{"input_tokens":10}}}"#,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t1","name":"read","input":{}}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"path\""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":": \"a.rs\"}"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":7}}"#,
        ]
        .iter()
        .map(|s| serde_json::from_str(s).expect("event parses"))
        .collect();

        let mut acc = Accumulator::new();
        for e in &events {
            acc.apply(e);
        }
        let resp = acc.finish();

        assert_eq!(resp.stop_reason, Some(StopReason::ToolUse));
        assert_eq!(resp.usage.output_tokens, 7);
        let uses = resp.tool_uses();
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].1, "read");
        assert_eq!(uses[0].2["path"], "a.rs");
    }

    #[test]
    fn unknown_block_and_event_types_do_not_break_the_fold() {
        let events: Vec<StreamEvent> = [
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"redacted_thinking","data":"xx"}}"#,
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"hi"}}"#,
            r#"{"type":"some_future_event"}"#,
        ]
        .iter()
        .map(|s| serde_json::from_str(s).expect("event parses"))
        .collect();

        let mut acc = Accumulator::new();
        for e in &events {
            acc.apply(e);
        }
        assert_eq!(acc.finish().text(), "hi");
    }
}
