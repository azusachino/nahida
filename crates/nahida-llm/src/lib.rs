//! `nahida-llm` — the provider layer.
//!
//! Everything that knows the Anthropic Messages API lives here, and nothing
//! here knows what an agent is. The split matters: the agent loop should be
//! testable against a fake provider, and adding a second provider should not
//! reach into the loop.
//!
//! Rust has no official Anthropic SDK, so this is raw HTTP. For a project whose
//! point is learning, that is the feature — the wire format is visible instead
//! of behind a generated client.
//!
//! Talking to a specific, known provider (as below) means using [`Client`]
//! directly. Letting the environment choose which provider — what
//! `nahida-cli` actually does — is [`provider::resolve`]'s job instead; see
//! [`provider`] for the abstraction that lets `nahida-agent` not care which
//! one it got.
//!
//! ```no_run
//! # async fn demo() -> Result<(), nahida_llm::Error> {
//! use nahida_llm::{Client, Message, ContentBlock, Request, SystemBlock, DEFAULT_MODEL};
//!
//! let client = Client::anthropic(std::env::var("ANTHROPIC_API_KEY").unwrap())?;
//! let response = client
//!     .send(&Request {
//!         model: DEFAULT_MODEL.to_string(),
//!         max_tokens: 1024,
//!         system: vec![SystemBlock::new("You are terse.")],
//!         messages: vec![Message::user(vec![ContentBlock::text("Say OK.")])],
//!         tools: vec![],
//!         output_config: None,
//!         thinking: None,
//!         stream: false,
//!     })
//!     .await?;
//! println!("{}", response.text());
//! # Ok(())
//! # }
//! ```

pub mod client;
pub mod configured;
pub mod openai;
pub mod provider;
pub mod stream;
pub mod types;

pub use client::{Client, Dialect, Error, Profile, Result};
pub use openai::OpenAiCompletionsProvider;
pub use provider::{EventStream, Provider, resolve};
pub use stream::{Accumulator, Delta, SseDecoder, StreamEvent};
pub use types::{
    API_VERSION, CacheControl, ContentBlock, DEFAULT_MODEL, Effort, Message, OutputConfig, Request,
    Response, Role, StopReason, SystemBlock, Thinking, ThinkingDisplay, ToolSpec, Usage,
};
