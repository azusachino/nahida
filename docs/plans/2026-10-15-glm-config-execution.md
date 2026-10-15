# Plan: run the existing GLM configuration in native Nahida

Date: 2026-10-15. Status: locally delivered and independently verified;
installed binary accepts the actual selected metadata. Live inference untested.

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

## Integration follow-up (2026-10-15)

Integration applied the three component patches based on `7e604e2` after all
three initial `git apply --check` commands passed. After that clean integration,
the wire owner supplied `wire-streaming-fix.patch`; its `git apply --check`
passed and it was applied without overlap. It preserves incremental text,
thinking and tool progress while validating `[DONE]`, finish reason, tool IDs
and complete JSON arguments before successful settlement. CLI host composition
translates config-owned metadata into OpenAI transport options. No general Pi
compatibility is claimed; source provenance and MIT attribution are recorded
above.

Actual-binary synthetic loopback coverage includes effort maps/no effort,
strict tool schema/request fields, reasoning and tool replay, exact UTF-8
artifact bytes, final settlement, null-effort rejection before HTTP, malformed
and truncated tool data, explicit provider error, and SIGINT during a partial
tool-argument stream. The cancellation fixture confirms the signal handler ran,
the cancelled turn emitted no `done` event, and no artifact was written; server
close and process/accept timeouts bound cleanup. The supported effort and
success journey also verify the actual write result and subsequent final turn.

Focused checks passed after integration: config/LLM/agent unit and integration
tests, plus the CLI configuration, credential-precedence, GLM-execution and
provider-selection suites. Exact owning gates eventually passed:

- `RUSTUP_AUTO_INSTALL=0 CARGO_NET_OFFLINE=true make validate` — passed,
  including format, Clippy, full test suite and release build.
- `UV_LOCKED=1 UV_OFFLINE=1 UV_PYTHON_DOWNLOADS=never make tutorial-build` — passed.
- `rumdl check README.md docs/plans/2026-10-15-glm-config-execution.md` — passed.
- `git diff --check` — passed.

Gate iteration history is retained honestly: initial `make validate` first
reported rustfmt in the new CLI fixture; after formatting, Clippy found simple
assertion lints, which were corrected. A subsequent full validation caught a
fixture-only ordering assumption for normalized required fields; the test now
checks both required properties without depending on ordering. Those corrected
runs passed the exact gates above. The first rumdl run found two long lines in
the README/plan; they were wrapped and the exact command then passed. The first
cancellation assertion incorrectly required a nonzero process exit, although
Nahida handles SIGINT gracefully; the final fixture instead verifies the signal
handler ran, no `done` event was emitted, no artifact was written, and bounded
cleanup completed.

Credential fixture intervention: the initial attempted automatic-CN precedence
fixture injected a higher-priority synthetic `ANTHROPIC_API_KEY` while only
configuring a CN loopback endpoint. It therefore could have selected the
built-in non-loopback Anthropic endpoint with a fake value; whether any remote
request completed is not established. No real credential was used. Parent's
scratch incident record is `.tmp/nahida-glm-fleet/fixture-intervention.md`.
The corrected fixture supplies only the synthetic CN key for automatic CN
selection, retains the separate legacy-precedence test with an Anthropic
loopback endpoint, uses bounded accept/process waits, and drains HTTP request
bodies before replying. Final focused tests pass with the correction.

No real auth/config was read, no live/non-loopback inference intentionally
performed, and no install, commit or remote action occurred. This work is not
actual selected-record metadata preflight, Linux proof, independent review,
full milestone completion, or final G1-G6 acceptance. Candidate source/hash
and executable identity are in the integration handoff; parent retains
acceptance and publication authority.

## Independent review round 1 — strict optional arguments (2026-10-15)

The reviewer found a P1 in strict JSON-schema conversion: optional tool fields
were marked required but left non-nullable. The pinned Pi strict converter
(`refs/coding-agents/pi/packages/ai/src/api/constrained-sampling.ts:110-123`)
first wraps optional properties as `anyOf: [original, {type: null}]`, then
requires all fields. Integrated the wire owner's checked incremental patch,
which adds recursive, fail-closed required-nullable conversion while preserving
required/non-nullable semantics. The converter rejects unsupported forms rather
than silently changing their meaning.

Added actual-binary assertions that `read.offset`/`read.limit` and all six
optional `grep` inputs are nullable in strict schemas while `read.path` and
`grep.pattern` remain required non-nullable. A loopback journey invokes the real
`read` tool with explicit null offset/limit and with both fields omitted; both
must return the default complete file content, matching tool result, and final
answer. Both journeys pass. Existing write/reasoning replay/live progress,
malformed/truncated/provider-error, and SIGINT partial-stream/no-side-effect
fixtures remain intact.

Wire patch provenance: `wire-strict-fix.patch` passed `git apply --check`
before application. Pinned Pi remains read-only at
`42a3497d03ad17e308a2299fa824727894f2c0ec`; MIT attribution is in the
transport source. After this review fix, `make validate`, tutorial-build,
rumdl, and `git diff --check` passed. Round 2 is the plan's final review round.
No user auth/config inspection, live inference, install, commit, or remote
action. The first rumdl pass after adding this evidence found an overlong patch
path line; shortened it to the patch basename and reran the exact check
successfully.

Round 1 focused evidence: `cargo test -p nahida-cli --test glm_execution`
passed 9/9; `cargo test -p nahida-config -p nahida-llm -p nahida-agent`
passed (7 config, 46 LLM, 20 agent integration tests and doctests);
`cargo test -p nahida-cli --test glm_execution --test configuration
--test credential_precedence --test provider_selection` passed (31 CLI
tests). Final round-1 gates: offline `make validate`, locked/offline
`make tutorial-build`, `rumdl check README.md
docs/plans/2026-10-15-glm-config-execution.md`, and `git diff --check` all
passed after this fix. Tutorial build emitted the existing MkDocs Material
warning; build succeeded.

Candidate source diff SHA-256 over `crates/nahida-cli/src/configuration.rs`,
`crates/nahida-config/src/lib.rs`, `crates/nahida-llm/src/{configured.rs,
lib.rs,openai.rs,provider.rs}`, and both CLI test sources (docs excluded):
`c8a2dfd98708dbe2a537a0c121953fa1d3e8822ba2a8d684c4a3f14beae5a587`.
Tested `target/debug/nahida`: version `0.1.0`, Mach-O arm64, SHA-256
`6174b0a16e5a9a98f132d085b639422450f567347276a91af9e0b7e7c63c1ef5`.
No install performed at that integration checkpoint; parent closeout follows.

## Parent acceptance and local delivery (2026-10-15)

Round-2 independent GPT-6-Luna verifier found no remaining blockers. It rechecked
G1-G5, the strict optional/null correction against pinned Pi source, the actual
read/write/reasoning/tool-result journeys, incremental streaming, cancellation
and credential no-fallback fixtures. It independently reran all owning gates:
`make validate`, locked/offline `make tutorial-build`, changed-document rumdl
and diff checks; each exited 0. The initial P1 is resolved, not deferred.
No third review round was needed. G1-G5 source/runtime acceptance is met for the
bounded synthetic journeys, not all Pi configurations or provider features.

The verifier's round-2 run was `30909831-3fc6-4bbf-a2f0-dac92f082fe9`.
Parent confirmed the eight implementation/test paths are byte-identical to
that review. Defined digest: sort repository-relative source paths, then hash
`path + NUL + file bytes + NUL` for each path with SHA-256. Result:
`56edd2faff65258c2acfcde4fa253f1454e9ed1e98b818e03808d862b0322f2c`.
The earlier integration digest used a different encoding and is not treated as
an equivalent hash. Final parent changes after review affect this evidence
record only; runtime sources and inputs remain unchanged.

Parent ran the release executable with `--config-dir ~/.pi/agent
--provider zai-coding-cn --model glm-5.3-flash --describe`. Exit 0, with resolved
provider `zai-coding-cn`, model `glm-5.3-flash`, output cap 131072 and no
unsupported-option diagnostic. This metadata-only route never resolves keys or
opens/stats auth storage; no live request was made and user files were unchanged.
The configuration's actual selected option set is accepted, not removed.

Parent then ran locked/offline `cargo install --path crates/nahida-cli
--root "$HOME/.cargo" --force`. Exit 0, replacing the previous binary with
`nahida 0.1.0`. Installed `~/.cargo/bin/nahida` is byte-identical (`cmp`, exit 0)
to the verified release build; SHA-256:
`bb79ab1195dc9a8be5113098c738d8e6018648af6107130d546bd4fd4746e8df`.
Installed metadata inspection repeated the same successful provider/model/cap
resolution. Cargo warned that locked `chacha20 v0.10.1` is yanked; no dependency
or lockfile was changed. The debug binary hash above identifies test execution;
the release hash here identifies the installed artifact.

G6 local delivery is complete after these parent checks and owning final gates.
The containing implementation commit on `feat/pi-native-macos` is the durable
source checkpoint; the owning PR24 retains the stacked history. Component
worktrees/patches are supporting scratch, not an alternative authoritative tree.
The source scout and three component writers were followed by integration and
fresh verification, all on GPT-6-Luna; parent retained installation/acceptance.

Owner next command:

```bash
nahida --config-dir ~/.pi/agent \
  --provider zai-coding-cn --model glm-5.3-flash
```

To request supported reasoning explicitly, add `--effort low` (or `high`/`max`);
add `--thinking` to display reasoning. No effort sends disabled Z.ai thinking,
matching the pinned provider encoder. Explicitly null-mapped levels still fail
before HTTP. No real authentication/login/paid inference was tested; credentials,
GLM custom-client eligibility and live-provider behavior are not established by
successful metadata inspection. Original issue #19, full milestone, TUI/Herdr,
OAuth/Responses and Linux storage acceptance remain open. macOS bash retains the
user's privileges. No release, deployment or user configuration edits occurred.
