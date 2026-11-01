# Spec: nahida TypeScript rework

Implements [ADR-0003](../decisions/0003-rebuild-nahida-as-a-pi-backed-deep-work-harness.md).

## Outcome

`nahida` is a terminal agent for writing, reading and science work. It is
built on `pi-ai` and `pi-agent-core`. Its capabilities come from packs that a
profile composes.

## Assumptions (proposals, not owner decisions)

- Runtime is **Bun** with TypeScript, matching luna. All tools (bun, uv,
  python, linters) are pinned in a root `.mise.toml`, and `make` stays the task
  runner. There is no Nix devShell: `flake.nix` and `flake.lock` go away with
  the Rust tree. The old `docs/mise.toml` is folded into the root file or
  removed with the tutorial.
- Profiles live in `~/.config/nahida/profiles/<name>.toml`. A built-in
  `default` profile loads all three packs.
- Pack work files live in one confined workspace directory per session,
  defaulting to the current directory, and every model-supplied path is
  resolved inside it.
- pi package versions are pinned exactly. pi follows upstream releases that
  pass the owner's 3-day install cooldown (currently 1.2.0, as of 2026-10-17);
  this is not a deliberate hold on an older version. The project `bunfig.toml`
  enforces that cooldown. APIs are verified against the installed
  package's own docs, not `refs/coding-agents/pi` (stale at 0.87.1).

## Acceptance criteria

Each criterion names its evidence. Wherever the agent is involved, "test" means
an automated test against a fake or faux provider with no network.

**Core and migration**

1. The commit tagged `rust-final` (cd51c2b) builds with `nix develop -c make
   validate`, exit 0 (run 2026-10-17). Evidence: the gate run on the tag.
2. `main` contains no `Cargo.toml`, `crates/`, `flake.nix`, `flake.lock`, or
   Rust tutorial pages. From a clean clone, `mise install && make check` is the
   whole setup, and `make check` runs TypeScript typecheck, lint and tests. Evidence: tree
   listing and gate output.
3. `nahida "<prompt>"` streams an answer from the configured provider. Without
   credentials it exits non-zero with a message that names the missing
   variable. Evidence: test for the error path, manual run for the happy path.
4. `--json` emits one JSON event per line, and every line parses with `jq`.
   Evidence: test.

**Packs and profiles**

5. A pack is a module that exports `{ id, tools?, prompt?, commands?,
   activate(ctx) → dispose }`. Loading a profile with two packs that register
   the same tool name fails at boot and names both packs. Evidence: test.
6. `--profile writing` exposes only the writing pack's tools to the model.
   Evidence: test asserting the tool list in the recorded request.
7. `--describe` prints the resolved profile: model id, packs in order, tool
   names, prompt fragment sources, approval policy, session dir. It contains
   no API key, even when one is set in the environment. Evidence: test that
   sets a sentinel key and greps the output for it.
7a. Providers, models and credentials come from pi's existing configuration,
    not nahida-specific env vars. The agent dir is `PI_CODING_AGENT_DIR`,
    falling back to `~/.pi/agent`. nahida reads `auth.json` (including OAuth
    credentials such as `openai-codex`) and `models.json` (custom providers
    such as `zai-coding-cn`) through pi's own registry code, so file
    locking and OAuth refresh behave exactly as they do for pi and luna. A
    profile's `model = "<provider>/<id>"` wins over the defaults; otherwise
    `settings.json`'s `defaultProvider`/`defaultModel` apply. Evidence: a test
    with a fixture agent dir (API-key provider plus custom models.json)
    showing the resolved model and that `--describe` names the provider but
    not the key.
8. Every pack's disposer runs on normal exit and on SIGINT. Evidence: test with
   a pack that records disposal.

**Sessions**

9. `--continue` resumes the last session in that workspace. The resumed
   model request equals what replaying the session log produces. Evidence:
   test comparing recorded request bodies before and after restart.
10. Context a pack adds (for example, an extracted PDF passage) appears in the
    session log before the request that uses it. Evidence: test.
10a. Failures and durability, carried over from issue #19 (A5, A6):
    - A failed or partial stream never executes tool calls and never ends as a
      successful answer.
    - Ctrl-C during a request or tool call leaves the log consistent: a later
      `--continue` neither replays a partial tool call nor repeats its side
      effects.
    - A session log with a corrupt tail is refused, with a message, rather than
      appended to.
    - A session records its provider/model, and resuming under a different
      model is either refused or explicitly re-projected, never silent.

    Evidence: tests with scripted faux-provider failures and a truncated log
    fixture.

**Packs**

11. **writing**: tools to read, create and edit Markdown drafts in the
    workspace. Edits are targeted replacements that fail when the target text
    is missing or ambiguous, and a path outside the workspace returns a tool
    error, not an exception. Evidence: tests for missing, ambiguous and escaped
    paths.
12. **reading**: tools that turn a local PDF/EPUB or a URL into text with
    location anchors (page or section), and that record a highlight or note
    with its anchor to a Markdown file in the workspace. Evidence: tests on a
    fixture PDF and fixture HTML. EPUB can be deferred if the slice runs long,
    but it must be listed as deferred.
13. **science**: a paper-search tool over at least one open API (arXiv or
    Semantic Scholar) that returns title, authors, year, id and abstract, plus
    a `compute` tool that runs a Python snippet via `uv run` in the workspace
    with a timeout and requires approval. The subprocess runs without the pi
    agent dir in reach: a scrubbed environment and a cwd confined to the
    workspace. If full read isolation is not achievable on macOS, that is
    documented as a limitation. Evidence: test with a recorded API
    fixture, test that a denied approval executes nothing, and test that the
    timeout kills a looping script.

## Out of scope

Web or Telegram surfaces, hot reload, remote sessions, a pack registry or
installer, sharing packs with luna, and writing into apricot.

## Slices

Each slice ends green on `make check` and is one PR.

1. Tag `rust-final` (done: cd51c2b). Remove the Rust tree and flake, add `.mise.toml`,
   scaffold the Bun/TS project, wire the pi-ai and pi-agent-core CLI, and add
   `--json`. Covers criteria 1–4.
2. Pack and profile loader, pi config reuse, `--describe`, disposal. Covers
   5–8, including 7a.
3. Sessions on the pi-agent-core store, plus `--continue`. Covers 9–10a.
4. writing pack. Covers 11.
5. reading pack. Covers 12.
6. science pack. Covers 13.
7. Rewrite `AGENTS.md` and README, and remove the tutorial. Re-check 2.
