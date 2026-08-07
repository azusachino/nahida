//! `nahida` — the terminal front end.
//!
//! Wires the three crates together and does the two things a front end owes the
//! user: render progress as it happens, and make Ctrl-C stop the agent instead of
//! killing the process mid-turn.

mod confirm;
mod render;

use std::io::{IsTerminal as _, Write as _};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use clap::Parser;
use confirm::TerminalConfirm;
use nahida_agent::{Agent, AgentError, Cancel};
use nahida_llm::{Client, ContentBlock, Effort, Message};
use nahida_tools::Sandbox;
use render::Renderer;

#[derive(Parser)]
#[command(name = "nahida", version, about = "A small coding agent.")]
struct Cli {
    /// The task. Omit for an interactive session.
    prompt: Vec<String>,

    /// Model id. Defaults to the resolved provider's default.
    #[arg(short, long)]
    model: Option<String>,

    /// Reasoning effort: low | medium | high | xhigh | max.
    /// Ignored by compatible gateways that do not implement it.
    #[arg(short, long)]
    effort: Option<Effort>,

    /// Workspace root. Tools cannot read or write outside it.
    #[arg(short = 'C', long, default_value = ".")]
    root: std::path::PathBuf,

    /// Give up after this many tool-calling turns.
    #[arg(long, default_value_t = 32)]
    max_turns: u32,

    /// Summarize and replace the transcript once a turn's prompt reaches this
    /// many tokens. Off by default — the right value depends on the model's
    /// context window, which varies by provider.
    #[arg(long)]
    compact_at: Option<u64>,

    /// Cap on output tokens per turn. Defaults to the provider's default.
    #[arg(long)]
    max_tokens: Option<u32>,

    /// Retries for a rate limit, server overload, or transport error, with
    /// exponential backoff. 0 disables retrying.
    #[arg(long, default_value_t = 3)]
    max_retries: u32,

    /// Base backoff delay in ms; doubles each retry attempt.
    #[arg(long, default_value_t = 500)]
    retry_base_delay_ms: u64,

    /// Disable the one-shot compact-and-retry recovery on a real
    /// context-overflow error. On by default; independent of --compact-at.
    #[arg(long)]
    no_overflow_recovery: bool,

    /// Show summarized reasoning as it streams.
    #[arg(long)]
    thinking: bool,

    /// Show turn boundaries, token usage, and successful tool results.
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let sandbox = Sandbox::new(&cli.root)
        .with_context(|| format!("workspace root `{}` is not usable", cli.root.display()))?;

    // The credential error is the one a new user hits first, so let it speak for
    // itself instead of wrapping it in context.
    let client = Client::from_env()?;
    let profile = client.profile().clone();

    let model = cli.model.unwrap_or_else(|| profile.default_model.clone());
    let max_tokens = cli.max_tokens.unwrap_or(profile.default_max_tokens);

    if cli.verbose {
        eprintln!(
            "provider {} ({:?}) · model {model} · root {}",
            profile.name,
            profile.dialect,
            sandbox.root().display()
        );
    }

    let mut agent = Agent::new(client)
        .model(&model)
        .system(include_str!("prompt.md"))
        .tools(nahida_tools::default_set(&sandbox))
        .effort(cli.effort)
        .max_turns(cli.max_turns)
        .max_tokens(max_tokens)
        .max_retries(cli.max_retries)
        .retry_base_delay_ms(cli.retry_base_delay_ms)
        .recover_from_overflow(!cli.no_overflow_recovery)
        .show_thinking(cli.thinking);
    if let Some(threshold) = cli.compact_at {
        agent = agent.compact_at(threshold);
    }
    // With no one to answer a prompt, a scripted or piped invocation would
    // hang on a read that never comes — so gating only turns on when a human
    // is actually attached.
    if std::io::stdin().is_terminal() {
        agent = agent.confirm(Arc::new(TerminalConfirm));
    }

    let cancel = Cancel::new();
    {
        // One watcher for the whole process: each prompt resets the flag rather
        // than spawning another listener.
        let cancel = cancel.clone();
        tokio::spawn(async move {
            while tokio::signal::ctrl_c().await.is_ok() {
                eprintln!("\n(interrupting — Ctrl-C again to quit)");
                if cancel.is_cancelled() {
                    std::process::exit(130);
                }
                cancel.cancel();
            }
        });
    }

    let mut transcript: Vec<Message> = Vec::new();

    if cli.prompt.is_empty() {
        repl(&agent, &cancel, &mut transcript, cli.verbose).await
    } else {
        let prompt = cli.prompt.join(" ");
        run_once(&agent, &cancel, &mut transcript, &prompt, cli.verbose).await
    }
}

/// One prompt, one answer. Errors are reported and returned, not swallowed.
async fn run_once(
    agent: &Agent,
    cancel: &Cancel,
    transcript: &mut Vec<Message>,
    prompt: &str,
    verbose: bool,
) -> Result<()> {
    cancel.reset();
    transcript.push(Message::user(vec![ContentBlock::text(prompt)]));

    let mut renderer = Renderer::new(verbose);
    let result = agent.run(transcript, cancel, &mut |event| renderer.handle(&event)).await;
    renderer.finish();

    match result {
        Ok(outcome) => {
            if outcome.truncated() {
                eprintln!("\n(cut off at the output limit — raise --max-tokens to see the rest)");
            }
            if verbose {
                eprintln!("{} turns · {}", outcome.turns, render::format_usage(&outcome.usage));
            }
            Ok(())
        }
        // Interrupting is a choice the user made, not a failure to report back.
        Err(AgentError::Cancelled) => {
            eprintln!("(interrupted)");
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

/// Interactive session. The transcript carries across prompts, so follow-ups
/// keep their context — and so the prompt cache keeps hitting.
async fn repl(
    agent: &Agent,
    cancel: &Cancel,
    transcript: &mut Vec<Message>,
    verbose: bool,
) -> Result<()> {
    let interactive = std::io::stdin().is_terminal();
    if interactive {
        println!("nahida — Ctrl-C interrupts, Ctrl-D or `exit` quits.");
    }

    loop {
        if interactive {
            print!("\n› ");
            std::io::stdout().flush()?;
        }

        let mut line = String::new();
        if std::io::stdin().read_line(&mut line)? == 0 {
            break; // EOF
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if matches!(line, "exit" | "quit") {
            break;
        }

        // A failed prompt should not end the session — report it and keep going.
        if let Err(e) = run_once(agent, cancel, transcript, line, verbose).await {
            eprintln!("error: {e:#}");
        }
    }

    Ok(())
}
