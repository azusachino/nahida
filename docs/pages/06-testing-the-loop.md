!!! note "Outline — not yet expanded to full prose"
    This chapter is scaffolded with the real shape and the real traps. Ask
    for it to be written in full when you're ready to go deeper than the
    outline.

# 6. Testing the loop

**Where:** `crates/nahida-agent/tests/`

No API key, no network, no cost — and yet these tests catch the exact bugs
that would otherwise only show up as a silently degraded model (fewer
parallel tool calls, a wedged conversation) weeks later.

## What this chapter will cover

- **`FakeProvider` serves scripted SSE responses over a real local TCP
  listener** — it's not a mock of the `Client`, it's a real HTTP server the
  real `Client` talks to. That's deliberate: it exercises the real streaming
  and decoding path from chapter 2, not a stand-in for it.
- **It records every request it receives, not just the responses it
  serves.** `support::transcript_shape()` turns a captured request into
  `Vec<(role, [block types])>` — the shape assertions in `agent_loop.rs`
  read like `[(user, [text]), (assistant, [text, tool_use, tool_use]),
  (user, [tool_result, tool_result])]`. The interesting question isn't "did
  the loop return a value," it's "what transcript did it build" — that's
  where the invariants from chapter 3 actually live.
- **Every scripted response is split at a non-frame boundary** before being
  written to the socket, in two pieces with a pause between them — so the
  SSE decoder's buffering is exercised by every test in this file, not just
  `stream.rs`'s own unit tests.
- **A documented habit worth adopting**: after writing a loop test, break
  the loop on purpose and check the test fails. A test that passes against
  the bug it names is worse than no test.
- Walk a couple of the actual tests: `runs_a_tool_then_finishes` (the happy
  path), `replays_the_assistant_turn_and_batches_results_into_one_user_message`
  (the invariant that would silently kill parallel tool calls if broken),
  and `cancelling_stops_the_loop_and_keeps_the_transcript` (proving `Cancel`
  from chapter 3 actually leaves something resumable).

Next: [context compaction](07-context-compaction.md) — the first feature
built on top of the walking skeleton, and the first one with a real
API-shaped constraint to design around (message role alternation).
