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
make run ARGS="--continue tell me more"             # append to the last session
make run ARGS="--continue"                          # retry unfinished input
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

## Sessions

Each run starts a new session unless `--continue` is given. Nahida uses pi's
`SessionManager` JSONL store under `~/.config/nahida/sessions/`, or
`$NAHIDA_CONFIG_DIR/sessions/`, grouped by a hash of the canonical working
directory. `--describe` prints the exact directory without creating a session.
These logs are separate from pi's own sessions.

`--continue "<prompt>"` appends to the most recently modified session in that
workspace. Without a prompt, `--continue` retries unfinished user input or
requests the response to already recorded tool results. A completed answer
requires a new prompt. The provider/model, ordered packs, prompts, tool
schemas and approval policy must match the original session.

Requests are rebuilt from the stored messages. Pack activation can call
`ctx.addContext(text)` to queue model-visible context, which is recorded after
pending tool results and before the next request. Unused queued context is
discarded when the run ends. Failed
and incomplete responses are recorded as failure metadata, never replayed as
answers or executable tool calls. Ctrl-C exits 130 after tool settlement and
pack disposal; recorded tool results are reused on continuation.

A corrupt or truncated log is refused without modification. A crash leaving a
tool request without its results also blocks continuation: inspect the workspace
for side effects and start a new session. Nahida cannot infer whether an
unrecorded side effect completed. A workspace lock prevents concurrent writers;
a hard crash can leave a `.lock` containing the process ID in the session
directory. Confirm that process is gone and inspect the log and side effects
before manually removing it. Persistence follows pi's synchronous file writes;
there is no extra guarantee against power loss.

See [the slice 3 record](docs/plans/slice3-sessions.md) for scope and verification.

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
src/session.ts          pi session storage, strict resume checks and workspace lock
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
