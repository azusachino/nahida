# 0. Why nahida exists

Before any code: what is the actual bet this project is making, and what
rule falls out of it that you'll see enforced everywhere you look?

## The bet

Read enough agent codebases and a pattern shows up: the part that decides
*what happens next* — send a message, read the response, run a tool if asked,
go again — is small. Genuinely small, often under a thousand lines. Everything
that makes a real agent project large is *around* that loop: a dozen provider
adapters translating between wire formats, a terminal UI rendering streaming
tokens, session persistence surviving a restart, telemetry, auth flows,
retry policies tuned per provider.

`nahida`'s bet is that you can learn the interesting part — the loop — without
ever building the parts that make it large. So it doesn't. One provider today
(with a second one's endpoint reachable through a compatibility mode, not a
second adapter). No session persistence. No telemetry. A terminal front end
thin enough that deleting it wouldn't change what the loop does.

## The rule this produces

> **One crate, one concept. A crate may not know what the crate above it
> does.**

That's `AGENTS.md`'s own phrasing, and it's not a tidiness preference — it's
what makes the bet checkable. If `nahida-agent` could reach into
`nahida-tools` to know what a file is, "the loop is small" would stop being a
claim you could verify by reading one file; it would depend on knowing
everything every tool might do. The boundary is what keeps "read `agent.rs`
and you've read the loop" true.

Four crates, four concepts, strictly layered:

| Crate | Knows | Does not know |
| --- | --- | --- |
| `nahida-llm` | The Anthropic Messages API: wire types, auth, streaming, dialects | What a "turn" or an "agent" is |
| `nahida-agent` | Turns, tool dispatch, cancellation, events | What a file or a terminal is |
| `nahida-tools` | The filesystem, a sandboxed shell | That a terminal exists |
| `nahida-cli` | The `nahida` binary: flags, REPL, rendering | Nothing above it — it's the top |

You'll see this boundary tested directly, twice, later in this tutorial: once
in [chapter 5](05-sandboxing.md), where `nahida-tools` has to defend against a
path the model made up without any help from the loop above it; and once in
[chapter 10](10-the-cli.md), where the entire terminal-rendering layer could
be deleted without the loop noticing, because it was never told anyone was
watching.

## Prompts live next to the code they describe

One more convention worth knowing before you start reading: every tool
description and the system prompt live in `.md` files beside the Rust that
uses them (`crates/*/src/*.md`), pulled in with `include_str!` rather than
buried in a string constant. This isn't cosmetic. A tool's description is
what the model reads to decide *when* to call it — it's load-bearing prose,
not a comment — and prose you can't read and edit as prose tends to rot. You'll
see this in [chapter 4](04-tools.md), where a tool's `description()` is
argued to matter as much as its `call()`.

## The concept map is the syllabus

`AGENTS.md` tracks a table of what's built and what's still open, in the
order each thing was added — provider abstraction first, then the loop, then
tools, then the things that only make sense once a working loop exists
(compaction, gating, evals). This tutorial follows that same order for the
same reason the project was built in it: each chapter's problem doesn't
exist yet, in a meaningful sense, until the chapter before it is done.

Next: [the provider layer](01-the-provider-layer.md), where the bet starts —
what's the smallest thing that can talk to a model?
