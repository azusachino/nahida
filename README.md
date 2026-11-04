# nahida

A terminal agent for writing, reading and science work. It is a TypeScript
project on Bun, built on pi's `pi-ai`, `pi-agent-core` and
`pi-coding-agent` (for configuration and session storage). Capabilities come
from packs that a profile composes. Writing tools work on Markdown drafts;
reading tools extract PDFs and articles into anchored notes. The harmless echo
pack exercises the loader.

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
make run ARGS="--profile writing draft an outline in drafts/outline.md"
make run ARGS="--profile reading read paper.pdf and save a note in notes/paper.md"
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
discarded when the run ends. Failed and incomplete responses are recorded as
failure metadata, never replayed as answers or executable tool calls. Ctrl-C exits 130 after tool settlement and
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

With no `default.toml`, `default` loads every built-in pack (echo, writing and
reading). Built-in `writing` and `reading` profiles each select only that pack.
A file profile overrides a built-in profile. An unknown pack id,
an unknown key, or two packs registering the same tool name exits 2 at boot.

Sessions from before the writing pack was added used an echo-only default. To
continue one, create an `echo-only.toml` profile with `packs = ["echo"]` and run
`--continue --profile echo-only`. Other model and approval settings must still
match that session. Sessions from the later echo/writing default can similarly
use a file profile with `packs = ["echo", "writing"]` after reading is added.

A pack is a module with `id`, optional `tools`, optional `prompt` (`{ file,
text }`, the text imported from a `.md` file) and optional `activate(ctx)`,
which returns a disposer. Every disposer runs on normal exit and on SIGINT, in
reverse activation order.

## Writing drafts

The writing pack provides three tools:

| Tool | Behavior |
| --- | --- |
| `read_draft(path)` | Read a UTF-8 Markdown file. |
| `create_draft(path, text)` | Create a new draft and missing parent directories; refuse overwrite. |
| `edit_draft(path, oldText, newText)` | Replace exactly one literal occurrence; refuse empty, missing or ambiguous targets. |

Paths may be relative or absolute but must stay inside the current workspace
and end in `.md` or `.markdown`. Symlinks, hard-linked files, non-regular files
and invalid UTF-8 are refused. Edits preserve surrounding text, line endings
and any UTF-8 byte order mark. Failures return tool errors so the model can
correct its request.

These tools run with the user's filesystem permissions. The path checks assume
other processes do not replace ancestor directories while a tool is running;
they do not provide an OS sandbox or rollback after disk errors or crashes.
The profile's approval field remains descriptive; writing tools execute
without an interactive approval prompt. Use a workspace containing drafts you
intend the agent to edit.

See [the slice 4 record](docs/plans/slice4-writing.md) for acceptance and limits.

## Reading and notes

Use `--profile reading` for these tools:

| Tool | Behavior |
| --- | --- |
| `read_pdf(path)` | Extract a workspace PDF into `page:1`, `page:2`, … passages. |
| `read_article(url)` | Extract a public HTTPS HTML article into `section:1`, `section:2`, … passages. |
| `record_note(path, source, anchor, highlight?, note?)` | Append an anchored highlight and/or note to workspace Markdown. |

Extraction returns the source, content SHA-256 revision and anchored text.
`record_note` requires a source and anchor extracted in the current run; copy
those fields from the extraction result. Highlights must match the passage
exactly. Notes include source, revision, anchor, quote and your commentary.
Re-extract a source after restarting before recording more notes. Earlier
extraction results remain in session history.

PDFs are limited to 10 MiB and 200 pages, and extracted text to 200,000
characters. PDF parsing uses the pinned `unpdf`/PDF.js build with external font
and character-map loading disabled. These limits do not provide an isolated
parser or a strict CPU deadline for malformed PDFs. EPUB and OCR are deferred;
scanned PDFs without extractable text return an error.

Articles must be credential-free public HTTPS on port 443. All resolved
addresses must be public; the request connects to a vetted IP while retaining
the original TLS hostname. Redirects, compressed responses and non-HTML content
are refused. The request has a 20-second deadline and a 2 MiB body limit; HTML
must be UTF-8. Scripts and subresources are never loaded. Extraction removes
common page chrome and hidden elements but does not run a browser or implement
full CSS visibility or article-quality scoring.

Local PDFs and notes use the same workspace, symlink and hard-link protections
as drafts. Notes append without replacing prior text and have the same
filesystem concurrency and crash limits. Remote PDFs, authenticated sites and
JavaScript-rendered articles are outside this slice. See
[the slice 5 record](docs/plans/slice5-reading.md) for evidence and deferred work.

## Layout

```text
src/cli.ts              entry point (bin): flags, run, --describe
src/pack.ts             pack contract, selection, tool collision check, disposal
src/profile.ts          profile TOML loading (Bun.TOML)
src/session.ts          pi session storage, strict resume checks and workspace lock
src/pi-config.ts        pi agent dir, ModelRuntime and SettingsManager, model refs
src/packs/              built-in echo, writing and reading packs
src/workspace.ts        shared path and file-descriptor protections
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
