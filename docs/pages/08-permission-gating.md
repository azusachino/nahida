!!! note "Outline — not yet expanded to full prose"
    This chapter is scaffolded with the real shape and the real traps. Ask
    for it to be written in full when you're ready to go deeper than the
    outline.

# 8. Permission gating

**Where:** `Tool::requires_confirmation`, `Agent::confirm`,
`crates/nahida-agent/src/confirm.rs`, `crates/nahida-cli/src/confirm.rs`

`bash` ran unconditionally the moment the model asked for it — this is where
that stops being true, and where the "the loop doesn't know a terminal
exists" rule from chapter 0 gets tested for real.

## What this chapter will cover

- **`Tool::requires_confirmation` is a *provided* trait method**, default
  `false` — added without breaking a single existing `impl Tool`, including
  the test fixtures. A tool opts in explicitly (`Bash` overrides it to
  always return `true`); nothing else changes for tools that don't.
- **The seam that had to exist and didn't**: `AgentEvent`'s sink is
  fire-and-forget (`&mut dyn FnMut(AgentEvent)`, no return value) — fine for
  progress, useless for a decision the loop has to *wait* on. The answer is
  a separate trait, `Confirm`, with its own `async fn ask`, registered on the
  `Agent` independently of the event sink.
- **Confirmation is resolved sequentially, before dispatch runs anything
  concurrently.** Terminal interaction can't be parallelized the way tool
  execution can — you can't show two `y/N` prompts at once — so `dispatch()`
  walks every call once to collect approve/deny decisions, *then* runs every
  approved call through the same concurrent `join_all` as before.
- **A denial is a `ToolOutcome::err(...)`, never a dropped result** — same
  discipline as an unknown tool in chapter 3.
- **Why `bash` gates *all* calls, not risky ones**: its own doc comment
  already predicted this — "the harness sees only an opaque command string,
  so it cannot tell a `grep` from a `git push --force`." Selective gating
  needs dangerous actions split into their own tools first; see
  [what's next](11-whats-next.md).
- **`nahida-cli` only wires the terminal-prompting handler when stdin is a
  tty** — a scripted or piped invocation runs ungated instead of hanging on
  a read that will never come.

Next: [evals](09-evals.md) — the third feature, and the first one that costs
real money to run.
