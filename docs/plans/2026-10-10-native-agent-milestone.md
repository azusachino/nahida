# Plan: native Nahida, Pi capability parity

Date: 2026-10-10. Status: owner-confirmed outcome; implementation not started.

## Intent and scope

Nahida is a native Rust coding agent that Claude Code or Codex can call as a
sub-agent. Its long-term destination is Pi product **capability parity**, not
unchanged compatibility with Pi's JavaScript extensions, packages or SDK.
Rust-native distribution, learning the entire stack and better delegated-worker
control all matter. Port Pi's working implementation and tests, informed by its
model/provider commits; do not rederive a restrictive compatibility schema.
Preserve upstream license notices for copied or substantially ported code.

One engine supports a Pi-like native TUI, one-shot headless output and persistent
JSONL RPC ported from Pi. Reuse the RPC command/event contract against a pinned
source revision and conformance tests; it is not generic JSON-RPC 2.0. A JSON
output stream alone is not a command-input protocol. Herdr terminal prompting
and subprocess RPC are distinct control surfaces, not interchangeable wrappers.

The owner confirmed this outcome after four grilling rounds. The first milestone
is deliberately smaller than full parity: **real GLM, native TUI, genuine Herdr
agent control and a retained in-memory session on macOS**. This document advances
that milestone ahead of the remaining Responses/OAuth work; it does not cancel
that work or the original issue's criteria.

## Decisions and recorded failures

- At `14fecde173094ad388f76b99921f2b9b3dfe666b`, native metadata inspection of the
  existing Pi-directory `zai-coding-cn` / `glm-5.3-flash` selection failed with
  `selected provider/model uses options not implemented yet`. The selected
  record has `compat` and `thinkingLevelMap`. Earlier C1–C5 verification covered
  the bounded API-key slice, not this real configuration or full Pi parity.
- The Python demo proxied stdin/stdout and reported a custom `nahida` label while
  the real binary ran with `--json`. It hid the native terminal REPL and was not
  native integration. `herdr agent prompt` failed with `agent_not_ready`; no task
  was delivered. Do not repeat that demonstration or impersonate another kind.
- Herdr 0.9.3 accepts arbitrary reported labels for presentation, but prompting
  requires a recognized kind and matching foreground identity. A genuine Nahida
  kind/recognizer and lifecycle integration are needed; the owner permits the
  smallest Herdr-side change if required. A label, alias or idle report is not
  proof of callability, task completion or foreground ownership.
- The current binary has a line-oriented REPL, not the agreed TUI, parent-defined
  tool policy or Pi RPC. Existing cancellation/session code is not acceptance
  evidence for the new milestone.

## Architecture and capability map

Preserve one concept per crate and no upward dependency edges. These are the
existing homes, not a promise that the current five-crate count is a permanent
limit for the eventual full product.

| Module | Home | Responsibility | Depends on |
| --- | --- | --- | --- |
| configuration | `nahida-config` | Compatible files, selected model/settings/credential records | no transport/agent/UI |
| provider | `nahida-llm` | Catalog/defaults, compat interpretation, requests, streams, auth protocol | resolved configuration values, not files |
| agent | `nahida-agent` | Neutral turns, tools, cancellation and settlement | provider |
| workspace-tools | `nahida-tools` | Scoped filesystem tools | neutral tool contract |
| host | `nahida-cli` | Selection, parent grants, TUI/headless/RPC and lifecycle composition | configuration, provider, agent, workspace-tools |
| herdr-recognition | owning Herdr checkout | Native kind, foreground detection and agent API admission | native CLI/lifecycle contract |

The TUI and Herdr publisher consume host events. Neither belongs in the model or
agent layer. Nahida owns its default configuration home; sharing Pi's files is
an explicit `--config-dir` choice. Keep ordinary `--provider` and `--model`,
without a Pi application mode, runtime bridge or Node dependency. Do not put the
Herdr change in the read-only reference shelf.

## First-milestone acceptance

These M criteria supplement, not replace, issue #19 A1–A7 and its amendments.

- **M1 Configuration/provider:** the actual executable accepts the selected GLM
  record, including effective compat options and nullable thinking-level maps,
  without silently ignoring their meaning. Native requests, thinking/reasoning
  replay, streamed tools and final settlement match pinned Pi behavior. Selected
  unsupported efforts fail before HTTP. Synthetic broken-stream/error cases
  cannot execute partial calls or produce a successful result. Metadata remains
  auth-free; errors/events do not disclose credential values.
- **M2 Parent authority:** the host accepts explicit parent tool/workspace grants.
  The demo grants scoped read/write/edit/search/list capabilities and denies bash.
  Filter advertised schemas and enforce the same grant at dispatch, including
  forged unavailable tool calls. The model cannot broaden its own authority.
  No child-only human prompt silently substitutes for the parent's policy.
- **M3 Native TUI:** real foreground Nahida provides multiline prompt editing,
  scrolling/streamed Markdown, thinking and tool-result rendering, selected
  model/status display and usable interrupt/approval handling. Demonstrate
  terminal restoration on errors/exit and native behavior under a PTY; plain
  JSON, a status adapter or screenshots without behavior are insufficient.
- **M4 Herdr control:** identify the process as Nahida, launch/name it, and use
  genuine `agent.prompt`, observation/wait and cancellation against that native
  foreground process. Publish accurate working/blocked/idle transitions and
  release identity on exit. No Python input/status proxy or Pi identity. Idle
  alone does not acknowledge success: tie observed output/artifact to the task.
- **M5 Real journey:** with eligible existing configuration, the current parent
  calls Nahida through Herdr; GLM performs a real scoped file task and exact bytes
  are checked. A follow-up proves retained context. Cancellation interrupts the
  active turn without executing partial calls, claiming success or repeating side
  effects; a subsequent task proves continued usability. Do not promise rollback
  of already completed writes. Keep the native session/pane running for the owner.
- **M6 Verification/handoff:** owning gates and fresh independent review pass at
  a stable source/runtime identity. Record binary/config-policy/source identity,
  commands, outcomes and remaining blockers without secrets. macOS success is
  not Linux proof or completed Claude Code/Codex-specific acceptance.

## Ordered delivery slices

The issue is the portable task ledger; local progress/evidence may use the
owner-selected workstation `.tmp/tasks/nahida-native-milestone/`. It is not a
cross-device prerequisite or a replacement for this owning plan. Every slice
gets focused tests, owning gates and its required independent verification;
no slice below is implemented merely because the plan is recorded.

| Slice | Observable result / check | Depends on |
| --- | --- | --- |
| S1 GLM configuration | Port actual loader/overlay semantics for the selected record; credential-free native `--describe` succeeds; fixtures cover malformed/unsupported records and unchanged legacy selection. | none |
| S2 GLM request compatibility | Port effective request fields and nullable effort mapping; fixture captures exact requests and rejects unsupported efforts before HTTP. | S1 |
| S3 GLM stream/replay | Port thinking/tool deltas, exact reasoning replay and settlement; actual binary completes synthetic write/final-answer and broken-stream/cancellation cases without false success. | S2 |
| S4 Parent-granted tools | Scope schemas and dispatch identically; prove denial of bash and forged unavailable calls, workspace confinement and no policy elevation from prompts. | S3 |
| S5 Native TUI turn | Interactive PTY exercises editing, streaming transcript/status/tool results, interrupt, follow-up and terminal cleanup on a synthetic endpoint. | S4 |
| S6 Genuine Herdr recognition | Prepare a writable owning checkout; add/test Nahida kind, executable detection and prompt admission/lifecycle integration on a named pinned Herdr baseline. | S5 host contract |
| S7 Native Herdr fixture journey | Real foreground binary, synthetic provider, agent API prompt/wait/cancel, artifact and follow-up; verify no proxy/impersonation and no unrelated session changes. | S5, S6 |
| S8 Eligible live GLM journey | Independently reviewed source and existing eligible configuration pass M5; retain session and exact nonsecret evidence. Block if eligibility is unresolved. | S7, eligibility evidence |

S1–S3 are separate checkpoints, not one provider rewrite. Further split any slice
that exceeds a focused session. Preserve existing providers and the neutral loop.
The milestone's first demonstration uses the current parent, not actual Claude
Code and Codex. Their own acceptance and broader one-shot/RPC integration follow
as separate slices; the headless destination is not waived.

## Approval and stop boundaries

- Existing eligible configuration only; **no provider, credential or billing
  fallback**. A key or successful Pi call does not establish custom-client
  Coding Plan eligibility. Owner request covers the bounded GLM demonstration,
  not unrestricted inference, login, credential inspection or new paid accounts.
  Establish eligibility evidence before the live step and agree any missing
  operational limits before execution. The application may resolve the selected
  credential for the approved run; do not dump/copy real keys into task records.
- No bash in the first demo. Tool-level scope is not macOS OS isolation; neither
  configuration compatibility nor disabling one tool settles the original
  credential/storage safety contract. Do not advertise a sandbox that is absent.
- No shared Herdr restart, owner-pane close, release, deployment, root push or
  global parent configuration edit is implied. Use an isolated authorized test
  instance for Herdr changes; if unavailable, ask rather than disrupting sessions.
- No disk restart/resume prerequisite for this milestone: use an explicitly
  non-persisting session and retain in-memory context. Existing disk-session
  behavior is not removed. ADR-0002 replay/v2/storage work remains open.
- Linux remains owner-deferred. Responses, native OAuth refresh/locking and
  original registration/identity requirements remain open. Full Pi parity needs
  a source-derived capability inventory; it is a destination, not a completed
  or fully decomposed roadmap. Drop-in JavaScript ecosystem compatibility is out.

## Gates and source provenance

From the owning Nahida checkout:

```bash
RUSTUP_AUTO_INSTALL=0 CARGO_NET_OFFLINE=true make validate
UV_LOCKED=1 UV_OFFLINE=1 UV_PYTHON_DOWNLOADS=never make tutorial-build
rumdl check docs/plans/2026-10-10-native-agent-milestone.md \
  docs/plans/2026-10-10-pi-integration.md
git diff --check
```

These gates verify source/docs, not the future M1–M6 runtime journey. Each new
slice must supply that journey's own evidence. Herdr changes use its owning
instructions/gates once a writable checkout is prepared; refs are read-only.

- [Issue #19](https://github.com/azusachino/nahida/issues/19),
  [draft PR24](https://github.com/azusachino/nahida/pull/24) and the
  [configuration plan](2026-10-10-pi-integration.md) retain delivered/deferred scope.
- Pi installed version 1.1.0 supplied configuration/GLM source evidence; the
  observed reference `42a3497d03ad17e308a2299fa824727894f2c0ec` supplied headless
  interface evidence. They are not proven equivalent. Pin a reproducible upstream
  revision and source/test paths per ported slice, rather than trusting a moving
  or locally changed reference. Pi is MIT licensed, copyright Mario Zechner.
- [Herdr prompt guard](https://github.com/herdrdev/herdr/blob/7b116c05bfda646af39d2524c54e70c751f57ee8/src/app/api/agents.rs#L135-L215),
  [foreground identity](https://github.com/herdrdev/herdr/blob/7b116c05bfda646af39d2524c54e70c751f57ee8/src/app/agents.rs#L427-L446)
  and [kind lookup](https://github.com/herdrdev/herdr/blob/7b116c05bfda646af39d2524c54e70c751f57ee8/src/detect/mod.rs#L198-L227)
  are pinned to Herdr v0.9.3, not a claim of tested integration.
