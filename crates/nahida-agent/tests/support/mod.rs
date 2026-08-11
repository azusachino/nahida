//! A fake Anthropic-compatible provider, for testing the loop without an API key
//! and without network access.
//!
//! It serves a scripted list of SSE responses — one per request — and records
//! every request body it received. That combination is what makes loop behaviour
//! assertable: you can drive the loop into a state (a tool-use turn, a refusal,
//! a turn limit) and then check the *exact* transcript the loop built in
//! response, which is where the invariants that matter actually live.
//!
//! Responses are written in several pieces with a pause between them, so the
//! decoder's chunk-boundary handling is exercised on every test rather than only
//! in its own unit tests.

use std::sync::{Arc, Mutex};

use nahida_llm::{Client, Dialect, Profile};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;

/// One SSE event, ready to write.
pub fn event(payload: &serde_json::Value) -> String {
    let kind = payload["type"].as_str().unwrap_or("message");
    format!("event: {kind}\ndata: {payload}\n\n")
}

/// A turn that emits text, then asks for tools, then stops with `tool_use`.
///
/// Tool inputs are deliberately split mid-key across deltas — that is the shape
/// that breaks any implementation which parses a fragment on arrival.
pub fn tool_use_turn(text: &str, calls: &[(&str, &str, serde_json::Value)]) -> String {
    let mut out = String::new();
    out.push_str(&event(&serde_json::json!({
        "type": "message_start",
        "message": {"id": "m", "model": "fake-1", "usage": {"input_tokens": 10}}
    })));

    let mut index = 0;
    if !text.is_empty() {
        out.push_str(&event(&serde_json::json!({
            "type": "content_block_start", "index": index,
            "content_block": {"type": "text", "text": ""}
        })));
        out.push_str(&event(&serde_json::json!({
            "type": "content_block_delta", "index": index,
            "delta": {"type": "text_delta", "text": text}
        })));
        out.push_str(&event(&serde_json::json!({"type": "content_block_stop", "index": index})));
        index += 1;
    }

    for (id, name, input) in calls {
        out.push_str(&event(&serde_json::json!({
            "type": "content_block_start", "index": index,
            "content_block": {"type": "tool_use", "id": id, "name": name, "input": {}}
        })));
        let json = input.to_string();
        let mid = json.len() / 2;
        for fragment in [&json[..mid], &json[mid..]] {
            out.push_str(&event(&serde_json::json!({
                "type": "content_block_delta", "index": index,
                "delta": {"type": "input_json_delta", "partial_json": fragment}
            })));
        }
        out.push_str(&event(&serde_json::json!({"type": "content_block_stop", "index": index})));
        index += 1;
    }

    out.push_str(&event(&serde_json::json!({
        "type": "message_delta",
        "delta": {"stop_reason": "tool_use"},
        "usage": {"output_tokens": 5}
    })));
    out.push_str(&event(&serde_json::json!({"type": "message_stop"})));
    out
}

/// A turn that emits text and finishes.
pub fn text_turn(text: &str) -> String {
    [
        event(&serde_json::json!({
            "type": "message_start",
            "message": {"id": "m", "model": "fake-1", "usage": {"input_tokens": 10}}
        })),
        event(&serde_json::json!({
            "type": "content_block_start", "index": 0,
            "content_block": {"type": "text", "text": ""}
        })),
        event(&serde_json::json!({
            "type": "content_block_delta", "index": 0,
            "delta": {"type": "text_delta", "text": text}
        })),
        event(&serde_json::json!({"type": "content_block_stop", "index": 0})),
        event(&serde_json::json!({
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn"},
            "usage": {"output_tokens": 5}
        })),
        event(&serde_json::json!({"type": "message_stop"})),
    ]
    .concat()
}

/// A turn the safety classifiers declined: empty content, `stop_reason: refusal`.
pub fn refusal_turn(category: &str) -> String {
    [
        event(&serde_json::json!({
            "type": "message_start",
            "message": {"id": "m", "model": "fake-1", "usage": {"input_tokens": 10}}
        })),
        event(&serde_json::json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": "refusal",
                "stop_details": {"type": "refusal", "category": category,
                                 "explanation": "declined by policy"}
            },
            "usage": {"output_tokens": 0}
        })),
        event(&serde_json::json!({"type": "message_stop"})),
    ]
    .concat()
}

/// What `FakeProvider` serves for one request.
pub enum Script {
    /// A normal 200 response streaming this SSE body.
    Sse(String),
    /// An HTTP-level error: a status code plus the API's `{"error": {...}}`
    /// envelope shape (see `nahida_llm::client::api_error`), for exercising
    /// retry and overflow-recovery against something that looks real.
    Error { status: u16, kind: &'static str, message: &'static str },
}

impl From<String> for Script {
    fn from(body: String) -> Self {
        Self::Sse(body)
    }
}

impl Script {
    pub fn error(status: u16, kind: &'static str, message: &'static str) -> Self {
        Self::Error { status, kind, message }
    }
}

/// A running fake provider.
pub struct FakeProvider {
    pub client: Client,
    requests: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl FakeProvider {
    /// Serve `scripts` in order. Once exhausted, the last script repeats — which
    /// is what lets a turn-limit test run indefinitely off one entry.
    ///
    /// `Compat` dialect, so most tests also exercise the loop against a
    /// request with `output_config`/`thinking`/`cache_control` stripped —
    /// use [`FakeProvider::start_with_dialect`] for a test that needs one of
    /// those fields to actually reach the recorded request.
    pub async fn start<T: Into<Script>>(scripts: Vec<T>) -> Self {
        Self::start_with_dialect(scripts, Dialect::Compat).await
    }

    pub async fn start_with_dialect<T: Into<Script>>(scripts: Vec<T>, dialect: Dialect) -> Self {
        let scripts: Vec<Script> = scripts.into_iter().map(Into::into).collect();
        assert!(!scripts.is_empty(), "need at least one scripted response");

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let requests = Arc::new(Mutex::new(Vec::new()));

        {
            let requests = Arc::clone(&requests);
            tokio::spawn(async move {
                let mut served = 0usize;
                while let Ok((mut socket, _)) = listener.accept().await {
                    let Some(body) = read_request(&mut socket).await else { continue };
                    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&body) {
                        requests.lock().expect("lock").push(json);
                    }

                    let script =
                        scripts.get(served).unwrap_or_else(|| scripts.last().expect("non-empty"));
                    served += 1;

                    // No content-length: `connection: close` plus EOF delimits
                    // the body, which lets us write it in pieces.
                    let (head, payload): (String, Vec<u8>) = match script {
                        Script::Sse(body) => (
                            "HTTP/1.1 200 OK\r\n\
                             content-type: text/event-stream\r\n\
                             connection: close\r\n\r\n"
                                .to_string(),
                            body.as_bytes().to_vec(),
                        ),
                        Script::Error { status, kind, message } => (
                            format!(
                                "HTTP/1.1 {status} Error\r\n\
                                 content-type: application/json\r\n\
                                 connection: close\r\n\r\n"
                            ),
                            serde_json::json!({"error": {"type": kind, "message": message}})
                                .to_string()
                                .into_bytes(),
                        ),
                    };
                    if socket.write_all(head.as_bytes()).await.is_err() {
                        continue;
                    }

                    // Split at a boundary that is *not* a frame boundary, so the
                    // decoder has to buffer a partial frame.
                    let split = payload.len() / 3;
                    for piece in [&payload[..split], &payload[split..]] {
                        if socket.write_all(piece).await.is_err() {
                            break;
                        }
                        let _ = socket.flush().await;
                        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    }
                    let _ = socket.shutdown().await;
                }
            });
        }

        let profile = Profile {
            name: "fake",
            base_url: format!("http://127.0.0.1:{port}"),
            dialect,
            default_model: "fake-1".to_string(),
            default_max_tokens: 1024,
        };

        Self { client: Client::bearer("test-token", profile).expect("client"), requests }
    }

    /// Every request the loop sent, in order.
    pub fn requests(&self) -> Vec<serde_json::Value> {
        self.requests.lock().expect("lock").clone()
    }
}

/// Read one HTTP request and return its body.
async fn read_request(socket: &mut tokio::net::TcpStream) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];

    // Headers first, so we can find the content length.
    let header_end = loop {
        let n = socket.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };

    let headers = String::from_utf8_lossy(&buf[..header_end]).to_lowercase();
    let len: usize = headers
        .lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);

    while buf.len() < header_end + len {
        let n = socket.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }

    Some(buf[header_end..].to_vec())
}

/// The roles and block types of each message, for asserting transcript shape.
pub fn transcript_shape(request: &serde_json::Value) -> Vec<(String, Vec<String>)> {
    request["messages"]
        .as_array()
        .expect("messages array")
        .iter()
        .map(|m| {
            let role = m["role"].as_str().unwrap_or("?").to_string();
            let blocks = m["content"]
                .as_array()
                .map(|bs| {
                    bs.iter().map(|b| b["type"].as_str().unwrap_or("?").to_string()).collect()
                })
                .unwrap_or_default();
            (role, blocks)
        })
        .collect()
}
