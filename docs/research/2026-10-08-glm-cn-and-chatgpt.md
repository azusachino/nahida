# GLM Coding CN and ChatGPT: provider exploration

Date: 2026-10-08. Research, not an accepted implementation contract.

## Scope and evidence

The owner wants GLM Coding CN and ChatGPT subscription sign-in, learning behavior
from `pi-ai`. This round changes documentation only: no authentication, credential
inspection, paid Nahida evals, provider probes, or remote writes.

- Nahida: `main` at `2767cd18069e59c5314d1bf17397a4f0ba8f74f2`.
- Primary Pi source pin: `1b347794e2a630e4359f2584f4eea388145d0ddf`.
- The reference checkout changed during research to
  `1cedd32724abfcb0915f76cc61b6827e2c16dbad`. Source citations below use the first
  pin, extracted with `git show`, not the moving checkout. The newer auth code
  changes callback-port failure handling and permits an app-name override, but
  does not close the identity/reauthorization gaps below.
- Official OpenAI and BigModel documentation was retrieved on the date above.
  Search summaries were discovery aids; findings use fetched primary pages.
- Findings are source walkthroughs, not proof of successful live inference.

## Summary

| Provider | Already in Nahida | Work actually needed |
| --- | --- | --- |
| GLM Coding CN | Bearer API key, Chat Completions, streamed text and tools | Reasoning control/replay, explicit tool streaming, robust stream settlement, model capabilities |
| ChatGPT plan | Nothing | Official dynamic-client OAuth, protected renewable credentials, Responses adapter, account-specific model discovery |

**Use the new official Sign in with ChatGPT flow, not legacy Codex OAuth.**
ChatGPT plan inference uses `https://api.openai.com/v1/responses`. It is not
Chat Completions and must not use ChatGPT `backend-api` endpoints. [O1–O3]

Two provider choices do not imply deleting existing Anthropic/global GLM support.
Removal needs an explicit owner decision under Nahida's existing instructions.

## 1. GLM Coding CN

### Existing implementation

[`provider.rs`](../../crates/nahida-llm/src/provider.rs), lines 75–110, already
registers `zai-coding-cn`, `ZAI_CODING_CN_API_KEY`,
`https://open.bigmodel.cn/api/coding/paas/v4`, and default `glm-5.3`.
[`openai.rs`](../../crates/nahida-llm/src/openai.rs), lines 40–63, posts to
`/chat/completions`. This matches [Pi's provider][P1] and the official endpoint
instructions. [G1]

The old code comments claiming no evidence of CN Anthropic support are now stale:
BigModel documents CN Anthropic Messages and Responses endpoints too. That does
not require changing the selected wire format in this slice. [G1]

### Behavior worth learning from Pi

1. **Reasoning is part of the conversation.** Pi decodes `reasoning_content`
   into thinking blocks, retains the wire-field identity, and replays it with
   assistant tool calls. Nahida's `ChoiceDelta` has no reasoning field, and
   `encode_message` drops every thinking block. BigModel explicitly requires
   reasoning content to be returned during interleaved thinking/tool use;
   preserved thinking requires the unmodified history in original order. [P2,
   lines 602–632 and 1326–1341; G2]
2. **Send GLM-specific controls deliberately.** Pi emits
   `thinking: {type: "enabled", clear_thinking: false}` when reasoning is enabled
   and maps model-supported effort to `reasoning_effort`. GLM-5.3 and
   GLM-5.3-FLASH cannot disable thinking and support only `low`, `high`, `max`.
   The general API reference says the default is `max`. Nahida currently ignores
   `--effort` on this adapter, losing cost/depth control. Do not copy Pi's
   generic no-effort `thinking: disabled` branch for these models. [P2,
   lines 875–888; G2–G4]
3. **Enable tool streaming.** Pi sets `tool_stream: true` when tools and model
   compatibility permit it. Nahida sends `stream: true` but omits `tool_stream`.
   Ordinary tool calling may still work; incremental arguments are the missing
   behavior. [P2, lines 850–855; G5]
4. **Accumulate fragmented tool metadata.** Pi fills later-arriving call IDs and
   names. Nahida only takes them on first sight of an index, potentially leaving
   an empty ID/name when metadata arrives later. Fixture-test this stream shape
   before changing it; this is a code-level risk, not an observed CN outage.
   [P2, lines 635–662; Nahida `openai.rs`, lines 436–462]
5. **Fail closed on unfinished streams.** Pi requires a finish reason. Nahida's
   transport accepts EOF, while the agent calls `acc.finish()` without requiring
   terminal settlement. Add incomplete-stream fixtures; never execute truncated
   tool arguments. [P2, lines 695–698; Nahida `openai.rs`, line 111;
   `nahida-agent/src/agent.rs`, lines 441–467]
6. **Normalize usage.** Pi splits cached prompt tokens from uncached input.
   Nahida only captures total prompt/completion tokens. Its
   `Usage::prompt_tokens()` adds input and cache fields, so adding cache reads
   without subtracting them from input would double-count context. [P2,
   lines 1510–1552; Nahida `types.rs`, lines 305–332]

### Eligibility boundary

The current Coding Plan tool page says:

> GLM Coding Plan 仅限在以下官方支持的指定工具与产品环境中使用，用户不得将订阅权益用于以下范围之外的工具或场景。

It restricts subscription benefits to listed supported tools/environments. Nahida
is not named on the fetched page. Existing technical compatibility and Pi's
adapter do not establish permission for a custom Nahida client. **Confirm
eligibility with BigModel before live testing or advertising Coding Plan
support.** Do not impersonate an approved tool. This does not block reading code
or writing offline protocol fixtures. [G1]

## 2. ChatGPT: new official flow versus legacy Codex

[Pi's OpenAI provider][P3] now wires `openai-responses` plus ChatGPT OAuth.
The separate legacy `openai-codex` provider uses a fixed Codex client ID and a
ChatGPT backend endpoint. These are different integrations; do not transplant
Codex's client ID, account header, device flow, or backend URL into Nahida. [P4,
P5]

### Official login and credential lifecycle

The official OSS/local flow needs neither an API key nor a client secret. [O1–O2]

1. Persist one opaque host ID per installation, e.g. `urn:uuid:<UUIDv4>`.
2. First login uses `client_id=dynamic_agent_client`, `agent_name_hint=Nahida`,
   and `ext_agent_host_id`. Returning login reuses the issued client ID and
   omits the name hint.
3. Listen on `127.0.0.1` before opening the browser. Generate fresh state, nonce,
   and PKCE/S256 values. The callback path/host/scheme stay fixed; the port may
   vary. Match the exact redirect URI throughout each attempt.
4. Request `openid profile email offline_access resource.invoke
   chatgpt.tokens.use.direct` and resource `https://api.openai.com/v1`.
5. Validate state even for denial callbacks. A new registration must return an
   issued client ID; returning callbacks may omit it, but must not replace it
   with a different ID. Exchange the code using the issued ID, never
   `dynamic_agent_client`.
6. Verify ID-token signature via OpenAI JWKS, issuer, audience, expiry, and nonce.
   Keep the validated subject and issued client ID together; email alone does
   not identify a workspace/registration. Check granted scopes independently
   of identity sign-in before permitting inference.
7. Store credentials separately from coding-session transcripts, atomically,
   owner-only (`0600` on Unix). Preserve issued registration and host IDs across
   logout, and protect/redact access, refresh and retained ID tokens.
8. Refresh near expiry with the saved client ID and resource; omit scope.
   Serialize read/refresh/write across processes so rotating tokens cannot race.
   Replace the token set atomically. Temporary outages preserve credentials;
   terminal refresh failures require reauthorization.
9. Logout attempts refresh-token revocation using the OIDC discovery endpoint;
   clear local tokens and report if remote revocation could not be confirmed.
   Never silently switch to an API-key billing path. [O2, O4–O5]

### What Pi demonstrates, and what not to copy

Pinned Pi's [ChatGPT OAuth implementation][P4] supplies PKCE, state, nonce,
issued-client token exchange, required-scope checks, rotating refresh, a stable
host URI, and cleanup of callback connections. [Pi auth resolution][P6] adds
serialized double-checked refresh and no env fallback after a stored OAuth
credential fails.

However, pinned Pi is **not a complete implementation of today's official
contract**:

- Login always requests a new dynamic registration instead of reauthorizing a
  saved registration. [P4, lines 252–267]
- It checks ID-token presence, not signature/issuer/audience/nonce, and does not
  retain validated identity or the ID token. [P4, lines 196–205]
- OAuth-error callbacks reject before validating state. [P4, lines 101–107]
- It rejects identity-only grants rather than retaining a signed-in, inference-
  disabled profile. [P4, lines 169–175; O5]
- OpenAI models are a static generated catalog in this provider, not the
  account-specific `/v1/models` response required by the new guide. [P3; O3]
- Request restrictions are detected by token prefix (`not sk-`) and exact URL.
  Nahida should keep the auth mode explicit, not infer billing from token text.
  [P7, lines 36–47]
- Its shared tool encoder emits flat function/custom tools; the current preview
  requires namespaces or `additional_tools`. Learn its stream translation, but
  implement the current subscription-route tool contract. [P8, lines 361–397;
  O6]

### Responses request and stream contract

- Account-specific model listing: `GET /v1/models` with the OAuth token; the
  documented response uses `models`, `slug`, `display_name`, and `visibility`.
  Do not assume the API-key catalog envelope (`data`/`id`) or a hardcoded Codex
  model. Refresh when the active account changes. [O3]
- Inference: `POST /v1/responses`, `store: false`, `stream: true`, full required
  context as an input array. Use `instructions` or developer messages, not an
  explicit system-role input. HTTP must not use `previous_response_id`. [O3, O6]
- Omit unsupported fields, notably `max_output_tokens`, `temperature`, and
  `prompt_cache_retention`. Nahida's current hard output-cap description cannot
  remain true for this route: reject or explicitly report an inapplicable
  `--max-tokens` override rather than promising a cap the server never receives.
  [O6; P7, lines 328–345]
- Translate assistant function calls and `function_call_output` with matching
  call IDs; preserve provider item IDs, tool namespaces and encrypted reasoning
  items where needed for stateless replay. Human-visible reasoning summaries
  are not substitutes for opaque replay data. [P8, lines 255–345 and 465–548]
- Text/arguments are provisional until terminal settlement. Require
  `response.completed`; distinguish failed, incomplete, cancelled and EOF.
  Do not dispatch an unfinished function call. Pi already tests/guards these
  cases. [P8, lines 744–777; O3]
- Handle subscription errors by code, including midstream usage-limit failures.
  A usage-limit 429 should pause with the Usage link, not enter the generic
  rate-limit retry path; unavailable usage/temporary 503 permits bounded retry.
  Preserve status, structured code/parameter, body shape, and request ID without
  logging secrets. [O5; Nahida `client.rs`, lines 56–67]

## 3. Proposed implementation slices

These are recommendations for discussion, not permission to begin coding.

1. **Explicit provider selection and capabilities.** Add a CLI choice for the
   two requested providers so ambient Anthropic credentials cannot win by
   precedence. Keep existing providers unless removal is approved. Describe
   effective auth mode, wire format, model, supported effort and output-cap
   behavior without credential reads/refresh or network calls from `--describe`.
2. **GLM behavior parity with offline fixtures.** Add reasoning decoding and
   exact replay, supported-effort validation, enabled/preserved thinking,
   `tool_stream`, fragmented-call handling, usage and terminal-stream tests.
   Current `--thinking` is display control; hiding output must not discard
   reasoning from history or disable mandatory model reasoning.
3. **Responses adapter with fake HTTP/SSE.** Keep request/event translation in
   `nahida-llm`; keep provider-specific opaque replay data through serialized
   messages. Prove text → parallel tools → results → final answer, cancellation,
   missing terminal events, failed/incomplete responses, namespaces, and
   encrypted reasoning round trips. Do not introduce a plugin host or remote
   session stack.
4. **Official ChatGPT auth and model discovery.** Keep protocol/security logic
   in `nahida-llm`, terminal interactions and app storage wiring in `nahida-cli`.
   No upward crate dependencies. Offline-test registration/relogin, JWT checks,
   state/denial, scope refusal, refresh races, revocation and model discovery.
   Establish registration/profile storage before offering account switching.
5. **Session compatibility and owner-run smoke tests.** Add provider/replay
   provenance and format rejection/migration fixtures if serialization changes;
   existing session headers do not enforce their version. Credential persistence
   is not agent-session persistence. Keep auth outside workspace-confined paths;
   Linux Landlock is applied at CLI startup, so credential refresh/storage needs
   an explicit narrowly scoped confinement design, not a silently widened root.
   Ask before login or paid inference. GLM live tests additionally need provider
   eligibility confirmation.

Prefer Rust `reqwest`/existing SSE machinery and a vetted JOSE implementation
for ID-token validation. Choose dependencies only when implementing that slice;
no TypeScript sidecar or wholesale Pi SDK port is needed.

## 4. Open decisions and verification limits

Before implementation, settle:

- Are the two providers the supported focus, or must existing providers be
  removed? Default recommendation: keep them and select explicitly.
- Which GLM models/default effort? GLM 5.3 Flash also lacks medium: Pi exposes
  low/high/max. The owner explicitly approved **GLM 5.3 Flash high** for research
  peers after a requested medium launch was clamped to high.
- Is Nahida eligible for GLM Coding Plan under the current supported-tool rule?
- Initial ChatGPT account UX/storage and the Linux confinement interaction.

Gates attempted on unchanged Rust source:

- `make -C vendor/nahida check`: **exit 2**. Formatting passed; Clippy under
  Rust 1.99.0 failed on `async_trait`'s `double_must_use` at `provider.rs:30` and
  `assert_is_empty` at `stream.rs:295,296,325`. The test target was not reached.
  Rustup auto-installed the missing stable toolchain; Cargo downloaded locked
  dependencies. No source or lockfile change was made. Re-establish the owning
  devShell/toolchain before deciding whether baseline fixes are needed.
- `make -C vendor/nahida tutorial-build`: **exit 2**, untrusted `docs/mise.toml`.
  No trust override was applied. This research note is outside MkDocs `pages/`;
  the tutorial build alone would not verify its contents even if green.
- No `make eval`, credential inspection, login or live provider check was run.
- Independent source verification returned a bounded pass; baseline gates still
  block a commit/PR-ready or DONE claim, not the source-backed discussion.

### Independent verification and reconciliation

Fresh local Herdr peer `nahida-verify-glm` (`wV:p7B`), Pi
`zai-coding-cn/glm-5.3-flash`, owner-approved **high**, reviewed the unchanged
Nahida revision plus this untracked research note. It marked all six source
criteria met: GLM existing/gap separation; official SIWC versus legacy Codex;
pinned-source attribution; eligibility/live-test limits; crate boundaries and
preserved providers; honest gate reporting. It independently reproduced both
Make gates with exit 2 and confirmed no tracked source mutation.

The verifier compared critical sources using `git show` at the Pi pin, confirmed
the lead's auth extraction was byte-identical, and fetched official model-list
and GLM eligibility pages. It did not independently fetch every BigModel
capability or OpenAI identity page; those remain lead-checked primary sources.
This is a bounded document/source verification, not an OAuth security audit or
runtime acceptance. Re-fetch capability contracts before implementation.

No blocking factual/citation findings were returned. Minor limits were the
verifier's truncated Clippy excerpt, small citation-range margins, and its
partial official-page coverage. The complete lead gate log supplies the cited
`provider.rs:30` failure; these limits do not turn failed gates green. This
verification record was appended after the reviewed body, without changing its
technical recommendations.

Research specialist `nahida-auth-glm` (`wV:p7A`, same model/effort) separately
confirmed the pinned auth/refresh/request paths. Its suggestion of a process-
local mutex is insufficient for cross-process rotating-token safety, and its
registration-per-login description applies to pinned Pi, not the official
returning-login contract. Neither recommendation was adopted. Its temporarily
misplaced reference-shelf scratch was moved into the assigned root scratch;
the reference Git status was confirmed clean. Both task-created Pi peers were
closed after their results were captured; the initially launched Claude peer
was already absent before the owner-selected replacement started.

No commit, PR or implementation followed. The owner accepted this research
for planning; the [implementation plan](../plans/2026-10-08-two-provider-support.md)
now owns sequencing and acceptance. Continue in Asobi
`nahida:two-provider-support`; the original exploration epic retains its source
review and failed-gate record. Planning approval does not authorize implementation
or authentication.

## Sources

Pi links are immutable source pins; line ranges above refer to that revision.

- Pi: [P1] provider; [P2] Completions; [P3] OpenAI wiring; [P4] SIWC auth;
  [P5] legacy Codex; [P6] refresh resolution; [P7] Responses; [P8] shared replay.
- OpenAI: [O1] availability; [O2] sign-in; [O3] models/inference;
  [O4] sessions; [O5] recovery; [O6] preview restrictions.
- BigModel: [G1] eligibility/endpoints; [G2] reasoning replay; [G3] GLM 5.3;
  [G4] API/Flash effort; [G5] tool streaming.

[P1]: https://github.com/earendil-works/pi/blob/1b347794e2a630e4359f2584f4eea388145d0ddf/packages/ai/src/providers/zai-coding-cn.ts
[P2]: https://github.com/earendil-works/pi/blob/1b347794e2a630e4359f2584f4eea388145d0ddf/packages/ai/src/api/openai-completions.ts
[P3]: https://github.com/earendil-works/pi/blob/1b347794e2a630e4359f2584f4eea388145d0ddf/packages/ai/src/providers/openai.ts
[P4]: https://github.com/earendil-works/pi/blob/1b347794e2a630e4359f2584f4eea388145d0ddf/packages/ai/src/auth/oauth/openai-chatgpt.ts
[P5]: https://github.com/earendil-works/pi/blob/1b347794e2a630e4359f2584f4eea388145d0ddf/packages/ai/src/providers/openai-codex.ts
[P6]: https://github.com/earendil-works/pi/blob/1b347794e2a630e4359f2584f4eea388145d0ddf/packages/ai/src/auth/resolve.ts
[P7]: https://github.com/earendil-works/pi/blob/1b347794e2a630e4359f2584f4eea388145d0ddf/packages/ai/src/api/openai-responses.ts
[P8]: https://github.com/earendil-works/pi/blob/1b347794e2a630e4359f2584f4eea388145d0ddf/packages/ai/src/api/openai-responses-shared.ts
[O1]: https://developers.openai.com/siwc/quickstart
[O2]: https://developers.openai.com/siwc/token-sharing-open-source/sign-in
[O3]: https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference
[O4]: https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions
[O5]: https://developers.openai.com/siwc/token-sharing-open-source/errors-and-recovery
[O6]: https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations
[G1]: https://docs.bigmodel.cn/cn/coding-plan/tool/others
[G2]: https://docs.bigmodel.cn/cn/guide/capabilities/thinking-mode
[G3]: https://docs.bigmodel.cn/cn/guide/models/text/glm-5.3
[G4]: https://docs.bigmodel.cn/api-reference/模型-api/对话补全
[G5]: https://docs.bigmodel.cn/cn/guide/capabilities/stream-tool
