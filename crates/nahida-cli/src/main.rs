//! `nahida` — the terminal front end.
//!
//! Wires the three crates together and does the two things a front end owes the
//! user: render progress as it happens, and make Ctrl-C stop the agent instead of
//! killing the process mid-turn.

mod confirm;
mod prompt;
mod render;
mod session;

use std::io::{IsTerminal as _, Write as _};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use clap::Parser;
use confirm::TerminalConfirm;
use nahida_agent::{Agent, AgentError, Cancel};
use nahida_llm::{ContentBlock, Effort, Message, Profile};
use nahida_tools::Sandbox;
use render::Renderer;
use session::SessionStore;

#[derive(Parser)]
#[command(name = "nahida", version, about = "A small coding agent.")]
// Independent CLI flags, not a state machine -- clap's derive wants each as
// its own field.
#[allow(clippy::struct_excessive_bools)]
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

    /// Print one JSON-encoded `AgentEvent` per line to stdout instead of
    /// human-readable rendering. For scripting; diagnostics still go to stderr.
    #[arg(long)]
    json: bool,

    /// Resume the most recent session for this workspace root.
    #[arg(short = 'c', long = "continue")]
    continue_session: bool,

    /// Resume a specific session by id. Overrides --continue.
    #[arg(long)]
    resume: Option<String>,

    /// Don't read or write a session log for this run.
    #[arg(long)]
    no_session: bool,

    /// Print the effective harness configuration and exit: provider, model,
    /// dialect, prompts, tools, approval policy, caching, compaction, and
    /// session format. Secret-free by construction — it reports what a
    /// credential resolved to (provider name, dialect), never the credential
    /// itself — and reproducible without spending a token, since nothing
    /// about it depends on an actual API call.
    #[arg(long)]
    describe: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let sandbox = Sandbox::new(&cli.root)
        .with_context(|| format!("workspace root `{}` is not usable", cli.root.display()))?;

    // Kernel-enforced (Landlock, Linux only): denies writes outside the
    // workspace root for this process and every child bash spawns, with no
    // API to lift it afterward. Best-effort on purpose — an unrelated kernel
    // config choice (old kernel, Landlock disabled, non-Linux) should never
    // stop the agent from starting; it just means this layer isn't there and
    // permission gating is carrying the whole weight of confining `bash`.
    if let Err(e) = nahida_tools::confine_writes(sandbox.root(), |msg| eprintln!("{msg}")) {
        eprintln!("warning: OS-level write confinement not applied: {e}");
    }

    // The credential error is the one a new user hits first, so let it speak for
    // itself instead of wrapping it in context.
    let provider = nahida_llm::resolve()?;
    let profile = provider.profile().clone();

    let model = cli.model.clone().unwrap_or_else(|| profile.default_model.clone());
    let max_tokens = cli.max_tokens.unwrap_or(profile.default_max_tokens);

    if cli.describe {
        print!("{}", describe(&cli, &profile, &model, max_tokens, &sandbox));
        return Ok(());
    }

    if cli.verbose {
        eprintln!(
            "provider {} ({:?}) · model {model} · root {}",
            profile.name,
            profile.dialect,
            sandbox.root().display()
        );
    }

    let mut agent = Agent::new(provider)
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
    let mut session = if cli.no_session {
        None
    } else {
        let sessions_dir = SessionStore::sessions_dir()?;
        if cli.continue_session || cli.resume.is_some() {
            let (store, loaded) =
                SessionStore::resume(&sessions_dir, sandbox.root(), cli.resume.as_deref())?;
            transcript = loaded;
            Some(store)
        } else {
            Some(SessionStore::create(&sessions_dir, sandbox.root())?)
        }
    };
    if let Some(s) = &session
        && cli.verbose
    {
        eprintln!("session {} (resume with `nahida --resume {}`)", s.id, s.id);
    }

    let ctx = RunCtx {
        agent: &agent,
        cancel: &cancel,
        sandbox: &sandbox,
        verbose: cli.verbose,
        json: cli.json,
    };

    if cli.prompt.is_empty() {
        repl(&ctx, &mut transcript, &mut session).await
    } else {
        let prompt = cli.prompt.join(" ");
        run_once(&ctx, &mut transcript, session.as_mut(), &prompt).await
    }
}

/// Report the effective harness configuration and exit, per ADR-0001: model,
/// dialect, prompts, tools, approval policy, caching, compaction, and session
/// format — everything reproducibility can silently depend on without this,
/// none of it a secret. Deliberately not a general profile system: this is
/// one function that reads state already resolved elsewhere and formats it,
/// not a new configuration surface of its own.
///
/// Returns a `String` rather than printing directly so it stays a plain
/// function `cargo test -p nahida-cli`'s unit tests can call and assert
/// against, the same way every other module in this bin crate is tested.
fn describe(
    cli: &Cli,
    profile: &Profile,
    model: &str,
    max_tokens: u32,
    sandbox: &Sandbox,
) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();

    writeln!(out, "provider      {} ({:?} dialect)", profile.name, profile.dialect).unwrap();
    writeln!(out, "model         {model}").unwrap();
    writeln!(
        out,
        "effort        {}",
        cli.effort.map_or("(provider default)".to_string(), |e| format!("{e:?}"))
    )
    .unwrap();
    writeln!(out, "thinking      {}", if cli.thinking { "on" } else { "off" }).unwrap();
    writeln!(out, "max tokens    {max_tokens}").unwrap();
    writeln!(out, "max turns     {}", cli.max_turns).unwrap();
    writeln!(
        out,
        "retries       {} (base delay {}ms, overflow recovery {})",
        cli.max_retries,
        cli.retry_base_delay_ms,
        if cli.no_overflow_recovery { "off" } else { "on" }
    )
    .unwrap();
    writeln!(out).unwrap();

    writeln!(out, "workspace     {}", sandbox.root().display()).unwrap();
    writeln!(
        out,
        "system prompt crates/nahida-cli/src/prompt.md ({} bytes, compiled in)",
        include_str!("prompt.md").len()
    )
    .unwrap();

    let tools = nahida_tools::default_set(sandbox);
    // Every tool shipped today decides `requires_confirmation` without
    // looking at its input (only `bash` overrides it, and unconditionally) —
    // see `bash.rs`'s own `always_requires_confirmation_regardless_of_input`
    // test — so probing with a placeholder input is representative. A tool
    // that started gating conditionally would make this line approximate
    // rather than wrong: still worth knowing, not worth blocking on here.
    let names: Vec<String> = tools
        .iter()
        .map(|t| {
            if t.requires_confirmation(&serde_json::Value::Null) {
                format!("{}*", t.name())
            } else {
                t.name().to_string()
            }
        })
        .collect();
    writeln!(out, "tools         {} (* requires confirmation)", names.join(", ")).unwrap();
    writeln!(
        out,
        "approval      {}",
        if std::io::stdin().is_terminal() {
            "interactive (tty attached — gated calls prompt)"
        } else {
            "ungated (no tty — gated calls run unconfirmed, same as pi's own no-popup default)"
        }
    )
    .unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "prompt cache  {}",
        match profile.dialect {
            nahida_llm::Dialect::Anthropic =>
                "enabled (system+tools breakpoint, moving breakpoint on messages)",
            nahida_llm::Dialect::Compat =>
                "disabled (Compat dialect strips cache_control before it reaches the wire)",
        }
    )
    .unwrap();
    writeln!(
        out,
        "compaction    {}",
        cli.compact_at.map_or("off".to_string(), |t| format!("at {t} prompt tokens"))
    )
    .unwrap();

    if cli.no_session {
        writeln!(out, "session       disabled for this run (--no-session)").unwrap();
    } else {
        match session::SessionStore::sessions_dir() {
            Ok(dir) => writeln!(
                out,
                "session       jsonl, format v{}, stored under {}",
                session::SESSION_FORMAT_VERSION,
                dir.display()
            )
            .unwrap(),
            Err(e) => {
                writeln!(
                    out,
                    "session       jsonl, format v{} ({e})",
                    session::SESSION_FORMAT_VERSION
                )
                .unwrap();
            }
        }
    }

    out
}

/// What every turn needs, independent of which turn it is. Bundled so
/// `run_once`/`repl` take one reference instead of growing a parameter each
/// time the CLI gains a mode.
struct RunCtx<'a> {
    agent: &'a Agent,
    cancel: &'a Cancel,
    sandbox: &'a Sandbox,
    verbose: bool,
    json: bool,
}

/// One prompt, one answer. Errors are reported and returned, not swallowed.
async fn run_once(
    ctx: &RunCtx<'_>,
    transcript: &mut Vec<Message>,
    session: Option<&mut SessionStore>,
    prompt: &str,
) -> Result<()> {
    ctx.cancel.reset();
    let prompt = prompt::expand_file_refs(prompt, ctx.sandbox);
    let session_start = transcript.len();
    transcript.push(Message::user(vec![ContentBlock::text(prompt)]));

    let mut renderer = Renderer::new(ctx.verbose);
    let result = if ctx.json {
        ctx.agent
            .run(transcript, ctx.cancel, &mut |event| {
                println!(
                    "{}",
                    serde_json::to_string(&event).expect("AgentEvent always serializes")
                );
            })
            .await
    } else {
        ctx.agent.run(transcript, ctx.cancel, &mut |event| renderer.handle(&event)).await
    };
    if !ctx.json {
        renderer.finish();
    }

    // Persisted regardless of `result`: even a cancelled or failed turn's
    // partial additions should survive to the next `--continue`, the same
    // way `transcript` itself stays usable in memory after an error.
    if let Some(session) = session {
        for message in &transcript[session_start..] {
            if let Err(e) = session.append(message) {
                eprintln!("warning: could not persist to session: {e}");
            }
        }
    }

    match result {
        Ok(outcome) => {
            // Diagnostics, not part of the event stream -- kept human-only.
            if !ctx.json {
                if outcome.truncated() {
                    eprintln!(
                        "\n(cut off at the output limit — raise --max-tokens to see the rest)"
                    );
                }
                if ctx.verbose {
                    eprintln!("{} turns · {}", outcome.turns, render::format_usage(&outcome.usage));
                }
            }
            Ok(())
        }
        // Interrupting is a choice the user made, not a failure to report back.
        Err(AgentError::Cancelled) => {
            if !ctx.json {
                eprintln!("(interrupted)");
            }
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

/// Interactive session. The transcript carries across prompts, so follow-ups
/// keep their context — and so the prompt cache keeps hitting.
async fn repl(
    ctx: &RunCtx<'_>,
    transcript: &mut Vec<Message>,
    session: &mut Option<SessionStore>,
) -> Result<()> {
    // In `--json` mode stdout is the event stream; the banner and prompt
    // indicator below are human chrome that would otherwise land in it.
    let interactive = std::io::stdin().is_terminal() && !ctx.json;
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
        if let Err(e) = run_once(ctx, transcript, session.as_mut(), line).await {
            eprintln!("error: {e:#}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod describe_tests {
    use super::*;

    fn cli() -> Cli {
        Cli {
            prompt: vec![],
            model: None,
            effort: None,
            root: ".".into(),
            max_turns: 32,
            compact_at: None,
            max_tokens: None,
            max_retries: 3,
            retry_base_delay_ms: 500,
            no_overflow_recovery: false,
            thinking: false,
            verbose: false,
            json: false,
            continue_session: false,
            resume: None,
            no_session: false,
            describe: true,
        }
    }

    fn sandbox() -> (tempfile::TempDir, Sandbox) {
        let dir = tempfile::tempdir().expect("tempdir");
        let sandbox = Sandbox::new(dir.path()).expect("sandbox");
        (dir, sandbox)
    }

    fn anthropic_profile() -> Profile {
        Profile {
            name: "anthropic",
            base_url: "https://api.anthropic.com".to_string(),
            dialect: nahida_llm::Dialect::Anthropic,
            default_model: "claude-opus-5".to_string(),
            default_max_tokens: 64_000,
        }
    }

    #[test]
    fn reports_the_resolved_provider_model_and_dialect() {
        let (_dir, sandbox) = sandbox();
        let profile = Profile {
            name: "zai",
            base_url: "https://api.z.ai/api/anthropic".to_string(),
            dialect: nahida_llm::Dialect::Compat,
            default_model: "glm-5.1".to_string(),
            default_max_tokens: 32_000,
        };
        let out = describe(&cli(), &profile, "glm-5.1", 32_000, &sandbox);
        assert!(out.contains("provider      zai (Compat dialect)"), "{out}");
        assert!(out.contains("model         glm-5.1"), "{out}");
        // Compat strips cache_control before it reaches the wire (chapter 1) —
        // describe must say so, or this line lies about what a run will do.
        assert!(out.contains("prompt cache  disabled"), "{out}");
    }

    #[test]
    fn marks_only_bash_as_requiring_confirmation() {
        let (_dir, sandbox) = sandbox();
        let out = describe(&cli(), &anthropic_profile(), "claude-opus-5", 64_000, &sandbox);
        let tools_line = out.lines().find(|l| l.starts_with("tools")).expect("a tools line");
        assert!(tools_line.contains("bash*"), "{tools_line}");
        assert!(tools_line.contains("read,"), "{tools_line}");
        assert!(!tools_line.contains("read*"), "{tools_line}");
    }

    #[test]
    fn honors_no_session() {
        let (_dir, sandbox) = sandbox();
        let c = Cli { no_session: true, ..cli() };
        let out = describe(&c, &anthropic_profile(), "claude-opus-5", 64_000, &sandbox);
        assert!(out.contains("session       disabled for this run (--no-session)"), "{out}");
    }

    #[test]
    fn reports_the_compaction_threshold_when_set() {
        let (_dir, sandbox) = sandbox();
        let c = Cli { compact_at: Some(5_000), ..cli() };
        let out = describe(&c, &anthropic_profile(), "claude-opus-5", 64_000, &sandbox);
        assert!(out.contains("compaction    at 5000 prompt tokens"), "{out}");
    }
}
