# Slice 3: durable sessions and continuation

Status: locally verified; ready for PR review.

## Checkpoint and scope

Resume of the TypeScript rework, criteria 9–10a in
[the spec](../specs/ts-rework.md). PRs #25 and #26 are merged. The previous
checkout was `docs/slice3-carryover` at
`6a0520424c62224a6ee204b4daa5b452d81c41c9`; its tree matches merged main at
`c6f77e7a82fabe11730f3ce61132e611ca8b2bc5`. Implementation starts from that
main revision on `feat/sessions`. One writer, this session. Workstation-task
playbook, tier 1. No dependency upgrade, live inference, release or installation.

Own `src/session.ts`, the CLI/pack context integration, offline tests and the
README/spec/instruction updates needed for this slice. Gates: `make check` before commit,
`make validate` before PR. Verification is self-review plus automated tests.

## Design and evidence boundary

Installed pi packages are exactly 1.2.0. Read their agent-core README,
`dist/agent.d.ts`, `dist/agent-loop.js`, and coding-agent SDK, session-format,
message-types and sessions docs plus `dist/core/session-manager.{js,d.ts}`.
The persistence API belongs to **pi-coding-agent's SessionManager**, rather
than pi-agent-core. Use it directly without loading pi's coding tools or UI.

- Store Nahida sessions under its config root, grouped by canonical workspace.
  Keep them separate from pi's own sessions. `--describe` shows the directory.
- Persist the system/tool baseline and finalized messages. Before every model
  request, rebuild messages from SessionManager. Pack-provided context uses a
  host callback that queues passages, then writes them to the same log after
  pending tool results and before they can enter a request. Context that is never
  used by a request is discarded at run end.
- `--continue` loads the latest log in that workspace. With a prompt it appends
  user input; without one it retries unfinished user input or continues after
  tool results. A completed assistant tail requires a new prompt.
- Refuse model or loadout/prompt changes on continuation instead of silently
  projecting history under different instructions or capabilities.
- Pi's loader tolerates malformed lines and can append a missing newline.
  Validate the complete Nahida log before opening it; reject corruption without
  modifying it. Do not fall back to an older log when the newest is corrupt.
- Persist successful assistant tool calls before execution. Failed, aborted,
  pending, deferred or length-limited responses never execute tools or become
  successful answers. Failure metadata stays outside model context.
- A handled SIGINT waits for tool settlement and records results. A crash can
  leave a complete tool request without all results: refuse continuation in
  that case, because whether side effects happened is unknowable. Never rerun
  the tool automatically or claim exactly-once execution after arbitrary crashes.
- Hold an exclusive workspace lock through activation, requests and disposal.
  A hard crash leaves a lock requiring owner inspection/removal; automatic stale
  lock recovery and filesystem power-loss guarantees are outside this slice.

## Acceptance checks

Offline fixture tests compare captured requests to log replay across restarts,
including prompt/tool declarations, tool results, opaque reasoning signatures
and pack context. Cover workspace isolation, model/loadout mismatch, corrupt
JSON and broken entry chains, no-session continuation, completed-tail behavior,
failed/partial streams, SIGINT during requests and side-effecting tools, and
unresolved calls after a crash. All auth/config and session files stay under
`.tmp/` in tests. Existing CLI behavior and disposal checks remain required.

## Verification and handoff

The implementation covers criteria 9–10a with offline fixtures. Requests are
compared to log replay before the provider responds, including opaque reasoning
signatures and tool results. Tests also cover context added during tool
execution: it follows the result and survives the next restart.

Final acceptance on `feat/sessions`:

- `make check`: passed TypeScript, Biome and the then-current 38 tests.
- Focused context-ordering regression: passed after adding that case.
- `make validate`: passed TypeScript, Biome, all 39 tests (183 assertions), Bun
  CLI build and the built executable's no-argument usage/exit-code smoke check.
- `git diff --check`: passed.

The initial implementation gate stopped at a TypeScript narrowing error in the
log validator. Explicit role checks fixed it; the later gates above passed.
Verification is automated fixture testing plus self-review, not independent
agent review or live-provider testing. All provider calls were injected and all
credential/config/session fixtures lived under `.tmp/`. No dependency versions,
real credentials, user configuration or installed executable were changed.

Changed paths: `src/session.ts`, `src/cli.ts`, `src/pack.ts`, `src/cli.test.ts`,
`README.md`, `AGENTS.md`, `docs/specs/ts-rework.md` and this record. Workstation
root unchanged. No implementation blocker remains. Owner PR review is pending;
merge, release and installation are outside this delivery.

Next: after this slice is accepted, implement slice 4's writing pack against
criterion 11. Keep workspace path confinement and targeted-edit failure behavior
in that slice. The REPL, reading/science packs and general crash recovery remain
outside the session slice. See the design section above for stale locks,
unresolved side effects and power-loss limits.

