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

    /// Provider to use. Omit to retain environment credential precedence.
    /// `ChatGPT` is reserved; official sign-in is not implemented yet.
    #[arg(long, value_parser = ["anthropic", "zai", "zai-coding-cn", "chatgpt"])]
    provider: Option<String>,

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
    /// dialect, tools, approval policy, caching, compaction, sandbox, and
    /// session format. No network call, and works even with no provider
    /// credentials set. Never prints a credential — see `describe`'s own
    /// doc comment for exactly what that guarantee does and doesn't cover.
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
    // `Ok(false)` (not fully enforced, or not on Linux at all) is a real,
    // expected outcome here, not an error -- only a hard Landlock failure is.
    // `--describe` needs the actual bool, not just whether this returned
    // `Ok`, to report sandbox status honestly.
    let confined = match nahida_tools::confine_writes(sandbox.root(), |msg| eprintln!("{msg}")) {
        Ok(enforced) => enforced,
        Err(e) => {
            eprintln!("warning: OS-level write confinement not applied: {e}");
            false
        }
    };

    // Handled before resolving a provider: unlike every other mode, --describe
    // has to stay useful with no credentials configured at all -- that's the
    // state a new user troubleshooting "why won't this start" is actually in.
    if cli.describe {
        let resolution = nahida_llm::provider::inspect_profile(cli.provider.as_deref());
        let info = resolution.as_ref().ok().map(|p| resolved_provider(&cli, p));
        let text = describe(&cli, info.as_ref(), resolution.as_ref().err(), confined, &sandbox);
        // In --json mode stdout is the event stream (see run_once's own
        // comment on the same rule) -- this diagnostic goes to stderr there
        // instead of corrupting it.
        if cli.json {
            eprint!("{text}");
        } else {
            print!("{text}");
        }
        return Ok(());
    }

    // The credential error is the one a new user hits first, so let it speak for
    // itself instead of wrapping it in context.
    let provider = nahida_llm::provider::resolve_named(cli.provider.as_deref())?;
    let profile = provider.profile().clone();
    let ResolvedProvider { model, max_tokens, .. } = resolved_provider(&cli, &profile);

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

/// What `--describe` (and the real run path) need from a resolved provider —
/// bundled once so `main` doesn't compute `model`/`max_tokens` twice with
/// slightly different logic in each spot.
struct ResolvedProvider {
    profile: Profile,
    model: String,
    max_tokens: u32,
}

fn resolved_provider(cli: &Cli, profile: &Profile) -> ResolvedProvider {
    ResolvedProvider {
        model: cli.model.clone().unwrap_or_else(|| profile.default_model.clone()),
        max_tokens: cli.max_tokens.unwrap_or(profile.default_max_tokens),
        profile: profile.clone(),
    }
}

/// Report the effective harness configuration and exit, per ADR-0001: model,
/// dialect, tools, approval policy, caching, compaction, sandbox, and session
/// format — everything reproducibility can silently depend on without this.
/// Deliberately not a general profile system: this is one function that
/// reads state already resolved elsewhere and formats it, not a new
/// configuration surface of its own.
///
/// Never prints a credential — but that's a property of what this function
/// is given and chooses to print, not a type-level guarantee. `base_url` on
/// `ResolvedProvider::profile` is deliberately never echoed here: a custom
/// `ANTHROPIC_BASE_URL` can embed a token in its path or userinfo, and
/// `Profile` carries whatever the environment gave it verbatim (see
/// `crate::provider::resolve` in `nahida-llm`). That omission also means the
/// single most useful fact after the model — which endpoint is actually in
/// play — is missing from the output on purpose, not an oversight.
///
/// Works with no provider resolved at all (`resolved` is `Err`) — this is
/// deliberately handled before `main` would otherwise propagate that error
/// and exit, so `--describe` stays useful for inspecting sandbox, tool, and
/// session state while troubleshooting exactly why no provider resolved.
///
/// Returns a `String` rather than printing directly so it stays a plain
/// function `cargo test -p nahida-cli`'s unit tests can call and assert
/// against, the same way every other module in this bin crate is tested.
// The provider/model split by resolution state, and the section count this
// now covers (sandbox, an unresolved-provider fallback path), push this past
// clippy's line-count lint on formatting alone -- nothing here is complex,
// it's just a lot of fields to report.
#[allow(clippy::too_many_lines)]
fn describe(
    cli: &Cli,
    resolved: Option<&ResolvedProvider>,
    resolve_error: Option<&nahida_llm::Error>,
    sandbox_confined: bool,
    sandbox: &Sandbox,
) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();

    if let Some(r) = resolved {
        writeln!(out, "provider      {} ({:?} dialect)", r.profile.name, r.profile.dialect)
            .unwrap();
        writeln!(out, "model         {}", r.model).unwrap();
    } else {
        let reason = resolve_error.map_or("no credentials found".to_string(), ToString::to_string);
        writeln!(out, "provider      (unresolved: {reason})").unwrap();
        writeln!(out, "model         (unresolved)").unwrap();
    }
    writeln!(
        out,
        "effort        {}",
        cli.effort.map_or("(provider default)".to_string(), |e| format!("{e:?}"))
    )
    .unwrap();
    writeln!(out, "thinking      {}", if cli.thinking { "on" } else { "off" }).unwrap();
    match resolved {
        Some(r) => writeln!(out, "max tokens    {}", r.max_tokens).unwrap(),
        None => writeln!(
            out,
            "max tokens    {}",
            cli.max_tokens.map_or("(provider default)".to_string(), |t| t.to_string())
        )
        .unwrap(),
    }
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
        "sandbox       {}",
        if sandbox_confined {
            "Landlock write confinement active (Linux) -- writes outside workspace denied at the kernel level"
        } else {
            "no OS-level write confinement (non-Linux, or Landlock unavailable) -- permission gating alone confines bash"
        }
    )
    .unwrap();
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

    match resolved {
        Some(r) => writeln!(
            out,
            "prompt cache  {}",
            match r.profile.dialect {
                nahida_llm::Dialect::Anthropic =>
                    "enabled (system+tools breakpoint, moving breakpoint on messages)",
                // Compat covers two different reasons, and Profile alone
                // can't tell them apart: an Anthropic-shaped gateway that
                // doesn't understand cache_control (it gets stripped by
                // Dialect::adapt), or a provider on a different wire format
                // entirely (Dialect is an unused placeholder there — see
                // Profile::dialect's own doc comment). Either way the
                // observable fact is the same: nothing is cached.
                nahida_llm::Dialect::Compat =>
                    "disabled (Compat gateway, or a non-Anthropic wire format where caching doesn't apply)",
            }
        )
        .unwrap(),
        None => writeln!(out, "prompt cache  (unresolved — depends on which provider resolves)")
            .unwrap(),
    }
    writeln!(
        out,
        "compaction    {}",
        cli.compact_at.map_or("off".to_string(), |t| format!("at {t} prompt tokens"))
    )
    .unwrap();

    if cli.no_session {
        writeln!(out, "session       disabled for this run (--no-session)").unwrap();
    } else {
        match session::SessionStore::sessions_path() {
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
            provider: None,
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

    fn resolved(profile: Profile, model: &str, max_tokens: u32) -> ResolvedProvider {
        ResolvedProvider { profile, model: model.to_string(), max_tokens }
    }

    fn anthropic_resolved() -> ResolvedProvider {
        resolved(anthropic_profile(), "claude-opus-5", 64_000)
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
        let r = resolved(profile, "glm-5.1", 32_000);
        let out = describe(&cli(), Some(&r), None, true, &sandbox);
        assert!(out.contains("provider      zai (Compat dialect)"), "{out}");
        assert!(out.contains("model         glm-5.1"), "{out}");
        // Compat covers both a stripping gateway and a non-Anthropic wire
        // format -- describe must not claim it's specifically the former.
        assert!(out.contains("prompt cache  disabled"), "{out}");
        assert!(!out.contains("strips cache_control"), "{out}");
    }

    #[test]
    fn reports_caching_as_enabled_on_the_anthropic_dialect() {
        let (_dir, sandbox) = sandbox();
        let r = anthropic_resolved();
        let out = describe(&cli(), Some(&r), None, true, &sandbox);
        assert!(out.contains("prompt cache  enabled"), "{out}");
    }

    #[test]
    fn marks_only_bash_as_requiring_confirmation() {
        let (_dir, sandbox) = sandbox();
        let r = anthropic_resolved();
        let out = describe(&cli(), Some(&r), None, true, &sandbox);
        let tools_line = out.lines().find(|l| l.starts_with("tools")).expect("a tools line");
        let list = tools_line
            .strip_prefix("tools         ")
            .and_then(|s| s.split_once(" (* requires confirmation)"))
            .expect("the tools line has the expected shape")
            .0;
        let names: Vec<&str> = list.split(", ").collect();
        assert_eq!(names, ["read", "write", "edit", "bash*", "find", "grep", "ls"], "{tools_line}");
    }

    #[test]
    fn honors_no_session() {
        let (_dir, sandbox) = sandbox();
        let c = Cli { no_session: true, ..cli() };
        let r = anthropic_resolved();
        let out = describe(&c, Some(&r), None, true, &sandbox);
        assert!(out.contains("session       disabled for this run (--no-session)"), "{out}");
    }

    #[test]
    fn reports_the_compaction_threshold_when_set() {
        let (_dir, sandbox) = sandbox();
        let c = Cli { compact_at: Some(5_000), ..cli() };
        let r = anthropic_resolved();
        let out = describe(&c, Some(&r), None, true, &sandbox);
        assert!(out.contains("compaction    at 5000 prompt tokens"), "{out}");
    }

    #[test]
    fn reports_sandbox_confinement_status() {
        let (_dir, sandbox) = sandbox();
        let r = anthropic_resolved();
        let confined = describe(&cli(), Some(&r), None, true, &sandbox);
        assert!(confined.contains("sandbox       Landlock write confinement active"), "{confined}");
        let unconfined = describe(&cli(), Some(&r), None, false, &sandbox);
        assert!(unconfined.contains("sandbox       no OS-level write confinement"), "{unconfined}");
    }

    #[test]
    fn works_with_no_provider_resolved() {
        // The whole point: --describe must stay useful for inspecting
        // sandbox/tool/session state even when no credential is configured
        // at all, not just when a provider already resolved successfully.
        let (_dir, sandbox) = sandbox();
        let err = nahida_llm::Error::NoCredentials;
        let out = describe(&cli(), None, Some(&err), true, &sandbox);
        assert!(out.contains("provider      (unresolved:"), "{out}");
        assert!(out.contains("model         (unresolved)"), "{out}");
        assert!(out.contains("prompt cache  (unresolved"), "{out}");
        // Everything that doesn't depend on a provider still has to work.
        assert!(out.contains("sandbox       Landlock"), "{out}");
        let tools_line = out.lines().find(|l| l.starts_with("tools")).expect("a tools line");
        assert!(tools_line.contains("bash*"), "{tools_line}");
    }
}
