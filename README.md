# nahida

A terminal agent for writing, reading and science work. It is a TypeScript
project on Bun, built on pi's `pi-ai` and `pi-agent-core` packages. Its
capabilities will come from packs that a profile composes. The first slice is
the CLI wiring: one prompt in, a streamed answer out.

## Setup

```bash
mise install
make check
```

## Usage

```bash
export ANTHROPIC_API_KEY=sk-ant-...
make run ARGS="what does this repo do?"
make run ARGS="--json what does this repo do?"   # one JSON event per line
```

The provider and model come from the environment:

| Variable | Default | Meaning |
| --- | --- | --- |
| `NAHIDA_PROVIDER` | `anthropic` | `anthropic` or `zai` |
| `NAHIDA_MODEL` | `claude-opus-5-5` (anthropic) or `glm-5.2` (zai) | Any model id in the pi catalog for that provider |
| `ANTHROPIC_API_KEY` | | Required for `anthropic` |
| `ZAI_API_KEY` | | Required for `zai` |

Without the key for the selected provider, `nahida` exits non-zero and names
the variable on stderr. It does this before any network call.

## Layout

```text
src/cli.ts         entry point (bin)
src/cli.test.ts    tests against pi's faux provider
src/prompt.md      system prompt
docs/              decisions and the current spec
```

## Make targets

| Target | What it does |
| --- | --- |
| `make check` | Typecheck, Biome lint and format check, `bun test` |
| `make validate` | `check`, then a `bun build` of the CLI |
| `make run ARGS="..."` | Run the agent |
