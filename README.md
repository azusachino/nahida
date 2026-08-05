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

nahida speaks the Anthropic Messages API. That format is also what several
gateways implement, so more than one provider works — but they do not implement
the same *features*, which is what `Dialect` is for: on a compatible endpoint,
Anthropic-only fields (`output_config.effort`, adaptive `thinking`,
`cache_control`) are stripped before the request goes out rather than gambling on
whether the gateway ignores or rejects them.

**Anthropic**

```bash
export ANTHROPIC_API_KEY=sk-ant-...
make run ARGS="..."                      # defaults to claude-opus-5
```

**Z.ai coding plan (GLM)**

```bash
export ZAI_API_KEY=...
make run ARGS="..."                      # defaults to glm-5.1
```

`glm-5.1` is the default because it is the model `refs/crush` exercises in its own
agent tests, so it is known to work behind an Anthropic-shaped tool loop.
`glm-5.2` is newer with a 1M context window; `glm-5` caps output near 20k tokens,
so pass `--max-tokens` if you use it.

The Z.ai base URL is a best guess (`https://api.z.ai/api/anthropic`) — crush pulls
provider metadata from a remote registry, so there is nothing checked in to
confirm it against. If requests 404, take the URL from your Z.ai dashboard and
override it with `ANTHROPIC_BASE_URL`.

**Any other compatible endpoint**

```bash
export ANTHROPIC_AUTH_TOKEN=...
export ANTHROPIC_BASE_URL=https://your-gateway.example/anthropic
export NAHIDA_DIALECT=compat            # optional; inferred from the host
```

## Flags

```
nahida [PROMPT...]
  -m, --model <ID>          override the provider's default model
  -e, --effort <LEVEL>      low | medium | high | xhigh | max (Anthropic only)
  -C, --root <DIR>          workspace root; tools cannot escape it  [default: .]
      --max-turns <N>       give up after N tool-calling turns      [default: 32]
      --max-tokens <N>      output cap per turn
      --thinking            stream summarized reasoning
  -v, --verbose             turn boundaries, token usage, tool results
```

Ctrl-C interrupts the current turn and keeps the session; twice quits.

## What it can do

Three tools — `read`, `write`, and `bash` — which together let it look at a
project, change it, and check its own work. Every path is confined to the
workspace root.

`bash` runs with this process's privileges and nothing asks you first. That is the
honest starting point rather than a claim to be safe: point `--root` at a repo you
can afford to have edited, and see `AGENTS.md` for where permission gating is
going to live.

## Reading the code

Start at `crates/nahida-agent/src/agent.rs`. The loop is one function, and the
rest of the repo exists to serve it. `crates/nahida-llm/src/stream.rs` is the
other place worth reading closely — folding a stream of events back into one
message is where the non-obvious details are.

`AGENTS.md` has the crate contract and the concept map.
