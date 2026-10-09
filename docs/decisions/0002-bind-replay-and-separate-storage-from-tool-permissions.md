# ADR-0002: Bind replay and separate storage from tool permissions

## Status

Replay/session contract accepted by the owner on 2026-10-08. Linux storage
boundary remains Proposed under
[issue #19](https://github.com/azusachino/nahida/issues/19), not an implemented
format, approved production auth helper or claim of Linux renewable-auth support.
The owner selected a further guarded-process proof with fake stores, private IPC,
concurrent atomic refresh and cleanup. The Linux prerequisite was not split or
deferred; T04/T05 still wait for the remaining T02 proof. This acceptance record
does not implement either contract or change production confinement.

## Date

2026-10-08

## Context

[ADR-0001](0001-adopt-a-statically-composed-event-sourced-harness-spine.md)
chooses four statically wired crates and typed durable entries. The
[two-provider plan](../plans/2026-10-08-two-provider-support.md) needs that narrow
implementation before GLM reasoning and Responses items can survive restart.
The current `Message` has only role/content; constructing it from
`response.replayable()` cannot carry additional native response metadata.

At investigation baseline `3c1a6b5e0205d4aa71d189209805472da77e74a9`, session
format v1 neither checks versions nor binds a provider/model. Compaction replaces
memory but not the log. `run_once` persists an index-based tail that can become
invalid after compaction. These are T04/T05 work, not fixed by this ADR.

Linux has another prerequisite: `main` calls Landlock inside the default
`#[tokio::main]` body, after constructing a multi-thread runtime. The current
ruleset does not request thread synchronization. Landlock's default restricts
only the calling thread and its future descendants, not preexisting siblings.
The process-wide wording in `main.rs`, `os_sandbox.rs` and README is therefore
stronger than that API guarantees. This is a startup-order risk; it does not
prove that today's CLI actually schedules a particular tool on an unconfined
worker. Tokio's main future itself runs on the calling thread, not a worker.

Sessions are opened after confinement, normally outside `--root`. Fully confined
code cannot create session directories or atomically replace renewable credentials
there. Granting those directories to model-invoked bash would contradict A7.
A storage thread shares memory and descriptors with the runtime; a separate
helper process also needs a genuinely private control channel. Neither is safe
merely because its Rust handle is not a model tool.

## Decision

### Proposed replay and session contract

1. Keep protocol data in `nahida-llm`, transcript mutation in `nahida-agent`,
   confinement in `nahida-tools` and disk/terminal behavior in `nahida-cli`.
   No new crate or upward dependency.
2. Add optional native replay metadata to canonical `Message` and settled
   `Response`, separate from public text/tool content. Its typed envelope carries
   the exact registered provider name and requested model. The data is a tagged
   enum: GLM's exact `reasoning_content`, or Responses adapter-owned item types
   retaining IDs, namespace and opaque reasoning. Recheck concrete Responses
   item fields in T09/T10; do not introduce a general JSON extension bag.
   Existing Anthropic thinking blocks remain verbatim, but session binding also
   prevents their replay under a different owner/model.
3. A provider-neutral response-to-message operation preserves metadata without
   branches in the loop. The owning adapter validates the tag and owner before
   encoding. Foreign-owner data is an error, not silently dropped or converted.
   Request copies alone acquire cache annotations; stored messages stay unchanged.
4. Native data is sensitive replay state, not an authentication credential.
   Serialize it only into the protected session log and the matching provider
   request. Never render it in `AgentEvent`, `--json`, `--describe`, debug output
   or errors. Do not persist tokens, registration secrets, email or endpoint URLs
   in a session binding. A raw `Debug` derive on the new payload is not acceptable.
5. Introduce v2 JSONL with exactly three entry variants:
   - `Header`: version, existing session identity/time/root, provider and model.
   - `Append`: an ordered batch of canonical messages.
   - `Replace`: the complete authoritative replacement transcript after compaction.
   Projection starts empty, extends on append and replaces on compaction. There
   are no parent pointers, branches, entry IDs or generic configuration language.
   An assistant tool-call message and all its results form one settled append
   batch; do not expose a half-exchange as an inference-ready resumed projection.
6. Persistence observes settled transcript mutations, including compaction before
   a later failure/cancellation. Use a small provider-neutral mutation channel
   separate from serializable live events; never infer changes from old indices
   or vector length. T05 must define write-failure behavior and refuse inference
   from an invalid/unsettled projection. This is not an exactly-once guarantee
   for external tool effects across a process crash.
7. Require one v2 header at the start, supported version, matching root/provider/
   model and valid known entry shapes. Reject unknown versions, duplicate headers,
   foreign replay tags, invalid tool exchanges and corrupt records before append
   or inference. `--continue` does not silently select a different model/provider
   or skip a corrupt matching session. Diagnostics name the incompatibility,
   never the native payload.
8. v1 is read/import-only. Explicit import writes a new v2 log and leaves the
   original bytes untouched. Bind the new session to the explicitly chosen target,
   not an invented historical origin. Initially import only settled canonical
   history with no origin-sensitive thinking/signatures/native state; otherwise
   refuse with an actionable missing-provenance diagnostic. A later explicit
   redacted migration can be added if needed. Lost v1 reasoning or compaction
   history cannot be recovered. No implicit import on ordinary resume.
9. A torn or non-newline-terminated last record prevents append, even if a valid
   prefix can be inspected. Explicit recovery/import may copy a validated settled
   prefix into a new log, never append after corrupt bytes or rewrite the source.
   T05 owns fixtures for import/reject, compaction followed by failure, owner
   mismatch and `torn_tail_refuses_append`.

### Linux diagnostic result and stop point

The isolated `nahida-tools/tests/storage_boundary.rs` target is part of ordinary
`make check` on Linux. It is a diagnostic spike, not shipped storage code. Every
case runs in its own child; the unrestricted parent owns temporary directories
and cleanup, bounds execution to 15 seconds and reaps the probe. Fully enforced
Landlock is required: unsupported kernels fail, not skip or pretend success.
Non-Linux runs explicitly report that these probes were not exercised.

The probes distinguish these expected observations:

| Probe | What it establishes, if it passes on Linux |
| --- | --- |
| `outside_store_denied` | Confined workspace writes succeed, but outside session creation, temporary refresh-file creation and rename fail; old fake state remains intact |
| `runtime_before_confinement` | A warmed preexisting Tokio worker can spawn a shell that writes outside the caller's confined root |
| `runtime_after_confinement` | New Tokio workers, their blocking filesystem work, new native threads and shell descendants inherit denial when the caller confines first |
| `preopened_session` | A preopened append handle remains usable; exec does not inherit it and shell reopening by path or procfs is denied |
| `private_pipe_reachable` | A same-domain shell can inject inert bytes through the parent's procfs control-pipe descriptor despite close-on-exec; a naive helper pipe is not private |

These are assertions about specific kernel/topology behavior, not results claimed
from a macOS run. The owning PR must carry the exact Linux CI revision/run and
outputs before using them as runtime evidence. No real credentials, refresh
request, login, host changes or sysctl changes occur.

**Bounded blocker:** no production helper or credential/session-format
implementation is approved by this spike. Concurrent refresh, logout/refresh
races, crash-safe atomic rotation, helper privacy and cleanup are still unproved.
T02 returns this stop point rather than claiming a safe small auth boundary.
T07 remains blocked. Replay-only work may proceed after the owner accepts that
part of this proposal and explicitly splits the Linux prerequisite.

A small next investigation can test a separate pre-confinement storage process
with fixed verbs, immutable store roots, bounded frames/timeouts and no arbitrary
paths/commands. It must protect the trusted parent's control descriptors against
procfs reopening and ptrace, without assuming a distro's Yama setting. Disabling
process dumpability is one candidate, not an approved dependency or architecture;
its debugging/lifecycle implications need review. Close-on-exec is necessary,
not sufficient. Refresh locking must span reload → fake refresh → atomic paired
replacement, coordinate independent processes, and reject stale generations
following logout. EOF/cancellation must close and reap task-owned helpers.

Do not silently add storage roots to model-tool permissions, disable Landlock,
use a shared unconfined Tokio pool or assume a privileged helper thread is isolated.
A separately approved narrower Linux auth scope is an alternative; it would not
satisfy the original two-provider A3/A7 acceptance. The current production startup
ordering/session-write conflict remains unresolved and must be tracked until fixed.

## Alternatives considered

### Native data as public content or an untyped JSON bag

This makes foreign fields easy to send to Anthropic and opaque data easy to print.
A typed, owner-bound side field gives the loop one neutral preservation operation.
Protocol adapters own wire-specific validation.

### Guess legacy provenance or rewrite v1 in place

A transcript cannot establish the missing provider/model of native data. An
explicit new log preserves the original and makes the target binding deliberate.
Rejecting unverifiable native history is safer than replaying a fabricated origin.

### Detect compaction from length or persist a summary index

Length can shrink or remain equal, and a later failure still needs the replacement.
An explicit full replacement entry has an unambiguous projection without a tree.

### Widen Landlock, keep an unconfined pool or rely on private descriptors

Widening writes exposes the same paths to tools. An unconfined pool breaks
inheritance. Preopened regular-file append handles can solve a narrower session
case but cannot create/rename rotating credentials. Anonymous control descriptors
need additional protection; their lack of a named socket path is not that proof.

## Consequences

- The intended durable contract is small, but version enforcement and provenance
  rejection are visible changes requiring explicit acceptance before T04/T05.
- Existing Anthropic/global GLM behavior stays supported; no provider-specific
  branch enters the agent loop. Compaction/persistence remains a T05 change.
- Linux runtime evidence is required before any auth safety claim. Passing these
  negative probes means the hazards were reproduced, not that they were repaired.
- Unrestricted reads/network remain the existing threat-model limitation. Unix
  owner-only permissions are not agent-secret isolation. Native replay makes
  session logs sensitive; protect them without promising protection from same-user
  approved bash reads.
- No production startup, sandbox behavior, session version, credential store or
  provider route changes in this investigation. Linux auth needs an owner decision
  and a separately reviewed small boundary proof, or stays unsupported.

## Sources

- Nahida baseline above: `crates/nahida-cli/src/{main,session}.rs`,
  `crates/nahida-agent/src/agent.rs`, `crates/nahida-llm/src/types.rs`,
  `crates/nahida-tools/src/os_sandbox.rs`; issue #19 and the owning plan.
- [Kernel Landlock documentation](https://docs.kernel.org/userspace-api/landlock.html):
  inheritance, ptrace restrictions, descriptor rights and special filesystems.
  Ptrace restrictions exist between domains; this is not a claim that Landlock
  provides no ptrace protection. Same-process shared state is not isolated.
- [landlock_restrict_self(2)](https://man7.org/linux/man-pages/man2/landlock_restrict_self.2.html):
  default per-thread enforcement; thread synchronization requires ABI 8.
- [landlock 0.4.7 source](https://docs.rs/landlock/0.4.7/src/landlock/ruleset.rs.html):
  `all_threads(false)` is the default. Nahida's ABI V5 filesystem access profile
  alone is not the reason thread synchronization is absent: it never requests it.
- [Tokio main macro](https://docs.rs/tokio/1.53.1/tokio/attr.main.html):
  default multi-thread builder precedes the body; the main future is not a worker.

## Investigation verification

Fresh independent peer `nahida-t02-verify` used Pi
`zai-coding-cn/glm-5.3-flash`, low, with runtime identity confirmed and no fallback.
It reviewed commit `d4fa18e9945dc6ad74588852bb8f6dbd2b01b3dc` against
`3c1a6b5e0205d4aa71d189209805472da77e74a9`. C1–C5 met the bounded investigation
criteria: replay proposal, durability proposal, hazard reproduction, honest safety
stop and owning gates. **The original T02 safe-storage boundary remained unmet.**
No production code changed; the new diagnostic target is executable test source.

Performed independently on macOS: offline `make validate` (including check and
release), strict locked/offline `make tutorial-build`, the focused probe target,
`rumdl check` and Git scope/diff checks, all exit 0. The focused macOS target
explicitly reported Linux probes not exercised. Existing 120 tests passed;
the two paid evals remained ignored.

The peer independently fetched and checked
[Linux CI run 37777648557, job 113312439605](https://github.com/azusachino/nahida/actions/runs/37777648557/job/113312439605)
at the exact reviewed head: Ubuntu 24.04.5, stable Rust 1.99.0, actual `make check`
job successful and all five probe completions present. This is reused Linux CI
evidence, not a Linux rerun on macOS. It verifies the expected unsafe/denied cases,
not concurrent refresh, protected IPC or a credential helper.

Findings retained: keep this ADR Proposed until owner acceptance; correct the
existing process-wide README/source wording with the eventual startup-order fix.
The delivery remains [draft PR #23](https://github.com/azusachino/nahida/pull/23).
Owner decisions and the unproved Linux auth boundary still block dependent work;
A3/A6/A7 are not completed by this receipt.
