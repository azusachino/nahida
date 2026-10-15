# Pi's real layers, provider abstractions and auth

Date: 2026-10-10. Source-backed research, not an implementation decision.

## Scope and evidence

Question: what are Pi's actual package boundaries and provider/auth contracts,
and what could Nahida reuse or learn from them?

- Nahida: `spike/replay-storage-boundary` at
  `321129ddb19add34e31de8d753ee92d5e92333ef` (clean when inspected).
- Pi source: release tag `v1.1.0`, checked against the installed
  `@earendil-works/pi-coding-agent` 1.1.0 docs/package metadata. Source links
  below are pinned to that tag. This is a versioned source walkthrough, not live
  provider testing.
- No credentials were inspected or used; no login, network inference, or
  provider API call was made.
- The workstation's `refs/pi` checkout is absent, so this research uses Pi's
  official v1.1.0 source and the installed 1.1.0 documentation rather than
  claiming a local reference checkout.

## Short answer

Pi's useful lesson is a sequence of explicit seams, not “add layers” as an end
in itself:

```text
provider API / OAuth and credential resolution
       ↓
pi-ai: models, provider catalog, wire APIs, streaming, request auth
       ↓
pi-agent-core: generic agent loop and tool execution
       ↓
pi-coding-agent: sessions, tools, resources, CLI/TUI/RPC and SDK composition
```

The code reflects these boundaries: `pi-agent-core` depends on `pi-ai`; the
coding-agent package depends on both and composes them with application
concerns. [P1–P4]

Pi supports Z.AI Coding Plan CN via `ZAI_CODING_CN_API_KEY`. It supports ChatGPT
subscription sign-in in the current `openai` provider. The older separate
`openai-codex` provider is explicitly labeled legacy and superseded by Sign in
with ChatGPT on `openai`. [P5–P8]

Pi is TypeScript/Node, while Nahida is Rust. Nahida cannot import Pi's npm
packages as native Rust crates. Reuse of Pi's existing configuration/auth
requires either a process/API boundary to a Pi runtime or an explicit
shared-storage integration; copying the model/provider/auth interfaces into Rust
is learning from Pi, not reusing its implementation.

## Pi package boundaries

### `@earendil-works/pi-ai`: model/provider and protocol layer

`pi-ai` describes a provider as the runtime unit that owns its model catalog,
auth behavior, and streaming behavior. A `Models` collection registers
providers, finds models, resolves auth, and dispatches requests. Providers reuse
protocol/API implementations (for example Anthropic Messages, OpenAI Responses
and Chat Completions) instead of each inventing their own stream parser. [P1]

The package exposes a deliberately small-provider entrypoint as well as explicit
provider factories and an all-providers entrypoint. This makes provider
selection and dependency weight visible to the embedding application. `Model`
metadata is typed and models identify their API implementation; provider
registration is not equivalent to the agent loop. [P1, P2]

### `@earendil-works/pi-agent-core`: generic loop

The agent package builds on `pi-ai`. Its `Agent` is given initial state, a
stream function, tools, and hooks; the loop emits lifecycle/message/tool events
and handles continuation. Its README distinguishes flexible agent messages from
the LLM's normalized messages via an explicit conversion boundary. [P3]

This is the closest conceptual counterpart to Nahida's `nahida-agent`. It should
not be confused with the full coding agent: it does not itself imply Pi's
filesystem tools, CLI, sessions, or terminal UI.

### `@earendil-works/pi-coding-agent`: application composition

The coding-agent package adds application services: session management, settings
and model runtime, resource discovery, tools, extensions, CLI/TUI/RPC, and SDK
session factories. The SDK can inject a `ModelRuntime`, `SessionManager`,
settings/resource loaders and tools independently. The CLI/RPC/SDK are
alternative interfaces over the same agent/session mechanisms. [P4]

Pi's runtime architecture is evidence that more packages can be appropriate when
each has a concrete owned concept and a useful independent consumer. It is not
evidence that Nahida needs to reproduce all Pi packages or their feature set.

## Provider and model interfaces worth learning

Pi keeps at least three concepts distinct:

1. **Model metadata/catalog:** identity, API kind, capabilities and limits.
2. **Provider:** model discovery, provider-specific auth, and dispatch to
   streaming behavior.
3. **API implementation:** wire-format request/response conversion and stream
   normalization.

A static compatible endpoint can be described through config; custom auth, model
discovery or protocol behavior warrants a provider implementation. A provider
can still reuse an existing API implementation if the wire format matches. Pi
documents compatibility flags as evidence-based behavior, not as a substitute
for testing. [P2, P6]

For Nahida, the transferable design question is whether to represent model
metadata and API protocol separately from credential resolution and provider
selection. Current Nahida `nahida-llm` already has provider
definitions/registry, Anthropic Messages and OpenAI Chat Completions adapters;
its project contract keeps protocol in `nahida-llm`, the loop in `nahida-agent`,
tools in `nahida-tools`, and terminal/persistence in `nahida-cli`. This existing
contract is already analogous to the Pi seams. See `AGENTS.md` and
`docs/pages/01-the-provider-layer.md` in the pinned Nahida tree.

An additional crate is justified only if it owns a real independent boundary
(for example, auth storage/refresh lifecycle) and callers can depend on it
without creating an upward dependency. Do not make `nahida-agent` know Pi, OAuth
wire formats, filesystem paths, or Herdr transports merely to mirror Pi's
package count.

## Auth support

### Pi's auth abstraction

`pi-ai`'s `ProviderAuth` separates API-key auth from OAuth auth. OAuth has
`login`, `refresh`, and `toAuth`; request authentication is normalized to fields
such as API key, headers or base URL. An app supplies `CredentialStore`. Its
contract is one tagged credential per provider, with `read`, metadata-only
`list`, serialized `modify`, and `delete`. [P7, P8]

The store contract explicitly places refresh inside serialized modification so
concurrent requests cannot race a rotated refresh token; cross-process exclusion
depends on the backing store. The default `InMemoryCredentialStore` only
serializes within that process and is not durable. Pi's coding-agent layer
supplies its own persistent auth storage/runtime path. Thus “Pi supports auth”
includes more than OAuth protocol code: it also depends on the host app's
persistence, locking, lifecycle and UI interaction implementation. [P8, P9]

Pi's user-facing docs say credentials live in the agent directory's `auth.json`;
the SDK permits selecting an `agentDir`/runtime `authPath`. Secrets may also
come from environment variables or configured API-key sources. Sharing that
store is a deliberate cross-runtime storage contract, not just selecting the
same model name. [P4, P10]

### GLM Coding Plan CN

The Pi v1.1.0 provider documentation lists `ZAI_CODING_CN_API_KEY`; the provider
implementation is a provider factory backed by that auth and an API
implementation. This supports reusing the same _API-key configuration
convention_ if Nahida adopts it. It does not prove that Nahida may use the
subscription: the existing Nahida plan records provider eligibility as a
separate check before live use. [P5, P11]

### ChatGPT subscription OAuth

Pi v1.1.0's `openai` provider supports both OpenAI API-key auth and OAuth
labeled “Sign in with ChatGPT.” It uses the OpenAI Responses API. By contrast,
the separate `openai-codex` provider is explicitly named “legacy” and uses
ChatGPT subscription auth with its own backend endpoint. Pi's AI package docs
say that legacy flow is superseded by Sign in with ChatGPT on the OpenAI
provider. If Nahida studies Pi for current ChatGPT behavior, study the `openai`
provider and its OAuth loader/API, not just older `openai-codex` code. [P5, P6,
P12]

This describes Pi's implementation. It is not a statement that OpenAI guarantees
this unofficial client flow for third-party implementations, nor is a successful
Pi login evidence that Nahida is eligible to use the same subscription. Provider
terms and owner approval remain separate.

## Review correction: file storage and native Rust

A situation review checked the installed Pi 1.1.0 implementation, not real
credential files. The following distinctions correct the later discussion:

- `InMemoryCredentialStore` is only in-process. The coding-agent's file-backed
  `AuthStorage` instead uses `FileAuthStorageBackend` and `proper-lockfile` 4.1.2
  for cross-process exclusion. Its `modify` reloads the file while locked,
  invokes the mutation, merges the provider entry and writes before releasing
  the lock. Concurrent Pi processes can therefore coordinate. [P13, P15]
- OAuth resolution rechecks expiry under that lock, skips refresh if another
  caller already refreshed or logged out, and holds the lock through refresh
  and persistence. Once refresh starts, caller cancellation does not discard a
  potentially rotated token; the refresh is timeout-bounded. [P14]
- A Rust implementation can participate in this storage protocol. A Rust-only
  mutex or `flock` would not coordinate with Pi's mkdir-based `auth.json.lock`
  protocol. Compatibility tests need path normalization, heartbeat/stale-lock
  behavior, compromised-lock handling, locked reload/merge, refresh and logout
  races, and preservation of unrelated provider data. This is work to verify,
  not evidence that a Node runtime bridge is necessary. [P13–P15]
- Locking is not crash-safe atomic replacement: Pi's backend uses
  `writeFileSync`, not a temporary-file-and-rename transaction. Nahida's stronger
  storage acceptance is not proved just by matching Pi's locking. [P13]
- The file is `models.json`, not `model.json`. It configures compatible
  endpoints and overlays model metadata; it is not Pi's entire built-in catalog
  or its executable provider extensions. Native support must name supported
  APIs/fields and reject unsupported behavior rather than claim full Pi parity.
- Nahida's existing Landlock rules permit reads everywhere and deny outside-root
  writes. They do not hide `auth.json` from approved bash reads. A bridge would
  not by itself fix that either. ADR-0002's startup, storage and IPC proof remains
  distinct from JSON compatibility; do not quietly promise secret-read isolation.

The owner requested native Rust config/auth compatibility, Rust refresh and
T02-first sequencing. The Node bridge proposal was rejected. The later
reconsideration followed the false in-process-only claim; no bridge approval
follows from it. Implementation is paused for review. No cross-language runtime
probe or Linux safety proof was performed in this source review.

## What this means for Nahida

- **Reuse models/configuration:** Pi model names, provider catalog and env
  conventions can inform Nahida's provider profiles. Reusing
  `ZAI_CODING_CN_API_KEY` is straightforward configuration compatibility. It
  does not require coupling the agent loop to Pi.
- **Reuse Pi's ChatGPT auth directly:** this cannot happen by Rust trait reuse.
  Options for later evaluation are (a) a narrow bridge to a running Pi
  process/runtime that owns its credential handling, or (b) a Nahida-owned Rust
  implementation with a separately approved, protected credential lifecycle.
  Reading Pi's `auth.json` directly would couple to its storage schema and put
  refresh-token handling under the unresolved Nahida Linux storage boundary; do
  not assume a file format is a supported public auth API.
- **Keep model/provider work below the loop:** the natural Nahida extension seam
  is inside `nahida-llm` or a lower, separately-owned crate if the interface is
  proven useful. The loop should continue to depend on neutral provider/stream
  contracts, not Pi concepts.
- **Keep Herdr separate:** Herdr API/process interaction is an outer
  transport/host integration, not a model-provider or auth layer. It should
  consume an agent/session interface and event stream, not move provider secrets
  into Herdr or the core loop. This research did not inspect Herdr's current API
  contract.
- **Do not solve auth before the existing safety gate:** Nahida's ADR-0002
  records that Linux storage/process isolation is not yet proven and T02 remains
  the next task. Pi's existence does not resolve that blocker or authorize a
  production helper, login, live inference, or widening tool permissions.

## Suggested next research/design slice

Before choosing new crates or implementing auth, draw one concrete flow and
compare alternatives:

1. Nahida selects a named model/provider from a local profile.
2. The provider layer resolves API-key/OAuth credentials without leaking them
   into model-visible tools/events.
3. It sends the provider-specific request and returns normalized stream events
   plus provider-owned replay metadata.
4. The session layer persists only the approved, owner-bound replay state.
5. Herdr, if selected, attaches outside the agent loop and consumes
   events/accepts input through a documented interface.

Evaluate a Pi runtime bridge against a Rust-owned adapter on interface
stability, auth-store ownership/locking, session replay compatibility, Linux
boundary, offline operation, and whether the change still teaches Nahida's loop
rather than replacing it. Treat this as a proposal to investigate, not an
authorization to start production auth work.

## Sources

All Pi source URLs are pinned to release tag `v1.1.0`:

- [P1 `packages/ai/README.md`](https://github.com/earendil-works/pi/blob/v1.1.0/packages/ai/README.md)
  — provider/model/API concepts, auth, model collections.
- [P2 `packages/ai/src/models.ts`](https://github.com/earendil-works/pi/blob/v1.1.0/packages/ai/src/models.ts)
  — provider/model definitions and APIs.
- [P3 `packages/agent/README.md`](https://github.com/earendil-works/pi/blob/v1.1.0/packages/agent/README.md)
  — generic loop, tool execution and events.
- [P4 `packages/coding-agent/docs/sdk.md`](https://github.com/earendil-works/pi/blob/v1.1.0/packages/coding-agent/docs/sdk.md)
  and
  [`package.json`](https://github.com/earendil-works/pi/blob/v1.1.0/packages/coding-agent/package.json)
  — runtime composition, SDK, dependencies/layer.
- [P5 `packages/ai/src/providers/openai.ts`](https://github.com/earendil-works/pi/blob/v1.1.0/packages/ai/src/providers/openai.ts)
  — OpenAI API key + ChatGPT OAuth on Responses.
- [P6 `packages/ai/src/providers/openai-codex.ts`](https://github.com/earendil-works/pi/blob/v1.1.0/packages/ai/src/providers/openai-codex.ts)
  — legacy Codex provider; plus Pi AI README's OAuth provider notes.
- [P7 `packages/ai/src/auth/types.ts`](https://github.com/earendil-works/pi/blob/v1.1.0/packages/ai/src/auth/types.ts)
  — `ProviderAuth`, `OAuthAuth`, credential/store contracts.
- [P8 `packages/ai/src/auth/credential-store.ts`](https://github.com/earendil-works/pi/blob/v1.1.0/packages/ai/src/auth/credential-store.ts)
  — in-memory implementation and in-process serialization.
- [P9 `packages/coding-agent/src/core/model-registry.ts`](https://github.com/earendil-works/pi/blob/v1.1.0/packages/coding-agent/src/core/model-registry.ts)
  — coding-agent facade over `ModelRuntime`.
- [P10 Pi v1.1.0 docs: Providers](https://github.com/earendil-works/pi/blob/v1.1.0/packages/coding-agent/docs/providers.md),
  [Configuration](https://github.com/earendil-works/pi/blob/v1.1.0/packages/coding-agent/docs/configuration.md),
  [Models](https://github.com/earendil-works/pi/blob/v1.1.0/packages/coding-agent/docs/models.md)
  — auth/config behavior and credential precedence.
- [P11 `packages/ai/src/providers/zai-coding-cn.ts`](https://github.com/earendil-works/pi/blob/v1.1.0/packages/ai/src/providers/zai-coding-cn.ts)
  and Pi providers docs — Z.AI CN provider/env support.
- [P12 `packages/ai/src/auth/oauth/openai-chatgpt.ts`](https://github.com/earendil-works/pi/blob/v1.1.0/packages/ai/src/auth/oauth/openai-chatgpt.ts)
  (login/refresh implementation) and `packages/ai/README.md` (legacy provider
  supersession note).
- [P13 `packages/coding-agent/src/core/auth-storage.ts`](https://github.com/earendil-works/pi/blob/v1.1.0/packages/coding-agent/src/core/auth-storage.ts)
  — checked against installed 1.1.0 `dist/core/auth-storage.js`: file locks,
  locked modification, reload and direct file writes.
- [P14 `packages/ai/src/auth/resolve.ts`](https://github.com/earendil-works/pi/blob/v1.1.0/packages/ai/src/auth/resolve.ts)
  — checked against installed `pi-ai/dist/auth/resolve.js`: double-checked
  refresh, logout handling and cancellation/persistence behavior.
- [P15 `proper-lockfile` v4.1.2](https://github.com/moxystudio/node-proper-lockfile/blob/v4.1.2/lib/lockfile.js)
  — installed 4.1.2 source: atomic lock-directory creation, mtime heartbeat,
  stale detection and compromised-lock handling.
- Nahida: `AGENTS.md`, `docs/pages/00-why-nahida.md`,
  `docs/pages/01-the-provider-layer.md`,
  `docs/decisions/0002-bind-replay-and-separate-storage-from-tool-permissions.md`,
  and `docs/plans/2026-10-08-two-provider-support.md` at the pin above.
