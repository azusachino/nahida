# ADR-0003: Rebuild nahida as a pi-backed, pack-composed deep-work harness

## Status

Accepted. Supersedes [ADR-0001](0001-adopt-a-statically-composed-event-sourced-harness-spine.md) and [ADR-0002](0002-bind-replay-and-separate-storage-from-tool-permissions.md).

## Date

2026-10-17

## Context

ADR-0001 kept nahida a four-crate Rust coding agent and rejected both pi's full
package stack and DeepSeek Harness's plugin runtime, mainly because nahida had
one surface, one scenario (coding), and no consumer for a plugin system.

That premise has changed:

- The target is no longer a coding agent. nahida is to become a harness for
  long-form personal work: **writing**, **reading** (papers, books, web
  articles), and **science** (literature search, computation, notes with
  sources). Each scenario has a different tool set and different instructions, which
  gives the composition layer at least three concrete consumers.
- The Rust implementation re-derives what pi already provides. pi's TypeScript
  packages are open, embeddable and broader than nahida's Rust port.
  `@earendil-works/pi-ai` covers providers. `@earendil-works/pi-agent-core`
  1.3.0 covers the loop, tool execution and an event-sourced `AgentHarness`.
  The owner prefers the TypeScript ecosystem for this project.
- luna already runs a quick Telegram secretary on pi. nahida must not duplicate
  it.

The Rust line did not stop at ADR-0001. PRs #20–#24 continued it with a GLM
provider, a `nahida-config` crate, and Landlock storage probes. The
`rust-final` tag marks that last Rust state at cd51c2b. The pi auth research in
[pi layers and auth](../research/2026-10-10-pi-layers-and-auth.md) motivates
calling pi's own `AuthStorage` (which locks with `proper-lockfile`) instead of
re-implementing it.

DeepSeek Harness shows the composition shape worth borrowing. A run is a
**profile** made of ordered **bundles**. "Model-visible means logged": the
session log is the source of model context. New behavior attaches to a
documented seam rather than to the loop. Its Cordis runtime (everything is a
hot-reloadable plugin, reversible effects, HMR) solves multi-surface product
problems nahida does not have. See its
[architecture](https://github.com/deepseek-ai/deepseek-harness/blob/639ed015/docs/architecture.md).

## Decision

1. **Language and base.** nahida becomes a TypeScript project. Models and
   providers come from `pi-ai`. The loop, tool execution and session
   persistence come from `pi-agent-core`. nahida does not reimplement anything
   these packages export. The Rust workspace is tagged `rust-final` and removed
   from `main`. The Rust tutorial retires with it.
2. **Packs are the plugin unit.** A pack is a TypeScript module that
   contributes tools, a system-prompt fragment, and optionally slash commands
   and settings. It returns a disposer for anything it registers. Writing,
   reading and science are each a pack. Core ships no domain tools.
3. **Profiles compose packs.** A profile is a named, ordered list of packs plus
   model and policy settings, read from a file at boot. `nahida --profile
   <name>` selects one. `nahida --describe` prints the resolved profile without
   secrets: model, packs, tools, prompt sources, approval policy, session
   location. Packs load once at boot. Hot reload is out of scope.
4. **Model-visible means logged.** Anything that reaches the model is
   reconstructable from the session log, using pi-agent-core's session store.
   A pack that adds model context does so through a logged entry, never by
   mutating the request in place.
5. **First surface is a terminal CLI.** It has an interactive REPL plus
   `--json` event output. Other surfaces (web, Telegram via luna) are later
   consumers of the same harness, not part of this decision.
6. **Boundary with luna.** nahida owns deep, long-running work sessions. luna
   stays the quick-capture Telegram secretary. Sharing packs with luna is
   allowed later but not designed now.

## Alternatives considered

### Keep Rust and grow `nahida-llm` toward pi-ai

This keeps the learning artifact but spends the effort on provider plumbing
instead of on the scenarios. The owner rejected it.

### Build on `pi-coding-agent`'s SDK and extension system

This is how luna works and needs the least code. But the harness shape
(sessions, extension contract, defaults) stays pi's coding-agent design, and
coding tools have to be stripped back out. nahida wants to own its composition
layer.

### Adopt Cordis or pi's `chord` facet runtime

Both solve multi-process, hot-swappable composition. `chord` is still marked
experimental, and nahida has one process and one surface. A plain pack
interface with disposers can later move onto either one without rewriting the
packs' tools.

## Consequences

- The crate-boundary contract and the Rust rules in `AGENTS.md` are replaced.
  `AGENTS.md` is rewritten in the same change that removes the Rust tree.
- Tooling moves from the Nix devShell to mise, following the workstation
  default of mise per project. The flake retires with the Rust tree.
- nahida's correctness now depends on pi-agent-core's session format and API.
  Versions are pinned exactly, and upgrades are deliberate.
- pi writes `auth.json` without an atomic rename, and tools that run shell or
  code can read the agent dir. Later packs must not hand the model unrestricted
  reads of it.
- The pack interface becomes the contract that matters. It stays as small as
  the three first packs need. A field no pack uses is not added.
- The 0-to-hero tutorial no longer describes the code. It is removed rather
  than left stale, and remains readable at `rust-final`.
