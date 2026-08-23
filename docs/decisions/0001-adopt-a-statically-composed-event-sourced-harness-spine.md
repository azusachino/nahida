# ADR-0001: Adopt a statically composed, event-sourced harness spine

## Status

Accepted

## Date

2026-08-24

## Context

Nahida deliberately keeps the provider, agent loop, tools, and terminal surface
in four small crates. That static shape is still appropriate: the process has one
surface, tools are wired at startup, and no current requirement needs plugins to
appear or disappear while it runs.

Two established harnesses solve adjacent problems at larger scale:

- Pi separates live tool progress from the ordered messages persisted for the
  model. In parallel mode it prepares calls sequentially, executes allowed calls
  concurrently, emits completion events as each call finishes, then emits result
  messages in the assistant's original source order. It also stores session
  entries as a tree whose current position is a movable leaf. See Pi's
  [tool-execution contract](https://github.com/earendil-works/pi/blob/c49906ec77788625aacbdc53ebca6fbe65bd20f5/packages/agent/src/types.ts#L33-L41)
  and
  [session types](https://github.com/earendil-works/pi/blob/c49906ec77788625aacbdc53ebca6fbe65bd20f5/packages/agent/src/harness/session/types.ts#L14-L19).
- DeepSeek Harness makes an append-only session event log the durable source of
  model context, replay, transcripts, and forks. It distinguishes durable session
  events from live agent and capability events, while its resolved profile can be
  inspected before boot. See its
  [architecture](https://github.com/deepseek-ai/deepseek-harness/blob/b150a551b8d465e31e418e1b2eaf5e79bbb7d28e/docs/architecture.md#events).

Nahida already exposes `AgentEvent`, runs tools concurrently, and keeps the model
transcript in assistant source order. Its live `ToolResult` events do not appear
until every call in a parallel batch finishes, however, because `dispatch` awaits
the whole `join_all` before emitting any result. Session persistence exists on
the separate `feat/pi-alignment` branch as a linear JSONL transcript, while
prompt caching exists on `feat/prompt-caching`; neither branch is part of `main`
at the time of this decision.

The design needs a direction that lets those pieces converge without importing
Pi's remote-session stack or DeepSeek Harness's reactive plugin framework.

## Decision

Nahida will use a statically composed, event-sourced harness spine:

1. Keep the existing compile-time crate composition. Runtime dependency
   injection, hot-swappable plugins, and a remote session protocol are not part
   of this decision.
2. Keep `AgentEvent` for ephemeral observation of work in flight. Add typed,
   versioned `SessionEntry` records for facts that must survive restart, such as
   messages, compaction, and configuration changes that affect replay.
3. Once durable sessions ship, make the active model transcript a projection of
   the selected session entries. Anything model-visible must be reconstructable
   from those entries; live rendering details need not be persisted.
4. Structure parallel tool dispatch as three phases: prepare calls in source
   order, execute permitted calls concurrently and emit completion events as
   each finishes, then assemble the durable/model-facing results in source order.
5. Start with append-only JSONL persistence. Add stable entry IDs and parent
   pointers only with a user-facing revert or branch operation; do not build a
   session tree before that operation exists.
6. Add a secret-free harness description command before reproducibility depends
   on implicit CLI state. It should report the effective model and dialect,
   prompts, tools, sandbox and approval policy, caching, compaction, and session
   format without introducing a general profile system.
7. If runtime extensions later become real, each registration must own a
   disposer. Static startup wiring remains the default until at least two actual
   dynamic extension use cases justify a lifecycle abstraction.

## Alternatives considered

### Keep only an in-memory transcript and CLI-specific persistence

This is the smallest current design, but it leaves compaction and future
configuration changes indistinguishable from ordinary messages and makes replay
semantics depend on whichever surface wrote the file. It remains acceptable as
an implementation waypoint, not as the long-term session contract.

### Adopt Pi's complete package and remote-session architecture

Pi's provider matrix, client/server protocol, transport adapters, and shared or
exclusive session leases solve multi-provider embedding and remote attachment.
Nahida has neither requirement. Taking those packages would add compatibility
surfaces without adding a current capability.

### Adopt DeepSeek Harness's reactive plugin runtime

Reactive dependency injection provides reversible hot-swapping across many
profiles and surfaces. Nahida wires a few capabilities once per CLI process, so
the framework and lifecycle complexity would dominate the agent itself.

### Introduce a generic plugin API now

There is no second surface or dynamic extension lifecycle to validate such an
API. Typed events and ordinary Rust construction already expose the seams needed
for the planned work.

## Consequences

- Tool progress can remain responsive without changing the ordering required by
  the model transcript.
- Durable session entries become a compatibility surface and therefore require a
  format version, fixture-based replay tests, and an explicit migration or
  rejection policy.
- The flat JSONL implementation on `feat/pi-alignment` is useful input but is not
  automatically the final session model.
- Prompt caching must operate on the transcript projection rather than mutate
  durable history solely for provider-specific cache placement.
- A future branchable session can add parent pointers without replacing the
  append-only storage medium, but it is deliberately deferred until a CLI
  operation proves the need.
- Nahida keeps its current small crate graph. A new crate, protocol, plugin host,
  or profile language requires a separate decision backed by a concrete
  consumer.
