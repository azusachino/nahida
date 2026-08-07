!!! note "Outline — not yet expanded to full prose"
    This chapter is scaffolded with the real shape and the real traps. Ask
    for it to be written in full when you're ready to go deeper than the
    outline.

# 10. The CLI

**Where:** `crates/nahida-cli/`

The only crate in the workspace allowed to know a terminal exists — and the
payoff of every boundary in the earlier chapters is that this crate is the
*only* one that had to change to add streaming output, Ctrl-C handling, and
confirmation prompts.

## What this chapter will cover

- **`main.rs` wires the other three crates together and nothing more** —
  parse flags, build a `Sandbox`, resolve a `Client`, build an `Agent`, run
  either `run_once` (one prompt, one answer) or `repl` (the transcript
  survives across prompts, which is also what keeps the prompt cache from
  chapter 1 hitting).
- **Ctrl-C is one long-lived `tokio::spawn`, not a signal handler per
  prompt** — it flips the shared `Cancel` flag from chapter 3 rather than
  killing the process, so a first Ctrl-C interrupts cleanly and a second one
  during that interrupt force-quits.
- **`render.rs` is the only file that knows about ANSI codes** — its own
  doc comment states the payoff directly: "the loop emits events and stays
  ignorant of whether anyone is watching, which is what lets the same loop
  back a test or a server later." Chapter 6's `FakeProvider` tests are proof
  this already works: they pass `&mut |_| {}` as the sink and never link
  against this file at all.
- **A concrete instance of the crate boundary paying off**: `confirm.rs`
  (chapter 8) and the `--compact-at` flag (chapter 7) were both added by
  writing new code in `nahida-cli` and one opt-in builder method in
  `nahida-agent` — nothing already working had to change shape to make room
  for either.

Next: [what's next](11-whats-next.md) — the gaps this tutorial itself
surfaced, and where to look for how a much larger project solves them.
