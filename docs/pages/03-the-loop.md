!!! note "Outline — not yet expanded to full prose"
    This chapter is scaffolded with the real shape and the real traps. Ask
    for it to be written in full when you're ready to go deeper than the
    outline.

# 3. The loop

**Where:** `crates/nahida-agent/src/agent.rs`

This is the chapter the whole project is a bet on: read this one file and
you've read the agent. `agent.rs`'s own opening comment states the whole idea
in three sentences — "send the transcript, read the response, and look at
`stop_reason`. If it is `tool_use`, run the tools, append the results, and go
round again." Everything else is bookkeeping.

## What this chapter will cover

- **`Agent` is a builder over one method, `run()`.** Walk `Agent::new`'s
  defaults and every `#[must_use]` builder method, then `run()`'s `for turn
  in 1..=self.max_turns` loop.
- **The two invariants**, stated in the file's own doc comment and worth
  memorizing: the assistant turn goes back *verbatim* (so `tool_use` blocks
  have something for `tool_result` blocks to attach to), and every tool
  result rides in *one* user message (splitting them teaches the model to
  stop calling tools in parallel). Both are actively tested — see
  chapter 6.
- **The `stop_reason` match is the whole decision tree**: `Refusal` (never
  read `content` — it's empty or partial), `PauseTurn` (replay verbatim, no
  synthetic "continue" message), `ToolUse` (the branch that loops), and the
  catch-all `other` (`EndTurn`/`MaxTokens`/`StopSequence` — the turn is the
  answer).
- **`dispatch()` runs every requested tool concurrently** via
  `futures_util::future::join_all`, and still returns a result for a tool
  that doesn't exist — a dropped result would wedge the conversation forever,
  so "unknown tool" comes back as an error result, not a panic or a silent
  drop.
- **`Cancel` is cooperative, not preemptive** (`crates/nahida-agent/src/cancel.rs`):
  a flag checked between stream events and between turns, never mid-write —
  those are the only points where stopping leaves the transcript in a state
  a resumed session could use.

Next: [tools](04-tools.md) — what the loop is calling when `stop_reason` is
`ToolUse`.
