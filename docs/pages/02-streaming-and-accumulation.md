!!! note "Outline — not yet expanded to full prose"
    This chapter is scaffolded with the real shape and the real traps. Ask
    for it to be written in full when you're ready to go deeper than the
    outline.

# 2. Streaming and accumulation

**Where:** `crates/nahida-llm/src/stream.rs`

The API doesn't hand back one JSON object — it streams Server-Sent Events,
and a single logical answer (text, thinking, tool calls) arrives as many
small deltas that have to be folded back into the `Response` shape from
chapter 1.

## What this chapter will cover

- **`SseDecoder` buffers bytes, not text.** An HTTP chunk can end in the
  middle of a multibyte UTF-8 character. Decoding each chunk as text on
  arrival corrupts any response containing non-ASCII. Frame boundaries
  (`\n\n` / `\r\n\r\n`) are safe to find in raw bytes because no UTF-8
  continuation byte can be `\n` or `\r` — that's *why* bytes, not text.
- **A streamed tool call doesn't arrive as JSON.** It arrives as a sequence
  of `InputJsonDelta { partial_json }` fragments that only parse once fully
  concatenated. `Accumulator`'s `Partial::ToolUse { json: String, .. }`
  exists to hold the growing buffer; `serde_json::from_str` is only ever
  called once, in `finish()`.
- **Two data streams carry `Usage` at different points**: input/cache token
  counts arrive once, in `MessageStart`; output tokens arrive incrementally
  in each `MessageDelta`. Getting this wrong means reporting `0` for one of
  them.
- **The test suite intentionally splits every scripted response mid-frame**
  (`tests/support/mod.rs` writes responses in pieces with a pause between
  them) so the decoder's buffering is exercised by every test, not just its
  own unit tests. Read `stream.rs`'s `#[cfg(test)]` module — especially
  `survives_a_chunk_boundary_inside_a_multibyte_char` — for the traps this
  design specifically defends against.

Next: [the loop](03-the-loop.md) — where a completed `Response` actually gets
used.
