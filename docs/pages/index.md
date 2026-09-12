# Start here

This is a guided walk through `nahida`'s own code, in the order its concept
map (`AGENTS.md`) lists the ideas. It is not a Rust tutorial — it assumes you
can already read Rust — and it is not API documentation either. It is the
thing in between: *why* each piece exists, what problem it is the smallest
possible answer to, and which two or three lines would break if you got it
wrong.

## Why this exists

`nahida`'s own `AGENTS.md` puts it plainly:

> This is a learning project with a working product at the end of it, not a
> scaffold. The bet is that the core of a coding agent is small — the loop in
> every reference implementation is under a thousand lines — and that
> everything which makes those repos large is provider adapters, terminal UI,
> and session plumbing.

That bet is worth checking against something real. `refs/pi` — one of the
reference repos this project's workstation keeps around for comparison — is a
production-grade agent toolkit with the same shape as `nahida`: a provider
layer, an agent loop, a tool set, a terminal front end. It is also enormous:
thirty-plus provider adapters, session persistence, telemetry, a protocol
layer for client/server separation. Almost none of that size is the *loop* —
it's everything *around* the loop. `nahida` is what's left when you only build
the part that's actually small.

That means this tutorial can do something most agent documentation can't: show
you the *entire* mechanism, not a curated slice of it. By the end of the loop
chapter you will have read every line that decides what the agent does next.

## How to read this

The chapters follow the concept map in build order, which is also the order
each idea depends on the last:

1. **The provider layer** (`nahida-llm`) — talking to the API, knowing
   nothing about agents.
2. **Streaming and accumulation** — turning a byte stream into a structured
   response.
3. **The loop** (`nahida-agent`) — the ~200 lines that *are* the agent.
4. **Tools** — how the loop asks for work to be done without knowing what a
   file is.
5. **Sandboxing** (`nahida-tools`) — the one place untrusted input actually
   matters.
6. **Testing the loop** — proving the invariants without an API key.
7. **Context compaction**, **8. Permission gating**, **9. Evals** — the three
   features built after the walking skeleton, each with its own design
   constraints.
8. **The CLI** (`nahida-cli`) — the only crate allowed to know a terminal
   exists.
9. **What's next** — the gaps this tutorial itself surfaced.

Each chapter points at real files with real paths, not inline copies that can
drift from the source. Open the repo alongside this and follow along — the
tutorial tells you *why* a line is there; the file is still the truth about
what it says.

## Prerequisites

- Comfortable reading Rust: traits, `async`/`await`, `Arc`, ownership. This
  tutorial explains agent concepts, not the language.
- `nix develop` from the repo root gets you a working toolchain (`crates/`
  needs nothing beyond `rustc`/`cargo`; the tutorial site itself is a separate
  `mise`-managed Python project under `docs/`, kept apart on purpose so
  documentation tooling never leaks into the Rust devShell).
- No API key needed until [chapter 9, Evals](09-evals.md) — everything before
  that runs against a fake provider or doesn't run at all.

## Running this site

```bash
cd docs
mise install   # pulls uv
uv run mkdocs serve --dev-addr 0.0.0.0:1314   # or: make local
```
