//! The loop.
//!
//! One turn is: send the transcript, read the response, and look at
//! `stop_reason`. If it is `tool_use`, run the tools, append the results, and go
//! round again. That is the whole idea — everything else in this file is
//! bookkeeping around it.
//!
//! Two invariants are easy to get wrong and expensive to debug:
//!
//! 1. **The assistant turn goes back verbatim.** The `tool_use` blocks must be
//!    in the transcript or the `tool_result` blocks have nothing to attach to.
//! 2. **All tool results go in one user message.** Splitting them across several
//!    messages trains the model to stop making parallel calls.

use std::sync::Arc;

use futures_util::StreamExt;
use nahida_llm::{
    Accumulator, Client, ContentBlock, Delta, Effort, Message, OutputConfig, Request, Response,
    StopReason, StreamEvent, SystemBlock, Thinking, Usage,
};

use crate::cancel::Cancel;
use crate::event::AgentEvent;
use crate::tool::Tool;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error(transparent)]
    Llm(#[from] nahida_llm::Error),

    /// Safety classifiers declined. There is no partial answer worth showing.
    #[error("the model declined this request{}{}",
        .category.as_ref().map(|c| format!(" ({c})")).unwrap_or_default(),
        .explanation.as_ref().map(|e| format!(": {e}")).unwrap_or_default())]
    Refused { category: Option<String>, explanation: Option<String> },

    #[error("interrupted")]
    Cancelled,

    #[error("gave up after {0} turns without finishing")]
    TurnLimit(u32),
}

/// Why the loop stopped, plus what it cost.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub stop_reason: Option<StopReason>,
    pub usage: Usage,
    pub turns: u32,
}

impl Outcome {
    /// True when the answer was cut off by `max_tokens` rather than finished.
    pub fn truncated(&self) -> bool {
        self.stop_reason == Some(StopReason::MaxTokens)
    }
}

pub struct Agent {
    client: Client,
    model: String,
    system: Vec<SystemBlock>,
    tools: Vec<Arc<dyn Tool>>,
    max_tokens: u32,
    max_turns: u32,
    effort: Option<Effort>,
    thinking: Option<Thinking>,
    compact_threshold: Option<u64>,
}

impl Agent {
    /// `max_tokens` defaults high because we always stream: the cap covers
    /// thinking *and* visible text together, and on Opus 5 thinking is on by
    /// default, so a tight cap truncates mid-answer.
    pub fn new(client: Client) -> Self {
        Self {
            client,
            model: nahida_llm::DEFAULT_MODEL.to_string(),
            system: Vec::new(),
            tools: Vec::new(),
            max_tokens: 64_000,
            max_turns: 32,
            effort: None,
            thinking: None,
            compact_threshold: None,
        }
    }

    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Set the system prompt and mark it as a prompt-cache breakpoint.
    ///
    /// Render order is tools → system → messages, so this one breakpoint covers
    /// the tool schemas as well. It only works because the text is static: an
    /// interpolated timestamp here would invalidate the cache on every request.
    #[must_use]
    pub fn system(mut self, text: impl Into<String>) -> Self {
        self.system = vec![SystemBlock::new(text).cached()];
        self
    }

    #[must_use]
    pub fn tool(mut self, tool: Arc<dyn Tool>) -> Self {
        self.tools.push(tool);
        self
    }

    #[must_use]
    pub fn tools(mut self, tools: impl IntoIterator<Item = Arc<dyn Tool>>) -> Self {
        self.tools.extend(tools);
        self
    }

    #[must_use]
    pub fn effort(mut self, effort: Option<Effort>) -> Self {
        self.effort = effort;
        self
    }

    #[must_use]
    pub fn max_turns(mut self, turns: u32) -> Self {
        self.max_turns = turns;
        self
    }

    #[must_use]
    pub fn max_tokens(mut self, tokens: u32) -> Self {
        self.max_tokens = tokens;
        self
    }

    /// Summarize and replace the transcript once the *previous* turn's prompt
    /// token count reaches `tokens`. Off by default: the right threshold
    /// depends on the model's context window, which this crate does not track
    /// per provider, so callers who want it size it themselves.
    #[must_use]
    pub fn compact_at(mut self, tokens: u64) -> Self {
        self.compact_threshold = Some(tokens);
        self
    }

    /// Ask for summarized reasoning. Without this, thinking blocks arrive empty
    /// and a long think looks like the process has hung.
    #[must_use]
    pub fn show_thinking(mut self, show: bool) -> Self {
        self.thinking = show.then(Thinking::summarized);
        self
    }

    fn request(&self, messages: &[Message]) -> Request {
        Request {
            model: self.model.clone(),
            max_tokens: self.max_tokens,
            system: self.system.clone(),
            messages: messages.to_vec(),
            tools: self.tools.iter().map(|t| t.spec()).collect(),
            output_config: self.effort.map(|e| OutputConfig { effort: Some(e) }),
            thinking: self.thinking,
            stream: true,
        }
    }

    /// Run until the model stops asking for tools.
    ///
    /// `transcript` is mutated in place, so an interrupted or failed run still
    /// leaves the caller with a usable history to resume or inspect.
    pub async fn run(
        &self,
        transcript: &mut Vec<Message>,
        cancel: &Cancel,
        sink: &mut dyn FnMut(AgentEvent),
    ) -> Result<Outcome, AgentError> {
        let mut total = Usage::default();
        // What the *last* turn's request cost, in prompt tokens — a proxy for
        // how large the transcript is right now. Zero on turn 1, so
        // compaction never fires before there is anything to compact.
        let mut last_prompt_tokens = 0u64;

        for turn in 1..=self.max_turns {
            if cancel.is_cancelled() {
                return Err(AgentError::Cancelled);
            }

            // Only checked here, between turns: the transcript is always
            // "settled" at this point (every tool_use already has its
            // tool_result appended), so replacing it can never split a
            // pending tool exchange.
            if let Some(threshold) = self.compact_threshold
                && last_prompt_tokens >= threshold
            {
                let cost = self.compact(transcript, cancel, sink).await?;
                total.add(cost);
            }

            sink(AgentEvent::TurnStart { turn });
            let response = self.one_turn(transcript, cancel, sink).await?;
            total.add(response.usage);
            last_prompt_tokens = response.usage.prompt_tokens();
            sink(AgentEvent::TurnEnd { usage: response.usage });

            match response.stop_reason {
                // Never read `content` on a refusal — it is empty or a partial.
                Some(StopReason::Refusal) => {
                    let d = response.stop_details.as_ref();
                    return Err(AgentError::Refused {
                        category: d.and_then(|d| d.category.clone()),
                        explanation: d.and_then(|d| d.explanation.clone()),
                    });
                }

                // A server-side tool loop hit its iteration cap. Replay the turn
                // as-is and the server resumes; do not add a "continue" message.
                Some(StopReason::PauseTurn) => {
                    transcript.push(Message::assistant(response.replayable()));
                }

                Some(StopReason::ToolUse) => {
                    let calls: Vec<(String, String, serde_json::Value)> = response
                        .tool_uses()
                        .into_iter()
                        .map(|(id, name, input)| (id.to_string(), name.to_string(), input.clone()))
                        .collect();

                    transcript.push(Message::assistant(response.replayable()));

                    for (_, name, input) in &calls {
                        sink(AgentEvent::ToolCall { name: name.clone(), input: input.clone() });
                    }

                    let results = self.dispatch(&calls).await;

                    for (call, block) in calls.iter().zip(&results) {
                        if let ContentBlock::ToolResult { content, is_error, .. } = block {
                            sink(AgentEvent::ToolResult {
                                name: call.1.clone(),
                                is_error: *is_error,
                                content: content.clone(),
                            });
                        }
                    }

                    // One user message carrying every result, in call order.
                    transcript.push(Message::user(results));
                }

                // EndTurn, MaxTokens, StopSequence: the turn is the answer.
                other => {
                    transcript.push(Message::assistant(response.replayable()));
                    sink(AgentEvent::Done { stop_reason: other });
                    return Ok(Outcome { stop_reason: other, usage: total, turns: turn });
                }
            }
        }

        Err(AgentError::TurnLimit(self.max_turns))
    }

    /// Stream one assistant turn, forwarding deltas to `sink` while folding the
    /// events into a complete [`Response`].
    async fn one_turn(
        &self,
        transcript: &[Message],
        cancel: &Cancel,
        sink: &mut dyn FnMut(AgentEvent),
    ) -> Result<Response, AgentError> {
        let request = self.request(transcript);
        self.stream_request(&request, cancel, sink).await
    }

    /// Summarize `transcript` and replace it with the summary, so the next
    /// turn starts from a smaller context. Returns the summarization call's
    /// own usage, so callers can fold its cost into a running total.
    ///
    /// Only ever called between turns (see [`Agent::run`]), never mid-tool-
    /// exchange. `transcript` is sent as-is, with the instruction folded into
    /// the system prompt rather than appended as a new message — the
    /// transcript can legally end in either role (`PauseTurn` leaves it on
    /// assistant), and appending a user message would create two consecutive
    /// user turns whenever it already ended in one, which the API rejects.
    /// The replacement is a single user message — not a user/assistant pair —
    /// because the caller always appends an assistant message next (the turn
    /// that follows), and the API requires alternating roles starting from
    /// user.
    async fn compact(
        &self,
        transcript: &mut Vec<Message>,
        cancel: &Cancel,
        sink: &mut dyn FnMut(AgentEvent),
    ) -> Result<Usage, AgentError> {
        sink(AgentEvent::Compacting);

        let request = Request {
            model: self.model.clone(),
            max_tokens: 2_048,
            system: vec![SystemBlock::new(include_str!("compact.md"))],
            messages: transcript.clone(),
            tools: Vec::new(),
            output_config: None,
            thinking: None,
            stream: true,
        };

        let response = self.stream_request(&request, cancel, sink).await?;
        let summary = response.text();

        *transcript = vec![Message::user(vec![ContentBlock::text(format!(
            "(earlier conversation compacted)\n\n{summary}"
        ))])];

        sink(AgentEvent::Compacted);
        Ok(response.usage)
    }

    /// Stream one request, forwarding text/thinking deltas to `sink` while
    /// folding the events into a complete [`Response`]. Shared by
    /// [`Agent::one_turn`] and [`Agent::compact`] — the only difference
    /// between a turn and a compaction call is what `Request` gets built.
    async fn stream_request(
        &self,
        request: &Request,
        cancel: &Cancel,
        sink: &mut dyn FnMut(AgentEvent),
    ) -> Result<Response, AgentError> {
        let mut events = Box::pin(self.client.stream(request).await?);
        let mut acc = Accumulator::new();

        while let Some(event) = events.next().await {
            if cancel.is_cancelled() {
                return Err(AgentError::Cancelled);
            }
            let event = event?;
            acc.apply(&event);

            if let StreamEvent::ContentBlockDelta { delta, .. } = &event {
                match delta {
                    Delta::TextDelta { text } => sink(AgentEvent::Text { delta: text.clone() }),
                    Delta::ThinkingDelta { thinking } => {
                        sink(AgentEvent::Thinking { delta: thinking.clone() });
                    }
                    _ => {}
                }
            }
        }

        Ok(acc.finish())
    }

    /// Run every requested tool concurrently and return the results in call order.
    async fn dispatch(&self, calls: &[(String, String, serde_json::Value)]) -> Vec<ContentBlock> {
        let futures = calls.iter().map(|(id, name, input)| {
            let id = id.clone();
            let tool = self.tools.iter().find(|t| t.name() == name).cloned();
            let name = name.clone();
            let input = input.clone();
            async move {
                let outcome = match tool {
                    Some(tool) => tool.call(input).await,
                    // Still return a result: a call with no answer wedges the
                    // conversation, and the model can recover from a message.
                    None => crate::tool::ToolOutcome::err(format!(
                        "unknown tool `{name}` — it is not registered on this agent"
                    )),
                };
                ContentBlock::ToolResult {
                    tool_use_id: id,
                    content: outcome.content,
                    is_error: outcome.is_error,
                }
            }
        });

        futures_util::future::join_all(futures).await
    }
}
