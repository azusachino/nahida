# Plan: run the existing GLM configuration in native Nahida

Date: 2026-10-15. Status: planned; GPT-6-Luna implementation assigned.

## Outcome and baseline

The owner tried the installed binary with `--config-dir ~/.pi/agent
--provider zai-coding-cn --model glm-5.3-flash` and it rejected selected
options. Today deliver that configuration-to-tool-turn path, not merely remove
validation. This is the bounded S1-S3 work in the
[native milestone](2026-10-10-native-agent-milestone.md), plus the PR24
credential-precedence defect. It does not complete the broader milestone,
issue #19, or original C5's historical Herdr-specific acceptance.

Starting Nahida: `feat/pi-native-macos`,
`997b2c2175945e246001c4a79f7124481cfd2742`; PR24 remains stacked on PR23.
Parent owns integration and acceptance. Workflow: workstation task, tier 1
(multiple agents, durable plan/handoff). No release or published cross-repo
contract is requested. Existing issue #19 owns the larger task ledger.

Pinned source evidence: clean read-only `refs/coding-agents/pi` at
`42a3497d03ad17e308a2299fa824727894f2c0ec`. Inspect its model runtime/loader,
`packages/ai/src/api/openai-completions.ts`, types/model validation and
`packages/ai/src/providers/zai-coding-cn*`, with relevant tests. This revision
is not asserted equivalent to the previously installed Pi 1.1.0. Record exact
source paths and ported behavior; retain MIT attribution for substantial ports.

Only selected non-secret metadata was inspected; no auth file was read.
The existing record has reasoning metadata, these compat options:

- `supportsStore: false`, `supportsDeveloperRole: false`;
- `supportsReasoningEffort: true`, `maxTokensField: "max_tokens"`;
- `thinkingFormat: "zai"`, `supportsStrictMode: true`, `zaiToolStream: true`.

Thinking mapping: `off`, `minimal`, `medium`, `xhigh` map to null;
`low`, `high`, `max` map to their corresponding strings. Null and missing are
not interchangeable. Do not rewrite or simplify the owner's record.

## Acceptance

- **G1 Load:** real binary metadata inspection accepts the selected existing
  provider/model with every listed option interpreted, not silently discarded.
  Inspection never opens/stats auth, expands key sources or calls HTTP. Synthetic
  fixtures cover provider/model overlays and malformed/unsupported options.
- **G2 Request:** captured loopback HTTP matches pinned Pi's effective GLM
  behavior for the selected compat fields, thinking enable/disable and supported
  effort mappings. Unsupported requested levels fail before HTTP. Omitting effort
  follows source-backed default semantics, not an invented null fallback.
- **G3 Replay/settlement:** thinking and streamed tool deltas survive the neutral
  event/transcript path; subsequent requests replay required reasoning exactly.
  Actual binary writes exact fixture bytes, returns the matching successful
  result and settles with final text. Split frames and multi-turn calls work.
  Malformed, truncated, errored or cancelled streams cannot execute partial calls
  or be reported as successful completion. No promise to roll back prior writes.
- **G4 Credentials:** automatic provider identity retains legacy environment
  precedence, then applies stored-key precedence and stored-OAuth rejection.
  Explicit and configured-default paths remain correct. Synthetic tests prove
  no ambient API billing fallback, no credential mutation or secret leakage.
- **G5 Regression:** existing Anthropic and OpenAI configurations, env selection,
  caching/effort dialect behavior and crate boundaries retain their contracts.
  No transport/configuration dependencies enter the neutral agent layer.
- **G6 Delivery:** focused tests and owning gates pass; fresh GPT-6-Luna review
  covers G1-G5 at a stable identity. Record provenance and remaining limits in
  owning docs. Reinstall the verified binary to `~/.cargo/bin/nahida`, check
  version/binary identity and perform credential-free metadata inspection of
  the actual selected record. Live calls are not acceptance evidence unless
  separately authorized with eligibility and operational limits established.

## Fleet and dependency order

Topology: multi-seam. Configuration interpretation, OpenAI wire/stream/replay
and host credential selection are independently testable contracts. Concurrent
component writers get separate worktrees and exclusive files. The parent does
not edit their checkouts. All agents use `openai-codex/gpt-6-luna`.

| Lane | Ownership and output | Dependency / focused check |
| --- | --- | --- |
| Source scout | Read-only pinned Pi/Nahida analysis; exact options, default/effort/replay semantics and minimal cross-crate handoff contract | First; stop on unsupported provenance or unresolved semantics |
| Config worker | `crates/nahida-config/` only: selected option interpretation, overlay/null preservation and tests | Scout; `cargo test -p nahida-config` |
| Wire worker | `crates/nahida-llm/` except `src/provider.rs`, plus neutral stream types only if genuinely needed in this crate: request compat, reasoning replay, stream settlement and focused fixtures | Scout; `cargo test -p nahida-llm -p nahida-agent` |
| Credential worker | CLI `src/configuration.rs`, LLM `src/provider.rs`, new CLI `tests/credential_precedence.rs`: provider identity and no-fallback regression fix only | Scout; focused CLI credential tests and existing selection tests |
| Integration worker | Existing main checkout, after all component handoffs: apply owned patches, wire config into transports/host, actual-binary GLM fixtures and accurate README | All components; CLI journey, then complete gates |
| Fresh verifier | Read-only integrated source/criteria, independent synthetic binary journey and gates; no credential access | Stable integration; G1-G6 evidence and concrete findings |

Component reports include base/full revision, changed files, diff/patch location,
focused commands/exits and any incomplete work. A successful child status is not
acceptance. Component partial work is preserved, never reset/stashed or merged
blindly. Integration is integration-only: unimplemented component responsibilities
return to their owner rather than becoming an issue-wide rewrite.

## Gates and stop boundaries

Use the existing installed toolchain; no dependency upgrades, toolchain installs
or Nix/host repair. From owning Nahida checkout:

```bash
RUSTUP_AUTO_INSTALL=0 CARGO_NET_OFFLINE=true make validate
UV_LOCKED=1 UV_OFFLINE=1 UV_PYTHON_DOWNLOADS=never make tutorial-build
rumdl check README.md docs/plans/2026-10-15-glm-config-execution.md
git diff --check
```

Run focused checks during slices. Before any commit, owning `make check` must
pass. Before a PR/push, `make validate` must pass; preserve the existing stacked
history and name `azusachino/nahida` on GitHub commands. Parent alone owns remote
publication and final installation. No child may push, publish, merge, release,
modify user config, read real auth, run live inference or delegate further.

No TUI, Herdr launcher/control, parent grants, OAuth login/refresh, ChatGPT
Responses, Linux sandbox/storage, new configuration mode or JS bridge. Existing
unsupported selected options outside today's implemented set still fail clearly.
No removing tests/validation to claim parity. At most two fix/review rounds;
unresolved findings or any failed/unavailable owning gate stop delivery and
require an explicit blocker report, not a weaker replacement check.

## Progress and closeout

Initial state: plan recorded; no production fixes delivered. The previous binary
at `997b2c2` remains unsuitable for the owner's selected GLM record. The previous
install and successful generic API-key tests are not evidence for G1-G6.

At close record actual integrated commit, source provenance, synthetic fixture
results, gate commands/exits, review disposition, installed binary identity,
actual metadata-inspection result, changed/uncommitted paths and next owner
command. Keep implementation status with issue #19 / its owning PR and this
repository; harness artifacts are supporting caches only.
