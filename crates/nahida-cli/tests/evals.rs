//! End-to-end evals against a real provider.
//!
//! `nahida-agent`'s own tests (`crates/nahida-agent/tests/`) prove the loop's
//! *mechanical* correctness against a fake provider — no network, no cost.
//! These ask a different question: given a real task, does the agent actually
//! get it right? That needs a real model call, which costs tokens and money,
//! so every eval here is `#[ignore]`d and never runs from `make check` or CI.
//! Run them deliberately with `make eval`.
//!
//! Add a task by writing a new `#[tokio::test]` that calls [`run_eval`] with a
//! prompt, a fixture setup closure, and a check closure. Keep checks as simple
//! as "does the expected file have the expected content" — that is enough to
//! catch a real regression without building a task-description format nobody
//! asked for yet.

use std::path::Path;

use nahida_agent::{Agent, Cancel};
use nahida_llm::{Client, ContentBlock, Message};
use nahida_tools::Sandbox;

/// Run `prompt` against a real provider in a scratch workspace seeded by
/// `setup`, then hand the finished workspace to `check`.
async fn run_eval(
    prompt: &str,
    setup: impl FnOnce(&Path),
    check: impl FnOnce(&Path) -> Result<(), String>,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());

    let sandbox = Sandbox::new(dir.path()).expect("sandbox");
    let client = Client::from_env()
        .expect("no credentials — set ANTHROPIC_API_KEY, ZAI_API_KEY, or ANTHROPIC_AUTH_TOKEN");
    let profile = client.profile().clone();

    let agent = Agent::new(client)
        .model(&profile.default_model)
        .system(include_str!("../src/prompt.md"))
        .tools(nahida_tools::default_set(&sandbox))
        .max_tokens(profile.default_max_tokens)
        .max_turns(10);

    let mut transcript = vec![Message::user(vec![ContentBlock::text(prompt)])];
    let outcome =
        agent.run(&mut transcript, &Cancel::new(), &mut |_| {}).await.expect("agent run failed");

    println!(
        "finished in {} turns · {} in / {} out",
        outcome.turns, outcome.usage.input_tokens, outcome.usage.output_tokens
    );

    if let Err(msg) = check(dir.path()) {
        panic!("eval failed: {msg}");
    }
}

#[tokio::test]
#[ignore = "costs tokens; run with `make eval`"]
async fn writes_a_new_file() {
    run_eval(
        "Create a file named hello.txt in the workspace root containing exactly \
         this one line: hello from nahida",
        |_root| {},
        |root| {
            let content = std::fs::read_to_string(root.join("hello.txt"))
                .map_err(|e| format!("hello.txt: {e}"))?;
            if content.trim() != "hello from nahida" {
                return Err(format!("unexpected content: {content:?}"));
            }
            Ok(())
        },
    )
    .await;
}

#[tokio::test]
#[ignore = "costs tokens; run with `make eval`"]
async fn fixes_a_typo_in_an_existing_file() {
    run_eval(
        "notes.txt has one typo: \"recieve\" should be \"receive\". Fix it and \
         leave everything else in the file unchanged.",
        |root| {
            std::fs::write(root.join("notes.txt"), "Please recieve this package by Friday.\n")
                .expect("seed fixture");
        },
        |root| {
            let content = std::fs::read_to_string(root.join("notes.txt"))
                .map_err(|e| format!("notes.txt: {e}"))?;
            if content.contains("recieve") {
                return Err(format!("typo still present: {content:?}"));
            }
            if !content.contains("receive") {
                return Err(format!("fix did not land: {content:?}"));
            }
            Ok(())
        },
    )
    .await;
}
