# Slice 4: workspace Markdown drafts

Status: locally verified; ready for PR review.

## Checkpoint and scope

The owner requested "merge it and next" after slice 3. PR #27 was merged at
`6073ef0a53077e4850cbad0e964067ed1cc2175b`; the session implementation is
`78d8bfa9608c757535c9876fd98edff72e598dd1`. This slice starts from that merge
on `feat/writing-pack`, with one writer, this session. Workstation-task
playbook, tier 1. [Spec criterion 11](../specs/ts-rework.md) owns acceptance.

Own the writing pack, built-in registry/CLI workspace binding, fixture tests,
README and current-slice instruction/spec updates. No new dependencies, live
inference, installation or release. `make check` before commit; `make validate`
before PR. Verification is offline tests and self-review.

## Contract

- `read_draft(path)` returns a UTF-8 Markdown draft.
- `create_draft(path, text)` creates a new draft, including missing parent
  directories, and refuses to overwrite an existing path.
- `edit_draft(path, oldText, newText)` replaces exactly one literal occurrence.
  An empty, missing or ambiguous target returns a tool error and changes nothing.
- All three accept `.md` and `.markdown` files inside the host-selected workspace.
  The built-in registry binds each run's canonical workspace into fresh tool
  closures, rather than storing mutable global workspace state.
- All filesystem failures return `isError: true`, never escape from execute.
  Mark tools sequential to keep a batch of edits ordered.
- The built-in `writing` profile exposes only these tools; a `writing.toml` file
  overrides it. The built-in default continues to load every built-in pack, now
  echo and writing. An echo-only profile preserves the configuration needed to
  continue sessions from before writing was added; document and test this path.

## Path boundary and limits

Model-supplied paths are untrusted. The trusted boundary is the workspace chosen
by the host; outside drafts and credentials are assets the tools must not reach.
Resolve paths below the canonical root, reject escapes and the root itself, and
reject symlinks in every model-selected path component. Existing files must be
regular UTF-8 files with one link, so a workspace hard link cannot alias an outside
file. Preserve byte order marks and reject invalid UTF-8 before editing. Open
existing files with no-follow and inspect the descriptor before read
or edit. Creation uses exclusive creation, so it cannot overwrite an existing
file or follow a final symlink. Targeted edits read ownership evidence (the exact
old text) before writing.

These are local filesystem tools, not an OS sandbox. Another process capable of
replacing ancestor directories while a tool runs is outside this slice's threat
model; Node's portable filesystem API does not provide directory-relative,
beneath-root resolution. Avoid concurrent external mutations of the workspace.
No promise of transactional rollback after disk errors, process crash or power
loss. Recheck pi's installed tool types in `pi-agent-core/dist/types.d.ts`;
`AgentToolResult.isError` supports recoverable failures directly.

## Acceptance and handoff

Tests must verify exact draft bytes, non-overwrite creation, unique literal edits,
missing/ambiguous/empty targets, traversal and absolute escapes, symlink files
and parents, hard links and non-regular files. A faux-provider CLI journey must
create, read and edit a real fixture draft, see errors as tool results, and
continue the session with the writing-only tool declarations.

Criterion 11 is implemented. The writing-only faux-provider journey creates a
real fixture draft, reads its content, receives a missing-target error, corrects
the edit and resumes the resulting session. Unit fixtures cover overlapping
matches as ambiguity, UTF-8 byte order marks and invalid encoding, as well as
the boundary cases above. An echo-only compatibility fixture confirms sessions
created before this pack was added can still continue with matching settings.

Verification:

- Focused writing/boundary tests: 14 passed before the final encoding and
  profile-compatibility cases were added.
- Final `make validate`: passed TypeScript, Biome, all 56 offline tests
  (305 assertions), CLI bundling and the built executable's usage/exit-code
  smoke check.
- `git diff --check`: passed.

The initial typecheck rejected tool callbacks contextually typed through the
heterogeneous pack array. Declaring each tool against its own schema, matching
the echo tool's existing pattern, resolved the error without suppressions.
Subsequent typecheck and full gates passed. Verification is self-review and
fixture tests, not independent agent review or live inference. Tests never
opened real credential/config files or made network model requests. Dependency
versions and user installations are unchanged.

Changed paths: `src/packs/writing/{index.ts,index.test.ts,prompt.md}`,
`src/packs/index.ts`, `src/cli.ts`, `src/cli.test.ts`, `src/profile.ts`,
`README.md`, `AGENTS.md`, `docs/specs/ts-rework.md` and this record. Workstation
root unchanged. The implementation branch is `feat/writing-pack`, based on the
full merged checkpoint above; the owning PR records the delivered full commit.
No implementation blocker remains. Owner PR review is pending.

Next after acceptance: slice 5's reading pack, criterion 12. Its PDF/EPUB/URL
extraction and anchored notes are separate work; EPUB may be explicitly deferred
under the spec. Keep external-fetch policy and any new dependencies with that
slice. No reading/science implementation, merge of this slice, release or
installation is included here.

