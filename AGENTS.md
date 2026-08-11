# nahida

An agent, built to be understood.

## Why this exists

This is a learning project with a working product at the end of it, not a
scaffold. The bet is that the core of a coding agent is small — the loop in every
reference implementation is under a thousand lines — and that everything which
makes those repos large is provider adapters, terminal UI, and session plumbing.
So nahida implements the concepts one at a time, in a tree where each concept has
exactly one home, and each new one lands on something that already runs.

## The contract

**One crate, one concept. A crate may not know what the crate above it does.**

`nahida-llm` knows the Anthropic Messages API and does not know what an agent is.
`nahida-agent` knows turns and tools and does not know what a file or a terminal
is. `nahida-tools` knows the filesystem. `nahida-cli` knows the terminal.

That split is the point, not tidiness: the loop has to be testable against a fake
provider, adding a second provider must not reach into the loop, and rendering
progress must not require the loop to know anyone is watching. When a change
wants to cross one of those lines, the line is usually right and the change is
usually in the wrong crate.

## Layout

| Path | What it is |
| --- | --- |
| `crates/nahida-llm/` | Provider: wire types, auth, dialects, SSE streaming |
| `crates/nahida-agent/` | The loop: turns, tool dispatch, cancellation, events |
| `crates/nahida-tools/` | `read`, `write`, `edit`, `bash`, `find`, `grep`, `ls`, and path confinement |
| `crates/nahida-cli/` | The `nahida` binary: flags, REPL, rendering, `@file` expansion, session persistence |
| `crates/*/src/*.md` | Tool descriptions and the system prompt, next to the code |
| `docs/` | The 0-to-hero tutorial: a concept-by-concept walkthrough of this repo, MkDocs Material, `mise`+`uv`-managed and kept apart from the Rust devShell |

Prompt text lives in `.md` files beside the code it describes and is pulled in
with `include_str!`. Descriptions are load-bearing — they are how the model
decides when to call a tool — so they belong somewhere you can read and edit as
prose, not buried in a string constant.

This split mirrors `earendil-works/pi`'s real package boundary once you strip
its remote-session support: `ai` → `agent` → `coding-agent` there is
`nahida-llm` → `nahida-agent` → `nahida-tools`+`nahida-cli` here. pi's
`protocol`/`client`/`server` packages exist only to run a session remotely,
and `telemetry` has no consumer nahida needs — both are deliberately not
ported; adding them here would be structure with nothing using it.

## The concept map

Where each idea lives, and which are still to come. Tracked as
`nahida:build-the-agent` in asobi.

| Concept | Where | Status |
| --- | --- | --- |
| Provider abstraction, streaming, dialects | `nahida-llm` | done |
| The agent loop, `stop_reason`, cancellation | `nahida-agent/agent.rs` | done |
| Tool schemas and parallel dispatch | `nahida-agent/tool.rs`, `nahida-tools` | done |
| Staleness-checked edit (targeted replacement, not a blind overwrite) | `nahida-tools/src/edit.rs` | done |
| Path confinement | `nahida-tools/sandbox.rs` | done |
| Prompt caching (tools+system prefix breakpoint, plus a moving breakpoint on the growing transcript) | `Agent::system`, `ContentBlock::mark_cached`, `Agent::request` | done |
| Loop regression tests vs a fake provider | `nahida-agent/tests/` | done |
| Context compaction | `Agent::compact`, opt-in via `compact_at` | done |
| Permission gating | `Tool::requires_confirmation`, `Agent::confirm` | done (bash only, all-or-nothing) |
| Evals | `crates/nahida-cli/tests/evals.rs`, `make eval` | done (2 tasks) |
| Reliability: retry with backoff, reactive overflow recovery | `Agent::max_retries`, `Agent::recover_from_overflow` | done |
| OS-level write confinement (Landlock) | `nahida-tools/src/os_sandbox.rs` | done, Linux only — macOS deliberately deferred (see `docs/pages/11-whats-next.md`) |
| Gitignore-aware search (`find`, `grep`), directory listing (`ls`) | `nahida-tools/src/{find,grep,ls}.rs` | done |
| `@file` prompt expansion | `nahida-cli/src/prompt.rs` | done |
| Machine-readable event stream (`--json`) | `AgentEvent: Serialize`, `nahida-cli/src/main.rs` | done |
| Session persistence (JSONL log, `--continue`/`--resume`) | `nahida-cli/src/session.rs` | done |

## Testing the loop

`crates/nahida-agent/tests/support/` is a fake Anthropic-compatible provider: it
serves a scripted list of SSE responses and records every request body. No API
key, no network, no cost — so the loop's invariants are cheap to assert and cheap
to keep asserting.

It records requests as well as serving responses because that is where the
invariants live. The interesting question is not "did the loop return a value" but
"what transcript did it build" — that a tool-use turn is replayed verbatim, that
every result comes back, that they all ride in one user message. Add a script
shape to `support/mod.rs` and a case to `agent_loop.rs` for each new loop
behaviour.

Two habits worth keeping:

- Responses are written in pieces split off a frame boundary, so every test
  exercises the SSE decoder's buffering rather than leaving it to its unit tests.
- After writing a loop test, break the loop on purpose and check the test fails.
  A test that passes against the bug it names is worse than no test.

## Evals

`crates/nahida-cli/tests/evals.rs` asks a different question than the loop
regression tests: given a real task, does the agent actually get it right?
That needs a real model call, which costs tokens and money, so every eval is
`#[ignore]`d — `make check` and CI compile them but never execute one. Run
them deliberately with `make eval`, which needs real provider credentials in
the environment.

Add a task with `run_eval(prompt, setup, check)`: `setup` seeds the scratch
workspace, `check` inspects it afterward. Keep checks as simple as "does the
expected file have the expected content" — that catches a real regression
without a task-description format nobody asked for yet.

## Working here

```bash
nix develop
make run ARGS="what does this repo do"
make check      # fmt + clippy -D warnings + tests, before every commit
make validate   # check + release build, before a PR
make eval       # real-provider evals -- costs tokens, run deliberately
make tutorial   # serve docs/ locally -- the 0-to-hero walkthrough
```

## Rules for agents

1. **Respect the crate boundaries** above. A new dependency edge that points
   upward is a design error, not a shortcut.
2. **A tool failure is a `ToolOutcome` with `is_error`, never a Rust `Err`.**
   Returning `Err` ends the turn; "file not found" is information the model should
   act on, so it goes back as a result.
3. **Every tool result must be returned, and all of them in one user message.**
   A dropped result wedges the conversation; splitting them across messages
   teaches the model to stop making parallel calls.
4. **Every model-supplied path goes through `Sandbox::resolve`.** A path from the
   model is untrusted input in exactly the way user input is.
5. **Check `stop_reason` before reading `content`.** On a refusal the content is
   empty or a partial, never an answer.
6. **Do not add `temperature`, `top_p`, `top_k`, or `thinking.budget_tokens`.**
   They are 400s on current models. Depth is `output_config.effort`. The request
   struct omits them so the compiler enforces this.
7. **Anthropic-only fields go through `Dialect`.** Adding one means teaching
   `Dialect::adapt` to strip it, or compatible gateways start failing.
8. **Read a file in full before a wide-ranging change to it.** A snippet from
   search is not enough context to edit a loop invariant or a trait contract
   correctly.
9. **Ask before removing functionality that looks intentional.** If it's not
   obviously dead code, it might be a design decision recorded nowhere but
   the code itself.
10. **Answer a direct question before editing code in response to it.** State
    the answer, then make the change — don't let the diff be the only reply.
11. **The user's explicit instruction overrides every rule above.** These
    rules are defaults for the unstated case, not a ceiling on what's allowed.

## Git

Commit messages are scoped by crate: `{feat,fix,chore,docs}(llm|agent|tools|cli): message`,
or unscoped for changes spanning the whole workspace (`AGENTS.md`, `Cargo.toml`,
`Makefile`).
