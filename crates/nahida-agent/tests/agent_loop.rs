//! Regression tests for the loop's invariants.
//!
//! Each of these encodes a rule from `AGENTS.md` that is easy to break in a
//! refactor and expensive to notice at runtime: a dropped tool result wedges the
//! conversation, tool results split across messages quietly kill parallel tool
//! calls, and reading `content` on a refusal shows the user nothing.

mod support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use nahida_agent::{Agent, AgentError, Cancel, Confirm, Tool, ToolOutcome};
use nahida_llm::{ContentBlock, Message, StopReason};
use support::{FakeProvider, Script, refusal_turn, text_turn, tool_use_turn, transcript_shape};

/// Records what it was called with and returns a fixed answer.
struct Spy {
    name: &'static str,
    calls: Arc<Mutex<Vec<serde_json::Value>>>,
    count: Arc<AtomicUsize>,
    gated: bool,
}

impl Spy {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            calls: Arc::new(Mutex::new(Vec::new())),
            count: Arc::new(AtomicUsize::new(0)),
            gated: false,
        }
    }

    /// A `Spy` that reports `requires_confirmation() == true`.
    fn gated(name: &'static str) -> Self {
        Self { gated: true, ..Self::new(name) }
    }
}

#[async_trait]
impl Tool for Spy {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "a test tool"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}, "additionalProperties": true})
    }

    fn requires_confirmation(&self, _input: &serde_json::Value) -> bool {
        self.gated
    }

    async fn call(&self, input: serde_json::Value) -> ToolOutcome {
        self.calls.lock().expect("lock").push(input);
        self.count.fetch_add(1, Ordering::Relaxed);
        ToolOutcome::ok(format!("{} ran", self.name))
    }
}

/// Approves or denies every call the same way, regardless of tool or input.
struct FixedConfirm(bool);

#[async_trait]
impl Confirm for FixedConfirm {
    async fn ask(&self, _tool: &str, _input: &serde_json::Value) -> bool {
        self.0
    }
}

fn agent(provider: &FakeProvider, tools: Vec<Arc<dyn Tool>>) -> Agent {
    Agent::new(provider.client.clone())
        .model("fake-1")
        .system("you are a test fixture")
        .tools(tools)
        .max_tokens(1024)
}

#[tokio::test]
async fn runs_a_tool_then_finishes() {
    let provider = FakeProvider::start(vec![
        tool_use_turn("Looking.", &[("t1", "spy", serde_json::json!({"q": "value"}))]),
        text_turn("Done."),
    ])
    .await;

    let spy = Arc::new(Spy::new("spy"));
    let calls = Arc::clone(&spy.calls);
    let agent = agent(&provider, vec![spy]);

    let mut transcript = vec![Message::user(vec![ContentBlock::text("go")])];
    let outcome =
        agent.run(&mut transcript, &Cancel::new(), &mut |_| {}).await.expect("loop completes");

    assert_eq!(outcome.stop_reason, Some(StopReason::EndTurn));
    assert_eq!(outcome.turns, 2);

    // The fragmented tool input reassembled into real JSON.
    assert_eq!(calls.lock().expect("lock").as_slice(), [serde_json::json!({"q": "value"})]);

    // Usage accumulates across turns rather than reporting only the last.
    assert_eq!(outcome.usage.input_tokens, 20);
    assert_eq!(outcome.usage.output_tokens, 10);
}

#[tokio::test]
async fn a_denied_call_returns_an_error_result_without_running() {
    let provider = FakeProvider::start(vec![
        tool_use_turn("", &[("t1", "spy", serde_json::json!({}))]),
        text_turn("ok"),
    ])
    .await;

    let spy = Arc::new(Spy::gated("spy"));
    let count = Arc::clone(&spy.count);
    let agent = agent(&provider, vec![spy]).confirm(Arc::new(FixedConfirm(false)));

    let mut transcript = vec![Message::user(vec![ContentBlock::text("go")])];
    agent.run(&mut transcript, &Cancel::new(), &mut |_| {}).await.expect("loop completes");

    assert_eq!(count.load(Ordering::Relaxed), 0, "a denied call must never actually run");

    // Still a result, not a dropped one — a wedged conversation is worse
    // than an error the model can see and react to.
    let requests = provider.requests();
    let result = &requests[1]["messages"][2]["content"][0];
    assert_eq!(result["tool_use_id"], "t1");
    assert_eq!(result["is_error"], true);
    assert!(result["content"].as_str().expect("string").contains("not approved"), "got {result:?}");
}

#[tokio::test]
async fn an_approved_gated_call_runs_normally() {
    let provider = FakeProvider::start(vec![
        tool_use_turn("", &[("t1", "spy", serde_json::json!({}))]),
        text_turn("ok"),
    ])
    .await;

    let spy = Arc::new(Spy::gated("spy"));
    let count = Arc::clone(&spy.count);
    let agent = agent(&provider, vec![spy]).confirm(Arc::new(FixedConfirm(true)));

    let mut transcript = vec![Message::user(vec![ContentBlock::text("go")])];
    agent.run(&mut transcript, &Cancel::new(), &mut |_| {}).await.expect("loop completes");

    assert_eq!(count.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn a_gated_call_runs_unconditionally_with_no_confirm_handler_registered() {
    let provider = FakeProvider::start(vec![
        tool_use_turn("", &[("t1", "spy", serde_json::json!({}))]),
        text_turn("ok"),
    ])
    .await;

    let spy = Arc::new(Spy::gated("spy"));
    let count = Arc::clone(&spy.count);
    // No `.confirm(...)` — gating is opt-in, so a flagged call runs exactly
    // as it did before gating existed.
    let agent = agent(&provider, vec![spy]);

    let mut transcript = vec![Message::user(vec![ContentBlock::text("go")])];
    agent.run(&mut transcript, &Cancel::new(), &mut |_| {}).await.expect("loop completes");

    assert_eq!(count.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn compacts_the_transcript_once_the_threshold_is_crossed() {
    let provider = FakeProvider::start(vec![
        // Turn 1: a tool call, reporting input_tokens: 10 (the fixed value
        // every helper in `support` uses).
        tool_use_turn("Looking.", &[("t1", "spy", serde_json::json!({}))]),
        // The compaction call's response — what the model hands back as the
        // summary.
        text_turn("read main.rs, ran the tests, both passed"),
        // Turn 2, sent against the now-compacted transcript.
        text_turn("Done."),
    ])
    .await;

    let spy = Arc::new(Spy::new("spy"));
    let calls = Arc::clone(&spy.calls);
    // Turn 1 reports input_tokens: 10, so a threshold of 10 fires compaction
    // before turn 2 is sent.
    let agent = agent(&provider, vec![spy]).compact_at(10);

    let mut transcript = vec![Message::user(vec![ContentBlock::text("go")])];
    let outcome =
        agent.run(&mut transcript, &Cancel::new(), &mut |_| {}).await.expect("loop completes");

    assert_eq!(outcome.stop_reason, Some(StopReason::EndTurn));
    assert_eq!(outcome.turns, 2, "the compaction call is not itself a turn");
    // The tool still ran once, on turn 1, before compaction touched anything.
    assert_eq!(calls.lock().expect("lock").len(), 1);

    let requests = provider.requests();
    assert_eq!(requests.len(), 3, "turn 1, the compaction call, then turn 2");

    // The compaction call must not look like a normal coding turn: no tools,
    // and a different system prompt than the one the agent was built with.
    assert!(requests[1]["tools"].as_array().is_none(), "compaction call must carry no tools");
    assert_ne!(requests[1]["system"][0]["text"], "you are a test fixture");

    // Turn 2 is built from the *compacted* transcript — one user message,
    // not the original "go" + tool_use + tool_result history.
    assert_eq!(
        transcript_shape(&requests[2]),
        vec![("user".to_string(), vec!["text".to_string()])],
    );

    // The compaction call's own cost is folded into the total, not dropped:
    // turn 1 + compaction + turn 2, 10 input / 5 output tokens each.
    assert_eq!(outcome.usage.input_tokens, 30);
    assert_eq!(outcome.usage.output_tokens, 15);
}

#[tokio::test]
async fn replays_the_assistant_turn_and_batches_results_into_one_user_message() {
    let provider = FakeProvider::start(vec![
        tool_use_turn(
            "Working.",
            &[
                ("t1", "alpha", serde_json::json!({"n": 1})),
                ("t2", "beta", serde_json::json!({"n": 2})),
            ],
        ),
        text_turn("Both done."),
    ])
    .await;

    let alpha = Arc::new(Spy::new("alpha"));
    let beta = Arc::new(Spy::new("beta"));
    let agent = agent(&provider, vec![alpha, beta]);

    let mut transcript = vec![Message::user(vec![ContentBlock::text("go")])];
    agent.run(&mut transcript, &Cancel::new(), &mut |_| {}).await.expect("loop completes");

    let requests = provider.requests();
    assert_eq!(requests.len(), 2, "expected a second request after the tool calls");

    // The invariant: the assistant turn comes back verbatim (so the tool_use
    // blocks exist to attach to), and *both* results ride in a single user
    // message. Splitting them teaches the model to stop calling tools in
    // parallel, which no test of the happy path would catch.
    assert_eq!(
        transcript_shape(&requests[1]),
        vec![
            ("user".to_string(), vec!["text".to_string()]),
            (
                "assistant".to_string(),
                vec!["text".to_string(), "tool_use".to_string(), "tool_use".to_string()]
            ),
            ("user".to_string(), vec!["tool_result".to_string(), "tool_result".to_string()]),
        ]
    );

    // Results must be in call order, or they attach to the wrong call.
    let results = &requests[1]["messages"][2]["content"];
    assert_eq!(results[0]["tool_use_id"], "t1");
    assert_eq!(results[1]["tool_use_id"], "t2");
    assert_eq!(results[0]["content"], "alpha ran");
    assert_eq!(results[1]["content"], "beta ran");
}

#[tokio::test]
async fn an_unknown_tool_still_gets_a_result() {
    let provider = FakeProvider::start(vec![
        tool_use_turn("", &[("t1", "does-not-exist", serde_json::json!({}))]),
        text_turn("Recovered."),
    ])
    .await;

    // No tools registered at all.
    let agent = agent(&provider, vec![]);
    let mut transcript = vec![Message::user(vec![ContentBlock::text("go")])];
    agent.run(&mut transcript, &Cancel::new(), &mut |_| {}).await.expect("loop completes");

    let requests = provider.requests();
    let result = &requests[1]["messages"][2]["content"][0];

    // A call with no answer leaves the conversation permanently stuck, so an
    // unknown tool has to come back as an error result, not as nothing.
    assert_eq!(result["tool_use_id"], "t1");
    assert_eq!(result["is_error"], true);
    assert!(result["content"].as_str().expect("string").contains("unknown tool"), "got {result:?}");
}

#[tokio::test]
async fn a_refusal_is_an_error_not_an_empty_answer() {
    let provider = FakeProvider::start(vec![refusal_turn("cyber")]).await;
    let agent = agent(&provider, vec![]);

    let mut transcript = vec![Message::user(vec![ContentBlock::text("go")])];
    let err = agent
        .run(&mut transcript, &Cancel::new(), &mut |_| {})
        .await
        .expect_err("a refusal must not look like success");

    match err {
        AgentError::Refused { category, explanation } => {
            assert_eq!(category.as_deref(), Some("cyber"));
            assert_eq!(explanation.as_deref(), Some("declined by policy"));
        }
        other => panic!("expected Refused, got {other:?}"),
    }
}

#[tokio::test]
async fn the_turn_limit_is_enforced() {
    // The fake repeats its last script, so this provider never stops asking for
    // tools — without a bound the loop would run forever.
    let provider =
        FakeProvider::start(vec![tool_use_turn("", &[("t1", "spy", serde_json::json!({}))])]).await;

    let spy = Arc::new(Spy::new("spy"));
    let count = Arc::clone(&spy.count);
    let agent = agent(&provider, vec![spy]).max_turns(3);

    let mut transcript = vec![Message::user(vec![ContentBlock::text("go")])];
    let err =
        agent.run(&mut transcript, &Cancel::new(), &mut |_| {}).await.expect_err("must give up");

    assert!(matches!(err, AgentError::TurnLimit(3)), "got {err:?}");
    assert_eq!(count.load(Ordering::Relaxed), 3, "one tool run per turn");
}

#[tokio::test]
async fn cancelling_stops_the_loop_and_keeps_the_transcript() {
    let provider = FakeProvider::start(vec![
        tool_use_turn("", &[("t1", "spy", serde_json::json!({}))]),
        text_turn("never reached"),
    ])
    .await;

    let agent = agent(&provider, vec![Arc::new(Spy::new("spy"))]);
    let cancel = Cancel::new();
    cancel.cancel();

    let mut transcript = vec![Message::user(vec![ContentBlock::text("go")])];
    let err = agent.run(&mut transcript, &cancel, &mut |_| {}).await.expect_err("must stop");

    assert!(matches!(err, AgentError::Cancelled), "got {err:?}");
    // The caller keeps what it had, so the session is still resumable.
    assert_eq!(transcript.len(), 1);
}

#[tokio::test]
async fn the_system_prompt_and_tool_schemas_are_sent() {
    let provider = FakeProvider::start(vec![text_turn("hi")]).await;
    let agent = agent(&provider, vec![Arc::new(Spy::new("spy"))]);

    let mut transcript = vec![Message::user(vec![ContentBlock::text("go")])];
    agent.run(&mut transcript, &Cancel::new(), &mut |_| {}).await.expect("completes");

    let request = &provider.requests()[0];
    assert_eq!(request["system"][0]["text"], "you are a test fixture");
    assert_eq!(request["tools"][0]["name"], "spy");
    assert_eq!(request["stream"], true);

    // This provider is Compat, so the Anthropic-only fields must be absent —
    // sending them to a gateway is how you get an unexplained 400.
    assert!(request.get("output_config").is_none(), "output_config leaked");
    assert!(request.get("thinking").is_none(), "thinking leaked");
    assert!(request["system"][0].get("cache_control").is_none(), "cache_control leaked");

    // These are 400s on current Anthropic models and must never be constructible.
    for banned in ["temperature", "top_p", "top_k"] {
        assert!(request.get(banned).is_none(), "{banned} must never be sent");
    }
}

#[tokio::test]
async fn retries_a_transient_server_error_then_succeeds() {
    let provider = FakeProvider::start(vec![
        Script::error(503, "overloaded_error", "overloaded"),
        text_turn("recovered").into(),
    ])
    .await;

    let a = agent(&provider, vec![]).max_retries(1).retry_base_delay_ms(1);

    let mut transcript = vec![Message::user(vec![ContentBlock::text("go")])];
    let outcome = a.run(&mut transcript, &Cancel::new(), &mut |_| {}).await.expect("recovers");

    assert_eq!(outcome.stop_reason, Some(StopReason::EndTurn));
    assert_eq!(provider.requests().len(), 2, "the failed attempt plus the retry");
}

#[tokio::test]
async fn gives_up_after_max_retries_on_a_persistent_transient_error() {
    // FakeProvider repeats its last script forever, so this never recovers.
    let provider = FakeProvider::start(vec![Script::error(500, "api_error", "down")]).await;

    let a = agent(&provider, vec![]).max_retries(2).retry_base_delay_ms(1);

    let mut transcript = vec![Message::user(vec![ContentBlock::text("go")])];
    let err = a.run(&mut transcript, &Cancel::new(), &mut |_| {}).await.expect_err("must give up");

    assert!(matches!(err, AgentError::Llm(_)), "got {err:?}");
    assert_eq!(provider.requests().len(), 3, "the initial attempt plus 2 retries, then stop");
}

#[tokio::test]
async fn a_non_retryable_error_fails_immediately() {
    let provider =
        FakeProvider::start(vec![Script::error(401, "authentication_error", "bad key")]).await;

    let a = agent(&provider, vec![]).max_retries(3).retry_base_delay_ms(1);

    let mut transcript = vec![Message::user(vec![ContentBlock::text("go")])];
    a.run(&mut transcript, &Cancel::new(), &mut |_| {}).await.expect_err("must fail");

    assert_eq!(provider.requests().len(), 1, "an auth failure must never be retried");
}

#[tokio::test]
async fn recovers_from_a_real_overflow_by_compacting_and_retrying_once() {
    let provider = FakeProvider::start(vec![
        Script::error(400, "invalid_request_error", "prompt is too long: 999999 > 200000"),
        text_turn("summary of everything so far").into(), // the compaction call
        text_turn("Done.").into(),                        // the retried turn
    ])
    .await;

    // recover_from_overflow defaults true; no compact_at needed, a real
    // overflow is its own signal.
    let a = agent(&provider, vec![]);

    let mut transcript = vec![Message::user(vec![ContentBlock::text("go")])];
    let outcome = a.run(&mut transcript, &Cancel::new(), &mut |_| {}).await.expect("recovers");

    assert_eq!(outcome.stop_reason, Some(StopReason::EndTurn));
    let requests = provider.requests();
    assert_eq!(requests.len(), 3, "the overflowing turn, the compaction call, the retried turn");
    assert_eq!(
        transcript_shape(&requests[2]),
        vec![("user".to_string(), vec!["text".to_string()])],
        "the retried turn goes out against the compacted transcript"
    );
}

#[tokio::test]
async fn overflow_recovery_gets_exactly_one_attempt() {
    let provider = FakeProvider::start(vec![
        Script::error(400, "invalid_request_error", "prompt is too long: 999999 > 200000"),
        text_turn("summary").into(),
        Script::error(400, "invalid_request_error", "prompt is too long: 999999 > 200000"),
    ])
    .await;

    let a = agent(&provider, vec![]);

    let mut transcript = vec![Message::user(vec![ContentBlock::text("go")])];
    let err = a.run(&mut transcript, &Cancel::new(), &mut |_| {}).await.expect_err("must fail");

    assert!(matches!(err, AgentError::Llm(_)), "got {err:?}");
    assert_eq!(provider.requests().len(), 3, "no second recovery attempt");
}

#[tokio::test]
async fn overflow_recovery_can_be_disabled() {
    let provider = FakeProvider::start(vec![Script::error(
        400,
        "invalid_request_error",
        "prompt is too long: 999999 > 200000",
    )])
    .await;

    let a = agent(&provider, vec![]).recover_from_overflow(false);

    let mut transcript = vec![Message::user(vec![ContentBlock::text("go")])];
    a.run(&mut transcript, &Cancel::new(), &mut |_| {})
        .await
        .expect_err("must fail without recovery");

    assert_eq!(provider.requests().len(), 1);
}
