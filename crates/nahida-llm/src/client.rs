//! HTTP transport for the Anthropic Messages wire format, credential
//! resolution for first-party Anthropic specifically, and dialect stripping.
//!
//! Which *provider* to use at all — first-party Anthropic, a Bearer-auth
//! gateway, a different wire format entirely — is [`crate::provider`]'s job.
//! This module only knows how to speak Anthropic Messages once a provider has
//! already been chosen.

use std::collections::VecDeque;

use futures_util::{Stream, StreamExt};
use serde::Deserialize;

use crate::stream::{SseDecoder, StreamEvent};
use crate::types::{API_VERSION, ApiErrorBody, ContentBlock, DEFAULT_MODEL, Request, Response};

pub(crate) const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "no credentials found. Set one of:\n\
         \x20 ANTHROPIC_API_KEY       — first-party Anthropic\n\
         \x20 ZAI_API_KEY             — Z.ai coding plan (GLM), global\n\
         \x20 ZAI_CODING_CN_API_KEY   — Z.ai coding plan (GLM), China region\n\
         \x20 ANTHROPIC_AUTH_TOKEN + ANTHROPIC_BASE_URL — any compatible gateway"
    )]
    NoCredentials,

    #[error("transport: {0}")]
    Transport(#[from] reqwest::Error),

    /// A non-2xx response, with the API's own error envelope decoded when present.
    #[error("api {status}: {kind}: {message}")]
    Api { status: u16, kind: String, message: String },

    #[error("could not decode {what}: {source}")]
    Decode {
        what: &'static str,
        #[source]
        source: serde_json::Error,
    },

    /// An `error` event mid-stream. The turn is over; anything already yielded stands.
    #[error("stream error: {kind}: {message}")]
    Stream { kind: String, message: String },
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Whether retrying the same request, unchanged, has a reasonable chance
    /// of succeeding. Transport failures and provider overload/rate-limit
    /// signals are transient; a decode failure or a rejected request are
    /// not — retrying either just reproduces the same failure.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(_) => true,
            Self::Api { status, .. } => matches!(status, 429 | 500 | 502 | 503 | 504),
            Self::Stream { kind, .. } => {
                matches!(kind.as_str(), "overloaded_error" | "rate_limit_error" | "api_error")
            }
            Self::Decode { .. } | Self::NoCredentials => false,
        }
    }

    /// Whether this looks like the request exceeded the model's context
    /// window, as opposed to any other rejection. Anthropic reports this as
    /// a 400 with "prompt is too long" in the message, or a 413
    /// (`request_too_large`); this only covers the shape observed from
    /// first-party Anthropic, not every compatible gateway's wording.
    pub fn is_context_overflow(&self) -> bool {
        match self {
            Self::Api { status, message, .. } => {
                *status == 413 || message.contains("prompt is too long")
            }
            Self::Stream { message, .. } => message.contains("prompt is too long"),
            _ => false,
        }
    }
}

/// How much of the Messages API the endpoint actually implements.
///
/// The wire format is the same, the *feature set* is not. An Anthropic-compatible
/// gateway generally understands messages, tools, and streaming, and does not
/// understand `output_config.effort`, adaptive `thinking`, or `cache_control` —
/// all of which post-date the shape everyone cloned. Whether an unknown field is
/// ignored or rejected depends on the gateway, so we strip rather than gamble.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// First-party. Everything in [`Request`] is understood.
    Anthropic,
    /// Core Messages API only.
    Compat,
}

impl Dialect {
    /// Guess from the host. A gateway is anything that is not Anthropic's own.
    pub(crate) fn infer(base_url: &str) -> Self {
        let authority = base_url.split_once("://").map_or(base_url, |(_, rest)| rest);
        let host = authority.split('/').next().unwrap_or("");
        // Strip any :port so `localhost:8080` compares as a host.
        let host = host.split(':').next().unwrap_or(host);
        if host.ends_with("anthropic.com") { Self::Anthropic } else { Self::Compat }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "anthropic" => Some(Self::Anthropic),
            "compat" => Some(Self::Compat),
            _ => None,
        }
    }

    /// Remove anything this dialect will not understand.
    fn adapt(self, req: &mut Request) {
        if self == Self::Anthropic {
            return;
        }
        req.output_config = None;
        req.thinking = None;
        for block in &mut req.system {
            block.cache_control = None;
        }
        for tool in &mut req.tools {
            tool.cache_control = None;
        }
        for message in &mut req.messages {
            for block in &mut message.content {
                if let ContentBlock::Text { cache_control, .. }
                | ContentBlock::ToolResult { cache_control, .. } = block
                {
                    *cache_control = None;
                }
            }
        }
    }
}

/// Endpoint defaults, so the CLI does not have to know which provider is in play.
#[derive(Debug, Clone)]
pub struct Profile {
    pub name: &'static str,
    pub base_url: String,
    /// Only meaningful for a provider speaking the Anthropic Messages wire
    /// format — a provider on a different wire format (see
    /// [`crate::provider::Provider`]) has no dialect of its own and fills
    /// this with [`Dialect::Compat`] as an informational placeholder.
    pub dialect: Dialect,
    pub default_model: String,
    /// Output ceilings vary a lot between providers; 64k is safe on Opus 5 and
    /// far too high for some GLM models.
    pub default_max_tokens: u32,
}

/// How we authenticate. The two schemes use *different headers* — an OAuth or
/// gateway token sent as `x-api-key` is a 401, which is a confusing way to learn
/// this.
///
/// `pub(crate)` rather than private: [`crate::provider::resolve`] constructs
/// this directly for the first-party-Anthropic case, which needs both variants
/// (and the OAuth flag) in a way [`Client::bearer`] alone can't express.
#[derive(Debug, Clone)]
pub(crate) enum Auth {
    /// `sk-ant-…` on `x-api-key`.
    ApiKey(String),
    /// A bearer token on `Authorization`. First-party OAuth additionally needs
    /// the `oauth-2025-04-20` beta header; a gateway must not get it.
    Bearer { token: String, oauth: bool },
}

#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    auth: Auth,
    profile: Profile,
}

impl Client {
    /// Shared constructor. `pub(crate)` so [`crate::provider::resolve`] can
    /// build the first-party-Anthropic case directly (it needs both `Auth`
    /// variants and the OAuth flag, which [`Client::bearer`] alone can't
    /// express) without duplicating the HTTP-client setup.
    pub(crate) fn new(auth: Auth, profile: Profile) -> Result<Self> {
        Ok(Self {
            // Ten minutes matches the SDKs' default. A long thinking turn at
            // high effort can genuinely run for minutes.
            http: reqwest::Client::builder().timeout(std::time::Duration::from_mins(10)).build()?,
            auth,
            profile,
        })
    }

    /// Build a client against an explicit endpoint with bearer auth,
    /// bypassing environment resolution.
    ///
    /// This exists so tests can point the loop at a fake provider without
    /// touching process-global environment state, which can't be set
    /// per-test without making the suite serial.
    pub fn bearer(token: impl Into<String>, profile: Profile) -> Result<Self> {
        Self::new(Auth::Bearer { token: token.into(), oauth: false }, profile)
    }

    /// Build a client against first-party Anthropic with an API key,
    /// bypassing environment resolution — for embedding this crate directly.
    /// [`crate::provider::resolve`] is what `nahida-cli` actually uses.
    pub fn anthropic(api_key: impl Into<String>) -> Result<Self> {
        Self::new(Auth::ApiKey(api_key.into()), anthropic_profile())
    }

    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    fn post(&self, req: &Request) -> reqwest::RequestBuilder {
        let base = self.profile.base_url.trim_end_matches('/');
        let b = self
            .http
            .post(format!("{base}/v1/messages"))
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json");

        let b = match &self.auth {
            Auth::ApiKey(k) => b.header("x-api-key", k),
            Auth::Bearer { token, oauth } => {
                let b = b.header("authorization", format!("Bearer {token}"));
                if *oauth { b.header("anthropic-beta", "oauth-2025-04-20") } else { b }
            }
        };

        b.json(req)
    }

    /// One non-streaming turn. Convenient for tests and short prompts; for
    /// anything with a large `max_tokens`, stream instead so the connection does
    /// not idle out.
    pub async fn send(&self, req: &Request) -> Result<Response> {
        let mut req = req.clone();
        req.stream = false;
        self.profile.dialect.adapt(&mut req);

        let resp = self.post(&req).send().await?;
        let status = resp.status();
        let body = resp.text().await?;

        if !status.is_success() {
            return Err(api_error(status.as_u16(), &body));
        }
        serde_json::from_str(&body).map_err(|source| Error::Decode { what: "response", source })
    }

    /// One streaming turn, as a stream of [`StreamEvent`].
    ///
    /// Fold it with [`crate::stream::Accumulator`] to get a [`Response`]; read
    /// the events directly to render text as it arrives. Both at once is the
    /// normal case.
    pub async fn stream(
        &self,
        req: &Request,
    ) -> Result<impl Stream<Item = Result<StreamEvent>> + Send + use<>> {
        let mut req = req.clone();
        req.stream = true;
        self.profile.dialect.adapt(&mut req);

        let resp = self.post(&req).send().await?;
        let status = resp.status();

        if !status.is_success() {
            // Errors are a normal JSON body even on a streaming request.
            let body = resp.text().await?;
            return Err(api_error(status.as_u16(), &body));
        }

        let state = StreamState {
            bytes: resp.bytes_stream(),
            decoder: SseDecoder::new(),
            pending: VecDeque::new(),
            body_done: false,
        };

        Ok(futures_util::stream::unfold(state, |mut st| async move {
            loop {
                if let Some(data) = st.pending.pop_front() {
                    let item = match serde_json::from_str::<StreamEvent>(&data) {
                        // An `error` event ends the turn. Surface it as an error
                        // rather than an event, so callers cannot ignore it.
                        Ok(StreamEvent::Error { error }) => {
                            Err(Error::Stream { kind: error.r#type, message: error.message })
                        }
                        Ok(event) => Ok(event),
                        Err(source) => Err(Error::Decode { what: "stream event", source }),
                    };
                    return Some((item, st));
                }

                if st.body_done {
                    return None;
                }

                match st.bytes.next().await {
                    Some(Ok(chunk)) => st.pending.extend(st.decoder.push(&chunk)),
                    Some(Err(e)) => {
                        st.body_done = true;
                        return Some((Err(Error::Transport(e)), st));
                    }
                    None => st.body_done = true,
                }
            }
        }))
    }
}

/// Carried across polls of the event stream.
struct StreamState<S> {
    bytes: S,
    decoder: SseDecoder,
    /// Frames decoded but not yet yielded — one chunk can hold several.
    pending: VecDeque<String>,
    body_done: bool,
}

#[async_trait::async_trait]
impl crate::provider::Provider for Client {
    fn profile(&self) -> &Profile {
        Client::profile(self)
    }

    /// Delegates to the inherent [`Client::stream`], boxing the result —
    /// `impl Trait` return position isn't expressible in a trait method
    /// without it. The loop only ever reaches this through the trait object;
    /// anything holding a concrete `Client` can still call the inherent
    /// method directly and get the unboxed stream.
    async fn stream(&self, req: &Request) -> Result<crate::provider::EventStream> {
        let events = Client::stream(self, req).await?;
        Ok(Box::pin(events))
    }
}

pub(crate) fn anthropic_profile() -> Profile {
    Profile {
        name: "anthropic",
        base_url: ANTHROPIC_BASE_URL.to_string(),
        dialect: Dialect::Anthropic,
        default_model: DEFAULT_MODEL.to_string(),
        default_max_tokens: 64_000,
    }
}

fn api_error(status: u16, body: &str) -> Error {
    #[derive(Deserialize)]
    struct Envelope {
        error: ApiErrorBody,
    }
    match serde_json::from_str::<Envelope>(body) {
        Ok(e) => Error::Api { status, kind: e.error.r#type, message: e.error.message },
        Err(_) => Error::Api {
            status,
            kind: "unknown".to_string(),
            message: body.chars().take(500).collect(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CacheControl, Effort, Message, OutputConfig, SystemBlock, Thinking};

    fn request() -> Request {
        let mut last_block = ContentBlock::text("hi");
        last_block.mark_cached();
        Request {
            model: "m".to_string(),
            max_tokens: 100,
            system: vec![SystemBlock::new("hi").cached()],
            messages: vec![Message::user(vec![last_block])],
            tools: vec![],
            output_config: Some(OutputConfig { effort: Some(Effort::High) }),
            thinking: Some(Thinking::summarized()),
            stream: true,
        }
    }

    fn message_cache_control(req: &Request) -> Option<&CacheControl> {
        match &req.messages[0].content[0] {
            ContentBlock::Text { cache_control, .. } => cache_control.as_ref(),
            _ => panic!("expected a Text block"),
        }
    }

    #[test]
    fn compat_strips_anthropic_only_fields() {
        let mut req = request();
        Dialect::Compat.adapt(&mut req);
        assert!(req.output_config.is_none());
        assert!(req.thinking.is_none());
        assert!(req.system[0].cache_control.is_none());
        assert!(message_cache_control(&req).is_none());
    }

    #[test]
    fn anthropic_keeps_them() {
        let mut req = request();
        Dialect::Anthropic.adapt(&mut req);
        assert!(req.output_config.is_some());
        assert!(req.thinking.is_some());
        assert!(req.system[0].cache_control.is_some());
        assert!(message_cache_control(&req).is_some());
    }

    #[test]
    fn dialect_is_inferred_from_the_host() {
        assert_eq!(Dialect::infer("https://api.anthropic.com"), Dialect::Anthropic);
        assert_eq!(Dialect::infer("https://api.z.ai/api/anthropic"), Dialect::Compat);
        assert_eq!(Dialect::infer("http://localhost:8080/v1"), Dialect::Compat);
    }

    #[test]
    fn server_overload_and_rate_limits_are_retryable() {
        for status in [429, 500, 502, 503, 504] {
            let err = Error::Api { status, kind: "x".to_string(), message: "x".to_string() };
            assert!(err.is_retryable(), "{status} should be retryable");
        }
    }

    #[test]
    fn rejected_requests_are_not_retryable() {
        for status in [400, 401, 403, 404, 413] {
            let err = Error::Api { status, kind: "x".to_string(), message: "x".to_string() };
            assert!(!err.is_retryable(), "{status} should not be retryable");
        }
    }

    #[test]
    fn decode_failures_and_missing_credentials_are_not_retryable() {
        assert!(!Error::NoCredentials.is_retryable());
        let source = serde_json::from_str::<Response>("not json").unwrap_err();
        assert!(!(Error::Decode { what: "x", source }).is_retryable());
    }

    #[test]
    fn a_413_or_too_long_message_is_context_overflow() {
        let by_status = Error::Api { status: 413, kind: "x".to_string(), message: "x".to_string() };
        assert!(by_status.is_context_overflow());

        let by_message = Error::Api {
            status: 400,
            kind: "invalid_request_error".to_string(),
            message: "prompt is too long: 213462 tokens > 200000 maximum".to_string(),
        };
        assert!(by_message.is_context_overflow());

        let unrelated_400 = Error::Api {
            status: 400,
            kind: "invalid_request_error".to_string(),
            message: "messages: roles must alternate".to_string(),
        };
        assert!(!unrelated_400.is_context_overflow());
    }
}
