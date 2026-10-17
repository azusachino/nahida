# nahida

A terminal agent for writing, reading and science work. It is a TypeScript
project on Bun, built on `@earendil-works/pi-ai` (providers) and
`@earendil-works/pi-agent-core` (loop, tools, events). Decision record:
[ADR-0002](docs/decisions/0002-rebuild-nahida-as-a-pi-backed-deep-work-harness.md).
Current slice and acceptance criteria: [docs/specs/ts-rework.md](docs/specs/ts-rework.md).

## Layout

| Path | What it is |
| --- | --- |
| `src/cli.ts` | The `nahida` entry point: flags, profile and model selection, the agent run, `--json`, `--describe` |
| `src/pack.ts` | The pack contract, pack selection, the tool-name collision check, disposal |
| `src/profile.ts` | Profile TOML loading (`Bun.TOML`) |
| `src/pi-config.ts` | pi's agent dir, `ModelRuntime` and `SettingsManager` from `pi-coding-agent` |
| `src/packs/` | Built-in packs. `echo` only, to exercise the loader |
| `src/cli.test.ts` | CLI tests against the faux provider (no network) |
| `src/prompt.md` | The system prompt, beside the code that loads it |
| `docs/decisions/`, `docs/specs/` | Decisions and the slice spec |

Sessions, the REPL and the writing, reading and science packs come in later
slices. Do not add them ahead of the spec.

## Rules for agents

1. **Do not reimplement pi.** Providers, the loop, tool execution and session
   persistence come from `pi-ai` and `pi-agent-core`. Read the installed
   packages' own docs under `node_modules/@earendil-works/*` (README and
   `dist/*.d.ts`). Do not rely on memory or on `refs/`.
2. **Pin pi exactly.** `@earendil-works/pi-ai`, `pi-agent-core` and
   `pi-coding-agent` have no `^` or `~`. Upgrades are deliberate, with the
   spec's criteria re-checked.
3. **A tool failure is a result, never an exception.** Return an error result the
   model can act on. Throwing ends the run.
4. **Every model-supplied path resolves inside the workspace.** A path from the
   model is untrusted input in the same way user input is.
5. **Check the stop reason before reading content.** On an error or aborted
   response the content is empty or partial, never an answer.
6. **Do not set `temperature`, `top_p` or `top_k`.** They are rejected on current
   models. Leave sampling to the provider defaults.
7. **Prompt text lives in `.md` files beside the code** that loads it.
8. **Tests run against a fake provider.** A test that needs the network is not a
   unit test. Every test that touches the model uses the faux provider or an
   injected `streamFn`. Tests point `PI_CODING_AGENT_DIR` and
   `NAHIDA_CONFIG_DIR` at a fixture under `.tmp/`, never at a real agent dir.
9. **Ask before removing functionality that looks intentional.**

## Commands

```bash
mise install        # pins bun from .mise.toml
make check          # typecheck + biome lint/format + bun test, before every commit
make validate       # check + bun build of the CLI, before a PR
make run ARGS="..." # run the agent
```

## Git

Commit messages are scoped: `{feat,fix,chore,docs}(core|cli|packs): message`,
or unscoped for changes that span the repo (`AGENTS.md`, `package.json`,
`Makefile`).
