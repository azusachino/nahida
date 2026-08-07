!!! note "Outline — not yet expanded to full prose"
    This chapter is scaffolded with the real shape and the real traps. Ask
    for it to be written in full when you're ready to go deeper than the
    outline.

# 7. Context compaction

**Where:** `Agent::compact` / `Agent::compact_at`, `crates/nahida-agent/src/compact.md`

The transcript grows every turn and nothing ever shrinks it — eventually a
long session hits the context window, even with prompt caching (caching
only makes the growing prefix cheap, not smaller).

## What this chapter will cover

- **Opt-in, off by default** (`compact_at(tokens)`), same shape as every
  other optional `Agent` builder method — the right threshold depends on a
  model's context window, which this crate deliberately doesn't track per
  provider.
- **Fires only *between* turns**, checked at the top of `run()`'s loop, never
  mid-tool-exchange — the transcript is always "settled" there (every
  `tool_use` already has its matching `tool_result`), so replacing it can't
  split a pending exchange.
- **A real bug worth knowing about, because the fix is the interesting
  part**: the first version appended a synthetic "please summarize" user
  message to the transcript before sending it. That's wrong whenever the
  transcript already ends on a user message (e.g. right after a tool
  result) — two consecutive user-role messages is something the real API
  rejects. The fix: the instruction lives in the *system prompt*
  (`compact.md`) instead, and the transcript is sent to the summarization
  call exactly as-is, whatever role it happens to end on.
- **The replacement is a single user message, not a user/assistant pair** —
  because whatever calls `compact()` always appends an assistant message
  next (the turn that follows), and the API requires starting from `user`.
- **The compaction call's own cost gets folded into `Outcome.usage`**,
  because a summarization call is itself a real API call with a real
  price — dropping it from the reported total would just be wrong
  accounting.

Next: [permission gating](08-permission-gating.md) — the second feature, and
the one with a genuinely open design question about what "asking the user"
means when the loop isn't allowed to know a terminal exists.
