# nahida

the agent you need

A small coding agent in Rust — and a readable account of what a coding agent
actually is. Four crates, one concept each: talk to a model, loop on tool calls,
touch the filesystem, render to a terminal.

## Quick start

```bash
nix develop

# pick one provider (see below), then:
make run ARGS="what does this repo do?"

# or an interactive session
make run
```

## Providers

nahida speaks two wire formats, behind one `Provider` trait: Anthropic
Messages and OpenAI Chat Completions. Most gateways implement the Anthropic
shape, which is also what several compatible endpoints implement — but they
do not all implement the same *features*, which is what `Dialect` is for: on
a compatible endpoint, Anthropic-only fields (`output_config.effort`,
adaptive `thinking`, `cache_control`) are stripped before the request goes
out rather than gambling on whether the gateway ignores or rejects them.

Choose a provider explicitly with `--provider anthropic`, `--provider zai`, or
`--provider zai-coding-cn`. Without the flag, the first nonempty credential wins:
`ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `ZAI_API_KEY`, then
`ZAI_CODING_CN_API_KEY`. A named provider never falls back to another key.
`--provider chatgpt` is reserved but unavailable: official sign-in and Responses
support are not implemented yet. `--describe` reports that limitation without
logging in or changing billing.

`--provider zai-coding-cn --describe` works even without a key. Description
inspects defaults, not a client or token store, and makes no network request.
Endpoint overrides are deliberately not printed because URLs can contain secrets.

### Anthropic

```bash
export ANTHROPIC_API_KEY=sk-ant-...
make run ARGS="..."                      # defaults to claude-opus-5
```

### Z.ai coding plan (GLM), global

```bash
export ZAI_API_KEY=...
make run ARGS="..."                      # defaults to glm-5.1
```

`glm-5.1` is the default because it is the model `refs/crush` exercises in its own
agent tests, so it is known to work behind an Anthropic-shaped tool loop.
`glm-5.2` is newer with a 1M context window; `glm-5` caps output near 20k tokens,
so pass `--max-tokens` if you use it.

### Z.ai coding plan (GLM), China region

```bash
export ZAI_CODING_CN_API_KEY=...
make run ARGS="..."                      # defaults to glm-5.3
```

A different domain from the global plan above (`open.bigmodel.cn`, not
`api.z.ai`) speaking a different wire format entirely — confirmed against
`refs/pi`'s own `zai-coding-cn` provider, which uses OpenAI Chat Completions
exclusively for this endpoint, not Anthropic Messages. `--effort`/`--thinking`
are silent no-ops here (OpenAI Chat Completions has no equivalent), the same
as any `Compat`-dialect Anthropic gateway.

### Any other compatible endpoint

```bash
export ANTHROPIC_AUTH_TOKEN=...
export ANTHROPIC_BASE_URL=https://your-gateway.example/anthropic
export NAHIDA_DIALECT=compat            # optional; inferred from the host
```

## Flags

```text
nahida [PROMPT...]
      --provider <NAME>    anthropic | zai | zai-coding-cn | chatgpt
                             chatgpt unavailable; omit for environment precedence
  -m, --model <ID>          override the provider's default model
  -e, --effort <LEVEL>      low | medium | high | xhigh | max (Anthropic only)
  -C, --root <DIR>          workspace root; tools cannot escape it  [default: .]
      --max-turns <N>       give up after N tool-calling turns      [default: 32]
      --max-tokens <N>      output cap per turn
      --compact-at <N>      summarize the transcript once a turn's prompt
                             reaches N tokens                        [default: off]
      --max-retries <N>     retries for a rate limit/server/transport
                             error, with backoff                     [default: 3]
      --retry-base-delay-ms <N>  base backoff delay; doubles per retry
                                  [default: 500]
      --no-overflow-recovery    disable the one-shot compact-and-retry
                             on a real context-overflow error
      --thinking            stream summarized reasoning
  -v, --verbose             turn boundaries, token usage, tool results
      --json                one JSON-encoded event per line to stdout, instead
                             of human-readable rendering
  -c, --continue            resume the most recent session for this --root
      --resume <ID>         resume a specific session by id (overrides -c)
      --no-session          don't read or write a session log for this run
      --describe            print the resolved provider, model, tools, approval
                             policy, caching, compaction, and session format,
                             then exit -- no API call, secret-free
```

Ctrl-C interrupts the current turn and keeps the session; twice quits.

## Sessions

Every run is logged to a JSONL file under `$XDG_DATA_HOME/nahida/sessions`
(falling back to `~/.local/share`) — a header line, then one line per
message, appended as the transcript grows. `-c`/`--continue` picks the most
recent session for the current `--root`; `--resume <ID>` picks one by id
(printed at startup with `--verbose`). `--no-session` skips logging
entirely. One writer, no branching — just the transcript, mirrored to disk.

## What it can do

Seven tools — `read`, `write`, `edit`, `bash`, `find`, `grep`, and `ls` —
which together let it look at a project, change it precisely or wholesale,
and check its own work. `edit` replaces exact text rather than the whole
file, and refuses instead of guessing if what it expected to find has
changed. `find` and `grep` walk the tree gitignore-aware, the way `git
status` would. Every path is confined to the workspace root.

`bash` runs with this process's privileges. In an interactive session it asks
first — every call needs a `y`/`N` before it runs — but that gate is
all-or-nothing (the harness only sees an opaque command string, so it can't
tell a `grep` from a `git push --force` yet) and only exists when stdin is a
terminal: a scripted or piped invocation runs ungated rather than hanging on a
prompt no one can answer.

On Linux, writes outside `--root` are also denied at the kernel level
(Landlock) — not just by the confirmation prompt — for this process and every
child `bash` spawns, applied once at startup with no way to lift it. Reads
stay unrestricted (see `nahida-tools/src/os_sandbox.rs` for why), and macOS
has no equivalent yet — `sandbox-exec` is deprecated with no real replacement
for headless sandboxing and is known to break `reqwest`'s macOS proxy
detection. Point `--root` at a repo you can afford to have edited regardless.

## Reading the code

Start at `crates/nahida-agent/src/agent.rs`. The loop is one function, and the
rest of the repo exists to serve it. `crates/nahida-llm/src/stream.rs` is the
other place worth reading closely — folding a stream of events back into one
message is where the non-obvious details are.

`AGENTS.md` has the crate contract and the concept map. `make tutorial` serves
a 0-to-hero walkthrough (`docs/`) that follows the concept map chapter by
chapter, using this code as the worked example.
