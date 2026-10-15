# Plan: GLM Coding CN and official ChatGPT support

Date: 2026-10-08. Status: local implementation authorized; live work gated.

## Outcome and scope

Make Nahida usable with explicitly selected GLM Coding CN and ChatGPT plan
providers, while retaining its four-crate learning-project shape. The owner
accepted the [research direction](../research/2026-10-08-glm-cn-and-chatgpt.md),
then authorized local implementation after plan review. Owning acceptance:
[issue #19](https://github.com/azusachino/nahida/issues/19).
Login, paid inference, eligibility exceptions and releases remain separately gated.

Baseline: Nahida `main` at `2767cd18069e59c5314d1bf17397a4f0ba8f74f2`.
Pi learning reference: `1b347794e2a630e4359f2584f4eea388145d0ddf`.
The research owns source comparisons and citations; official provider contracts
must be rechecked before their implementation slices. Do not use a moving
reference checkout as pinned evidence.

Live work and dependencies: Asobi `nahida:two-provider-support`. This document
owns acceptance and sequencing, not duplicated live status. No `tasks/todo.md`.
Future contract-changing implementation is tier 2: after baseline gates pass,
create an owning acceptance issue before changing CLI/session/auth contracts,
and use the intent-to-verified-result playbook. Issue #19 now provides that prior
acceptance. Planning was tier 1 under the workstation-task playbook.
Do not open a PR or claim DONE with failed gates.

### Proposed defaults

These are reviewable plan choices, not claims that they are implemented:

- `--provider zai-coding-cn` and `--provider chatgpt` select the requested paths.
  Preserve the existing auto/environment behavior when the flag is omitted;
  existing Anthropic/global GLM support is not removed. Saved ChatGPT auth does
  not silently change old invocations or switch billing.
- Retain `glm-5.3` as the CN default; support `glm-5.3-flash` explicitly. Propose
  `high` as the default reasoning effort for these models. Reject unsupported
  efforts with the supported list; never silently raise medium to high.
  The owner's high-effort selection for research peers does not itself set
  Nahida's application default.
- ChatGPT uses only the official OSS/local SIWC OAuth and public Responses API.
  No legacy Codex client ID/backend, OpenAI API-key billing fallback, WebSocket
  transport, plugins, remote protocol, or TypeScript sidecar.
- Initial ChatGPT profile UX is terminal-based: list/add/select/reauthorize/logout
  registrations. A small local account store suffices; no generic profile DSL.
  Commands below are proposed CLI spellings.
- ChatGPT model choices come from the active account's current visible catalog.
  `--model` overrides are validated against it; absent an override, select the
  first visible model in server order and report it. Empty catalogs fail clearly.
- ChatGPT rejects an explicit `--max-tokens`: this subscription route does not
  accept an output cap. Report that capability honestly, including compaction;
  never send a forbidden field or pretend a prompt instruction is a hard cap.

## Boundaries and acceptance map

| Capability | Owner | Required evidence |
| --- | --- | --- |
| Selection and effective capabilities | `nahida-llm`; CLI flags/rendering in `nahida-cli` | Mixed-environment subprocess tests; offline `--describe` |
| GLM request/stream/replay | `nahida-llm` | Recorded HTTP bodies and fragmented SSE fixtures |
| ChatGPT OAuth, JWT validation, Responses and model discovery | `nahida-llm` | Fake auth/JWKS/model/SSE endpoints, no account credentials |
| Protected storage and terminal account workflow | `nahida-cli`, using llm-owned protocol interfaces | Temporary-store/restart/race tests, redacted outputs |
| Provider-neutral turns, settled tools, compaction, cancellation | `nahida-agent` | Fake-provider regression tests |
| Kernel tool confinement | `nahida-tools` | Isolated Linux subprocess tests, not a shared test process |
| Durable messages, replay and compaction entries | `nahida-cli/session.rs` | Versioned JSONL fixture projections before/after restart |

No upward crate dependency is permitted. Provider/API details cannot become
branches in the agent loop. Credentials, tokens and account email are not model
context or tool-visible session metadata. Thinking display is separate from
reasoning capture/replay.

Acceptance IDs:

- **A1 Selection:** chosen provider wins over unrelated env credentials; old
  auto selection still works; `--describe` is secret-free and network/refresh-free.
- **A2 GLM:** enabled reasoning with supported effort; exact reasoning replay;
  streamed parallel calls; complete IDs/arguments/results; usage not double-counted.
- **A3 ChatGPT auth:** official registration and returning-login semantics;
  validated identity/grants; protected rotating credentials and explicit logout;
  independent saved registrations; no silent billing fallback.
- **A4 ChatGPT inference:** account-visible model discovery; subscription-safe
  Responses bodies; text and local tools complete; opaque reasoning/item/namespace
  replay survives subsequent requests and restarts.
- **A5 Failures:** no partial/failed stream causes tool execution or a successful
  answer; quota failures pause; temporary failures retry boundedly; cancellation
  can interrupt pending network/refresh/backoff rather than waiting for a delta.
- **A6 Durability:** version compatibility enforced; settled history after resume
  equals the active pre-exit projection, including compaction and provider binding.
- **A7 Safety/verification:** Linux write confinement is not silently weakened;
  live tests require approval and GLM eligibility evidence; owning gates and fresh
  independent verification pass before completion.

## Prerequisite stop points

1. **Baseline gates:** earlier `make check` exits 2 under Clippy 1.99
   (`double_must_use` from `async_trait`, three `assert_is_empty` findings).
   Tutorial build also failed on untrusted `docs/mise.toml`. T01 resolved these
   without suppressions: async-trait 0.1.92 and equivalent SSE assertions in
   [PR #20](https://github.com/azusachino/nahida/pull/20). Fresh independent review
   and Linux CI passed; local check, validate and strict tutorial build exited 0.
   The owner approved scoped dependency downloads and trust for this config only.
   After the Nix devShell failed fetching bmake (HTTP404), the owner selected
   installed stable Rust 1.99 as the gate environment. Nix itself is not repaired.
2. **GLM eligibility:** current BigModel docs restrict Coding Plan to approved
   tools; Nahida is not listed. Record provider/owner-supplied permission evidence
   before live use or advertising support. Offline adapter work can proceed after
   implementation approval. An OpenAI-only delivery is not completion of A2/A7.
3. **Linux auth/storage:** the CLI applies Landlock before provider/session setup;
   credentials and session logs normally live outside the workspace. Atomic
   refresh writes cannot be assumed to work under that confinement. Test actual
   thread/process inheritance as well as paths before settling a design. Never
   add the credential directory to permissions inherited by model-invoked bash.
   The owner selected existing Nahida Linux CI for isolated verification before
   Linux-dependent slices. No host setup or cluster deployment is authorized.
4. **Session/replay contract:** the current types are Anthropic-shaped and
   session versions are not enforced. Decide the smallest compatible metadata
   and version policy before adding native reasoning replay. This is a narrow
   implementation of ADR-0001, not a requirement for branches or a session tree.

## Ordered slices

Each slice should fit one focused session. Paths below are indicative scope,
not permission to edit unrelated hunks. If implementation discovers more than
about five files or two independent behaviors, split the slice before coding.
A task's Asobi numeric suffix is its index below plus two: T01 = task-3.
Local implementation is now authorized; dependencies and separate approval gates
still apply. Only plan author/review were authorized during planning verification.

### T01 — Restore the baseline (S/M; no dependencies; A7)

Paths: Makefile/toolchain declarations only if needed, failing Rust locations,
relevant dependency pin only if the macro/toolchain diagnosis warrants it.

- Establish the declared Rust environment and record the actual tool versions.
  Resolve errors without lint allowances, test removal or an unexplained downgrade.
- Obtain needed Mise trust/setup approval, then make both owning gates green.
- Verify: `make check`, `make tutorial-build` inside Nahida; then `make validate`.
  Record commands/exits and unchanged versus intentionally changed lockfiles.

**Checkpoint 0:** create the acceptance issue with A1–A7; get approval for
implementation and the choices in T02. No contract-changing code before this.

### T02 — Settle risky boundaries early (S/M spike; after T01; A3/A6/A7)

Paths: a proposed ADR in `docs/decisions/`, isolated tests/scratch, no live auth.

- Define typed, provider/model-tagged replay data and the exact session-version
  transition. Native/opaque data is only replayed to its owning provider/model;
  incompatible provider/model resume is rejected initially, not converted on guess.
- Prove a Linux credential/session-write boundary with fake stores. Preferred
  candidate: a narrowly scoped local storage/auth helper started before tool
  confinement, accessible only to the trusted runtime, never a model tool. It
  must not accept arbitrary paths/commands. Include lifetime, bounded IPC,
  refresh locking and cleanup. Do not assume a thread-only helper is safe.
- Define tests demonstrating confined shell writes still fail and concurrent
  refresh/storage succeeds. If a safe small design cannot be proved, stop for an
  owner decision on a narrower supported-OS scope; do not silently disable Landlock.

Verify: isolated Linux subprocess spike on an assigned Linux runner, fake-only;
review the proposed ADR/security boundary before helper or format implementation.
Missing Linux access blocks the Linux criterion, not source-only/macOS work.
This slice may return a bounded design blocker instead of speculative code.

### T03 — Select an existing provider explicitly (S; after T01; A1)

Paths: `nahida-llm/src/provider.rs`, CLI `main.rs`, selection tests.

- Add explicit selection of existing providers while preserving omission behavior;
  unrelated Anthropic variables cannot override an explicit CN choice.
- Reserve `chatgpt` with an actionable unavailable/not-signed-in result until its
  adapter is wired. Separate metadata inspection from credential resolution.
- Verify: fake/mixed env subprocess tests and offline `--describe` with missing
  credentials; no token-store reads or HTTP from description.

Focused commands: `cargo test -p nahida-llm`; `cargo test -p nahida-cli`.

### T04 — Preserve provider replay through the loop (M; after T02; A2/A4/A6)

Paths: llm `types.rs`, `stream.rs`, agent `agent.rs`, one regression fixture/module.

- Add only the typed replay/provenance fields agreed in T02; public text/tool
  behavior remains canonical. Anthropic payloads must not acquire foreign fields.
- Change assistant-message construction so native replay is retained rather than
  dropped by `response.replayable()` → `Message::assistant(...)`.
- Verify same-owner serialization round trips and foreign-owner rejection with a
  fake provider; cache annotations only affect outgoing request copies.

Focused commands: `cargo test -p nahida-llm`; `cargo test -p nahida-agent`.

### T05 — Persist compatible sessions and compaction (M; after T04; A6)

Paths: CLI `session.rs`, `main.rs`, agent compaction event boundary if needed,
versioned JSONL fixtures/tests.

- Introduce the minimal typed versioned entries agreed in T02: provider/model
  binding, messages and authoritative compaction replacement; projection defines
  active context. Unknown versions fail before inference.
- Preserve legacy v1 logs: explicit import into a new v2 log, never append new-format
  entries to v1. Missing replay provenance is not invented; don't promise recovery
  of history v1 compaction already lost. Old file stays untouched.
- Replace index-only tail persistence in `run_once`: shrinking the transcript
  must neither panic at `transcript[session_start..]` nor leave disk with
  old uncompacted history. Torn-tail handling must not append after corrupt bytes.

Verify: `cargo test -p nahida-cli`; `cargo test -p nahida-agent` with
append/resume, import/reject, compaction and post-compaction failures. Include
proposed fixture `torn_tail_refuses_append`: a truncated final JSONL entry must
prevent appending until explicit safe recovery/import, preserving the old file.

### T06 — Complete GLM CN behavior (M; after T03/T05; A2/A5)

Paths: llm `openai.rs`, provider capabilities, HTTP/SSE tests; CLI effort tests.
Split request/reasoning and stream hardening into separate commits if necessary.

- Send supported effort and enabled/preserved thinking, capture exact
  `reasoning_content`, replay it with tool calls/results, and enable `tool_stream`.
  Unsupported effort errors precede HTTP; hidden thinking is still retained.
- Accumulate fragmented IDs/names/arguments for interleaved parallel calls; validate
  finished JSON and terminal reason before returning dispatchable calls. Normalize
  usage/cache counts through the shared accumulator, not only the decoder.
  Recheck the official CN cache-field path before wiring it. Proposed fixtures:
  `cache_missing_is_zero` (100 prompt tokens → 100 uncached, zero cached) and
  `cache_total_not_double_counted` (100 total, 30 cached → 70 uncached, 30 cached;
  canonical prompt total remains 100). If CN exposes no documented cache field,
  record that limit and retain zero cache counts; do not invent provider data.
- Verify prompt → reasoning/two tools → both results → final answer, cache usage,
  missing terminal, malformed arguments, late metadata, compaction (which currently
  omits effort/thinking), and unchanged Anthropic behavior.

Focused commands: `cargo test -p nahida-llm`; `cargo test -p nahida-agent`;
`cargo test -p nahida-cli`. No real Coding Plan request here.

**Checkpoint 1:** A1/A2/A6 offline; independent review, `make check`, `make validate`.
GLM can be demonstrated live only after the separate eligibility/owner gates.

### T07 — Protect renewable ChatGPT credentials (M; after T02; A3/A7)

Paths: CLI auth storage module/helper, llm auth interface, credential-store tests;
manifest changes only for reviewed crypto/locking dependencies.

- Implement the approved bounded storage route for host ID and per-registration
  client/identity/token sets, outside coding-session logs. Unix directories/files
  are owner-only; writes are atomic, symlink substitution/unsafe modes fail closed.
- Serialize refresh and credential replacement across processes; recheck expiry
  after taking the lock. Never hold mixed new-access/old-refresh state. Logout/login
  races cannot resurrect revoked tokens. Temporary network failure preserves state.
- Test process races, restart, write failure, revoked/rotated credentials and
  helper cleanup using fake tokens. Report the existing limitation that unrestricted
  shell reads do not provide secret isolation; storage permissions are not an
  agent-secret sandbox.

Verify: `cargo test -p nahida-cli`; `cargo test -p nahida-llm`; isolated Linux
confinement tests on the approved runner. This slice uses fake refresh only.

### T08 — Implement official login and account lifecycle (M; after T07; A3)

Paths: llm ChatGPT OAuth module, CLI auth commands, fake OAuth/JWKS fixtures/tests.
If JWT validation needs its own session, split it before wiring login.

- `nahida auth login chatgpt` implements first registration and saved-client
  reauthorization with PKCE, state/nonce, issued-client checks, verified ID-token
  signature/issuer/audience/expiry, and independently checked granted scopes.
  Use vetted JOSE code; discovery/JWKS remain pinned to the trusted issuer.
- Provide list/add/select/status/logout of distinct registrations. Keep the pending
  login separate from the active profile; denied inference scope yields a signed-in
  but inference-disabled account. Logout revokes when possible, clears tokens and
  retains registration/host identity; report unconfirmed remote revocation.
- Test wrong state (including denial), mismatched client/subject, bad JWT/nonce,
  callback-port collisions, cancellation, absent scopes, same-email registrations,
  token expiry/refresh, reauthorization and logout. Never log hinted/token URLs.

Focused commands: `cargo test -p nahida-llm`; `cargo test -p nahida-cli`.
No real browser/account login until owner approval at T13.

### T09 — Run authenticated Responses text (M; after T03/T05/T08; A1/A3/A4)

Paths: llm Responses adapter/model discovery, provider wiring, CLI model selection,
fake Responses/model tests. Split discovery from integration if file scope grows.

- Model discovery uses the active account's OAuth credential and visible server
  catalog, not static Codex IDs. Account switches invalidate cached choices; offline
  `--describe` reports unknown/cached metadata without making HTTP or reading tokens.
- Encode full-history Responses input with safe instructions, `store:false`,
  `stream:true`, explicit subscription auth mode and an allowlisted request shape.
  Reject forbidden output-cap overrides; no backend endpoint or API-key fallback.
- Fake CLI login → model discovery → streamed text → `response.completed` → resume
  works. Failed/incomplete/EOF never yields a successful answer. Usage and
  request IDs are available without secret diagnostic dumps.

Focused commands: `cargo test -p nahida-llm`; `cargo test -p nahida-cli`.

**Checkpoint 2:** renewable auth plus authenticated text works entirely against
fake services; independent security/source review and `make check`/`make validate`.
No claims of real account eligibility or inference yet.

### T10 — Complete Responses local tool round trips (M; after T09; A4/A5)

Paths: Responses encoder/decoder, shared replay types only if necessary,
agent fake-provider integration tests.

- Use one stable namespace for Nahida's existing function tools (subject to the
  rechecked official contract); preserve namespace, call IDs, provider item IDs
  and opaque encrypted reasoning across requests and persisted messages.
- Decode complete, valid arguments keyed by output item/index; do not dispatch on
  deltas or `output_item.done` alone before whole-response settlement. Keep all
  results in canonical source order, regardless of parallel completion order.
- Verify reasoning → two interleaved calls → results → answer → restart replay;
  malformed/incomplete calls and terminal failure execute no tools.

Focused commands: `cargo test -p nahida-llm`; `cargo test -p nahida-agent`;
`cargo test -p nahida-cli`.

### T11 — Classify errors and interrupt pending work (M; after T06/T10; A5)

Paths: llm error mapping, agent `agent.rs`/`cancel.rs`, regression tests.

- Classify subscription quota, consent, revocation, temporary unavailability,
  transport and real overflow separately, including errors after SSE starts.
  Quota does not retry or trigger OAuth loops; 503 retries boundedly without erasing
  credentials. Preserve structured status/code/param/request ID, redact secrets.
- Wake cancellation during request setup, stalled SSE, refresh and backoff with
  a provider-neutral signal; no dependency from llm to agent's `Cancel`.
  Every settled dispatched call receives exactly one result; never replay
  partial assistant calls or repeat already executed side effects on retry.
- Verify with stalled fake endpoints: cancel returns within two seconds.
  Temporary failures stay within attempt limits; quota/unsupported requests
  do not retry. Partial failures do not advance the durable model context.

Focused commands: `cargo test -p nahida-llm`; `cargo test -p nahida-agent`;
`cargo test -p nahida-cli`.

### T12 — Verify complete saved-session journeys (M; after T11; A1–A7 offline)

Paths: CLI/agent integration tests, README/tutorial/AGENTS updates.
Split documentation from acceptance tests if scope exceeds a session.

- For both adapters, fake text/tool/failure/cancel/compaction → exit → resume produces
  the same selected provider/model and settled transcript projection. Incompatible
  owner/model resume fails clearly; account selection never mixes credential sets.
- Exercise token expiry across successive turns and compaction with no tools, stale
  catalogs, v1 import, unknown versions, corrupt tail and persistence failures.
  Never print credential/replay secrets in `--json`, errors or description.
- Document the real behavior, capabilities and limitations, including output caps,
  supported GLM effort, sign-in/usage controls, privacy and platform support.
  Preserve existing providers and established tool/loop invariants.

Verify: `make check`; `make tutorial-build`; `make validate`, plus isolated Linux
confinement/refresh/session-write tests. Fresh independent verifier checks A1–A7
against acceptance issue and fixed source/runtime inputs.

### T13 — Verify approved live journeys and close (S/M; after T12; A2/A3/A4/A7 live)

Paths: owning issue/PR evidence; disposable workspace, no user project edits.

- Obtain explicit owner approval for ChatGPT login and small quota-consuming calls;
  GLM also requires recorded Coding Plan eligibility. Owner authenticates; agents
  do not read existing Pi/Codex credentials or collect tokens in reports.
- In a disposable workspace, separately verify each provider: text, read-only tool
  call, two safe parallel tools, one confined file edit, resume, and renewable auth
  where practical. Bound turns/calls up front; record usage, not transcripts/secrets.
- Record observed success and limits, independent review, commit/revision and owning
  gates on the issue/PR. Never mark both providers supported if a provider or
  Linux criterion remains blocked. Release/deployment are outside this plan.

## Dependencies and dispatch

```text
T01 → T02 → T04 → T05 ─┬→ T06 ───────────────┐
  └→ T03 ─────────────┘                    │
        T02 → T07 → T08 → T09 → T10 ───────┤→ T11 → T12 → T13
                   T03 + T05 → T09        │
```

T03 selection, T02 boundary investigation and later protocol reading can be
explored independently. Implementation in this single checkout is serialized:
one writer, no competing changes to types/provider/CLI files. Checkpoints occur
after prerequisites, GLM/offline durability, ChatGPT auth/text, and whole-journey
acceptance. Gates belong to each slice's owning changes; do not defer correctness
until T12 just because it has the broadest suite.

## Verification and approval boundaries

Commands above run inside `vendor/nahida/`; root gates do not verify this project.
For each commit run `make check`; before a PR run `make validate`; documentation
changes also run `make tutorial-build`. Focused Cargo commands are iteration
checks, not substitutes. Record exact source, toolchain, fixtures, OS and exit
codes; avoid automatically installing/trusting tools during diagnosis.

GLM and Responses tests need real local HTTP/SSE fixtures in addition to decoder
unit tests. Existing agent support is Anthropic-shaped; extend or add small
protocol-specific fixtures rather than pretending they already exist. Split SSE
inside UTF-8/JSON/frame boundaries and deliberately break representative replay,
terminal and refresh-lock behavior to show the regression tests catch it.

A fresh peer independently verifies each checkpoint before DONE or leaving draft.
Use Pi `zai-coding-cn/glm-5.3-flash` high, the owner's selection, unless changed.
A source walk-through does not replace gates or the actual changed journey.
No credentials in Asobi, logs, issues, sessions or test fixtures.

Planning retained failed-gate evidence rather than overriding tools to prepare
Markdown. T01 subsequently restored the gates with explicit owner setup choices,
independent review and Linux CI in PR #20; this does not prove future provider
acceptance. Approval now covers local implementation, not login, paid calls,
provider policy exceptions, broader trust changes, release, deployment or
unrelated remote writes.

## Risks and contingency

| Risk | Response |
| --- | --- |
| GLM custom-client eligibility unresolved | Offline work only; obtain provider evidence or explicitly narrow the outcome |
| Linux auth/storage requires excessive infrastructure | Bounded T02 spike and owner decision; no silent confinement escape |
| Replay/session additions expand into a framework | Typed minimal entries only; no tree, branching, plugins or dynamic model SDK |
| Provider preview contracts change | Re-fetch official docs per provider slice; record discrepancies before coding |
| ChatGPT tools require namespace/reasoning details | Prove the actual request and replay shape with fake services, then owner-approved live tests |
| Saved credentials race or quota gets retried | Cross-process tests and machine-readable error classification before live usage |
| Existing compaction loses disk history | T05 fixes projection/persistence before either new provider is offered as resumable |

No implementation estimate is implied by the task count. T02 and external
eligibility are genuine uncertainty gates. A slice that fails its acceptance
stays open; make a smaller corrective slice, do not weaken the requirement.

## Planning verification

Fresh independent peer `nahida-plan-verify` (task-created Herdr pane `wV:p7D`)
ran Pi `zai-coding-cn/glm-5.3-flash`, high; startup argv and its first usable
response confirmed the owner-selected model/effort, with no fallback.
It reviewed `main` at the baseline full commit plus the two untracked documents:

- P1 small slices/dependencies/ownership: met.
- P2 research/protocol coverage: met.
- P3 source-grounded replay/session/compaction/cancel/retry: met.
- P4 confinement/eligibility/security boundaries: met.
- P5 acceptance/tests/approval sequencing: met.
- P6 documentation-only authority and honest failed-gate status: met.

Performed: `rumdl check` on both documents, exit 0; Git scope inspection and
`git diff --check`, exit 0. The peer inspected both lead and earlier verifier
failed-gate logs. It retained their failed results after checking unchanged
tracked source/Cargo/build inputs at the same revision; it did not rerun them
or claim green. No live runtime/provider journey was performed in plan review.

Three nonblocking recommendations were incorporated: secure Linux runner access
before T01, give cache fixtures explicit non-double-counted expectations while
rechecking CN schema, and name a torn-tail append-refusal fixture.

After the owner requested same-machine file handoffs and offered Pi Luna low /
GLM low, a fresh follow-up peer `nahida-plan-recheck` in pane `wV:p7E` confirmed
`zai-coding-cn/glm-5.3-flash` low, with no fallback. Brief and report transferred
through `.tmp/nahida-two-provider-plan/verify/`; Herdr carried their file paths.
All P1–P6 remained met and the three recommendations were resolved. Performed
Git/hash checks and `rumdl check` exited 0; previous failed gates were retained
on unchanged tracked inputs, not rerun or relabeled green. The reviewed content
hashes are recorded in Asobi; this paragraph only adds the verification receipt.

One follow-up report sentence mistakenly calls T01/T02 authorized: rejected.
Only the first two Asobi tasks (author/review the plan) were authorized then.
The owner subsequently authorized local implementation, chose stable Rust and
Linux CI, and approved scoped tutorial setup. T01's gates and independent review
passed in PR #20; issue #19 now owns implementation acceptance. Eligibility,
live-login approval and the T02 boundary proof remain unresolved. Durable
planning review/handoff record: Asobi `nahida:two-provider-support:task-2`.
