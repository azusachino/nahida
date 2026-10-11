# Plan: compatible configuration, native Nahida execution

Date: 2026-10-10. Status: bounded API-key configuration slice delivered;
Responses/OAuth and Linux acceptance remain open.

## Owner-confirmed next milestone

The owner subsequently selected whole Pi product **capability parity** in native
Rust, not drop-in JavaScript ecosystem compatibility. The first next outcome is
real GLM, a Pi-like core native TUI and genuine Herdr agent control with a retained
in-memory session and parent-defined workspace permissions, without bash.
The [native agent milestone](2026-10-10-native-agent-milestone.md) owns that scope,
acceptance and delivery order. It supersedes the next-step ordering below, not
issue #19's A1–A7 or the native amendments.

The remaining text records the delivered configuration slice's boundaries.
Statements excluding extension-host/product-parity work apply to that historical
slice, not to the newly accepted long-term destination. No Pi/Node application
runtime bridge is authorized by either plan. Local progress may use the
owner-selected `.tmp/tasks/`; the owning issue carries the cross-device handoff
while Asobi is unavailable. No GLM live journey, TUI or native Herdr integration
has been delivered. Linux remains deferred.

## Outcome and corrections

Make the actual `nahida` executable read configuration compatible with Pi's
record formats and keep its native Rust agent loop. The owner clarified the
boundary after the first draft: **support the configuration, not Pi itself**.
`nahida-config` owns reading it. Selection stays `--provider` / `--model`, with
`--config-dir` for a generic home override. Nahida's default home is its own;
sharing another program's directory requires an explicit host choice.

The uncommitted `llm::pi` / `--pi-model` draft was the wrong interface and is
replaced, not delivered. No Pi application/SDK/runtime bridge, extension host
or provider/model wrapper mode is authorized. A fifth crate is appropriate
here because the owner explicitly identified configuration as a separate
concept; the four original concepts and the neutral agent loop remain intact.

The owner also explicitly deferred Linux to prioritize working macOS behavior.
The [macOS amendment](https://github.com/azusachino/nahida/issues/19#issuecomment-6092959752)
splits that prerequisite from this delivery; it does not complete cross-platform
acceptance. The earlier Node bridge proposal and the incorrect in-process-only
locking claim remain superseded. See the
[source correction](../research/2026-10-10-pi-layers-and-auth.md#review-correction-file-storage-and-native-rust).

## Homes and boundaries

- `nahida-config`: file reads and compatible model/settings/credential records;
  no HTTP, transport registry, agent or terminal dependency.
- `nahida-llm`: provider defaults, API interpretation, transport and auth/refresh
  protocol. Its configured transport constructor accepts resolved values,
  never opens configuration files.
- `nahida-cli`: ordinary selection flags, config/provider composition, host
  lifecycle, terminal and sessions. `nahida-agent` stays provider-neutral and
  `nahida-tools` is unchanged.
- Pi v1.1.0 source is format evidence, not an application dependency or a claim
  of implementing its entire provider catalog, extensions or settings.
- Tests use fake files and loopback endpoints. Real credential inspection,
  login and inference remain separately approved actions. GLM custom-client
  eligibility is still a live-use gate; configured keys do not establish it.
- macOS bash has the user's privileges; Linux Landlock permits reads everywhere.
  Neither shared JSON nor this read-only store provides credential-read isolation.
- Herdr is outer verification/control, not a dependency of the agent or model
  layer. The owner selected Herdr API verification. The installed Herdr has no
  native Nahida launcher: a fresh recognized verifier invokes the actual binary;
  this is not a claim of direct Herdr/Nahida agent integration.

## Acceptance

[Issue #19](https://github.com/azusachino/nahida/issues/19) retains A1–A7 and the
[native amendment](https://github.com/azusachino/nahida/issues/19#issuecomment-6092640643).
Existing independently saved-account/registration and cross-platform criteria
are not satisfied merely by compatible files. This first slice is read-only
API-key configuration; Responses/OAuth/session-contract work is still required.

- **C1 Ownership/interface:** configuration file/format interpretation lives in
  `nahida-config`, not the provider/agent layer. Ordinary `--provider`/`--model`
  selects built-in or configured providers. No Pi mode or runtime process.
- **C2 Actual executable:** synthetic files drive prompt, streamed text, a real
  local `write` tool, returned tool result and final answer in the actual binary.
  Both existing wire formats are exercised through the CLI. Provider/model,
  endpoint, authorization and output-cap assertions inspect actual HTTP requests.
- **C3 Selection/credentials:** explicit flags win over configured defaults;
  without those defaults legacy environment precedence remains. Stored selected
  keys win over config/environment sources. Templates/escapes work; commands
  never execute. Unsupported stored OAuth fails without downgrade or file write.
- **C4 Inspection/errors:** `--describe` never reads auth, resolves credentials,
  executes commands or contacts HTTP. File sizes are bounded. Invalid/unsupported
  selected configuration and provider errors do not echo keys, credential
  sources, response bodies or endpoint paths. Error redaction retains retry and
  overflow classifications. Values are not exported to tool environments.
- **C5 Verification:** local focused tests, `make validate`, strict tutorial
  build, fresh independent review and actual binary invocation through a
  Herdr API-controlled verifier, with exact source/command/output evidence.
  Mac-only success is not Linux proof. No paid fixture endpoint or live auth.

## Ordered slices

Live state remains Asobi `nahida:pi-native-macos` (historical task name).
Deferred Linux state remains `nahida:two-provider-support:task-4`, draft PR23.
One writer per checkout; hold reviewed source stable during verification.

1. Add the isolated configuration crate and focused file/format checks.
2. Compose normal selection with the existing providers; verify actual text/tool
   behavior and preserve legacy selection tests. Document limits, run the owning
   gates and dispatch the fresh Herdr API verifier before completion.
3. Continue native ChatGPT Responses, replay/session binding and Rust refresh on
   macOS in independently testable protocol/storage slices. Follow
   [ADR-0002](../decisions/0002-bind-replay-and-separate-storage-from-tool-permissions.md)
   for the accepted replay/v2 contract, with Linux deferred.

For renewable shared auth, match concrete `proper-lockfile` 4.1.2 behavior:
`auth.json.lock` mkdir, normalization, heartbeat/stale/compromised checks,
locked reload, expiry/type recheck, refresh, merge and persistence. A Rust mutex
or unrelated `flock` does not coordinate. Test synthetic concurrent Pi/Rust
writers against the pinned backend, unknown-field preservation, logout ordering
and cancellation-safe persistence. Any Node use is test interoperability tooling,
not the Nahida runtime.

Pi's direct `writeFileSync` is not atomic replacement. Test Rust crash-safe writes
and Pi cache behavior separately; do not claim protection against every crashing
Pi writer. Finish fake OAuth/Responses/restart/error journeys before asking for
separately scoped live login/inference approval.

## Deferred boundaries

Linux startup inheritance, outside-root session/refresh writes, private IPC and
credential-read isolation remain open. Earlier diagnostic and guarded descriptor
probes do not implement a safe storage owner. No further Linux proof work is on
this macOS task, and no cross-platform completion follows from it.

## Sources

- [File/auth source research](../research/2026-10-10-pi-layers-and-auth.md).
- [Two-provider plan](2026-10-08-two-provider-support.md),
  [ADR-0002](../decisions/0002-bind-replay-and-separate-storage-from-tool-permissions.md)
  and issue #19 amendments above.
- Owning `AGENTS.md` supplies crate boundaries and gates.
