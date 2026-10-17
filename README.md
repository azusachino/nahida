# nahida

A terminal agent for writing, reading and science work. It is a TypeScript
project on Bun, built on pi's `pi-ai`, `pi-agent-core` and
`pi-coding-agent` (for pi's config loaders). Capabilities come from packs that
a profile composes. Today one harmless pack, `echo`, exercises the loader.

## Setup

```bash
mise install
make check
```

## Usage

Providers, models and credentials come from pi's agent dir, so one `/login` in
pi serves nahida too.

```bash
make run ARGS="what does this repo do?"
make run ARGS="--json what does this repo do?"       # one JSON event per line
make run ARGS="--profile pilot what does this repo do?"
make run ARGS="--describe --profile pilot"           # resolved config, no secrets
```

The agent dir is `PI_CODING_AGENT_DIR`, falling back to `~/.pi/agent`. nahida
reads `auth.json` (stored and OAuth credentials), `models.json` (custom
providers) and `settings.json` (`defaultProvider`, `defaultModel`) through pi's
own loaders. Project-level `.pi/settings.json` is ignored.

Model resolution: the profile's `model`, then `settings.json` defaults. Without
a credential for the chosen provider, `nahida` exits 1 before any network call
and names the provider.

## Profiles and packs

A profile is `~/.config/nahida/profiles/<name>.toml` (set `NAHIDA_CONFIG_DIR`
to move the config root):

```toml
packs = ["echo"]                        # ordered built-in pack ids (required)
model = "zai-coding-cn/glm-5.3-flash"   # optional, provider/id
approval = "ask"                        # optional, shown by --describe; not enforced yet
```

With no `default.toml`, `default` loads every built-in pack. An unknown pack id,
an unknown key, or two packs registering the same tool name exits 2 at boot.

A pack is a module with `id`, optional `tools`, optional `prompt` (`{ file,
text }`, the text imported from a `.md` file) and optional `activate(ctx)`,
which returns a disposer. Every disposer runs on normal exit and on SIGINT, in
reverse activation order.

## Layout

```text
src/cli.ts              entry point (bin): flags, run, --describe
src/pack.ts             pack contract, selection, tool collision check, disposal
src/profile.ts          profile TOML loading (Bun.TOML)
src/pi-config.ts        pi agent dir, ModelRuntime and SettingsManager, model refs
src/packs/              built-in packs; echo is the only one so far
src/prompt.md           the base system prompt
src/cli.test.ts         tests against a fixture agent dir and pi's faux provider
docs/                   decisions and the current spec
```

## Make targets

| Target | What it does |
| --- | --- |
| `make check` | Typecheck, Biome lint and format check, `bun test` |
| `make validate` | `check`, then a `bun build` of the CLI |
| `make run ARGS="..."` | Run the agent |
