# Plan: native Rust compatibility with Pi configuration and auth

Date: 2026-10-10. Status: native direction authorized; T02 proof first.

## Outcome and owner decisions

Nahida remains a native Rust agent. It recognizes Pi's `models.json` and
`auth.json` for the supported providers and refreshes supported OAuth
credentials in Rust. No Node runtime bridge, Pi SDK sidecar, or replacement
agent loop is requested. Preserve existing Anthropic and Z.AI behavior.

The owner selected native Rust, Rust refresh, T02-first sequencing and
reconciliation through issue #19 and Asobi. The earlier bridge proposal was
rejected. A later request to reconsider followed the erroneous claim that
Pi's file-backed auth locking was only in-process; that claim is withdrawn
and supplies no bridge approval. The
[source review](../research/2026-10-10-pi-layers-and-auth.md#review-correction-file-storage-and-native-rust)
records the actual cross-process file locking.

This plan extends [issue #19](https://github.com/azusachino/nahida/issues/19)
and the [two-provider plan](2026-10-08-two-provider-support.md). It does not
remove their acceptance criteria. In particular, importing a Pi OAuth record
is not proof of the original A3 registration, identity validation or independent
account workflow. Those criteria remain open until implemented and verified.
[ADR-0002](../decisions/0002-bind-replay-and-separate-storage-from-tool-permissions.md)
still governs accepted replay/v2 and the proposed Linux storage boundary.

## Scope and boundaries

- `nahida-llm` owns provider configuration types, supported wire APIs, OAuth
  protocol/refresh, model capabilities and replay ownership.
- `nahida-cli` is today's composition root for `nahida`, explicit config paths,
  storage lifecycle and terminal presentation. A future non-CLI host is a
  separate consumer, not justification to add a crate now.
- `nahida-agent` stays provider-neutral and has no Pi file, OAuth, process or
  Herdr dependency. `nahida-tools` owns confinement; diagnostic storage code is
  test-only, not a shipped helper.
- Herdr remains separate outer host/control work, outside this plan.
- Initial compatibility targets existing Anthropic/OpenAI-completions paths
  and the planned OpenAI Responses/ChatGPT path. Do not claim that parsing Pi
  JSON implements all Pi providers or extensions.
- No login, real credential inspection, live inference, deployment, release or
  confinement widening is authorized by these offline slices.

## File and credential contract

Pin compatibility evidence to Pi v1.1.0. Read source and synthetic fixtures,
not the owner's actual credential files. Use an explicit user-selected Pi
agent directory when the feature is eventually wired; model tools cannot
choose credential paths or storage operations.

`models.json` describes compatible endpoints and model overrides, not the
entire built-in catalog, executable provider extensions, or default-model
settings. Name supported fields/APIs. Fail clearly on unsupported selected
behavior instead of ignoring it or silently substituting another provider.
Do not execute `!command` credential sources in the initial implementation;
supporting command execution requires a separately reviewed trusted-host path.

`auth.json` is version-coupled storage, not a stable public SDK contract. The
Rust integration must validate selected credential shapes, preserve unrelated
providers and unknown fields during supported updates, and never dump input
JSON, tokens or OAuth response bodies into errors/logs/events/sessions.
Stored OAuth selects OAuth: a failed refresh must not fall back to API billing.

Pi's file backend uses `proper-lockfile` 4.1.2: atomic mkdir of a path-based
`auth.json.lock` directory, mtime heartbeat/stale detection, compromised-lock
checks, then locked reload, mutation, merge and persistence. A Rust-only mutex,
`flock`, or another lock filename does not coordinate with Pi. Test the concrete
protocol, including synchronous Pi readers' stale policy, before shared writes.

Refresh rechecks expiry and credential type under the shared lock. Another
writer's completed refresh or logout must be respected. Once the provider may
have rotated a token, cancellation must not discard its successful replacement;
bound and complete persistence while allowing the caller to stop waiting.

Cross-process locking and crash-safe writes are distinct. Pi writes directly
with `writeFileSync`, so matching its lock does not prove crash-safe replacement.
Rust atomic replacement, permissions/symlink checks and Pi cache invalidation
need their own fixtures and interoperability tests. Do not claim the shared
file is crash-safe against an independently crashing Pi writer.

## Security boundary and retained stop points

The existing Landlock implementation permits reads everywhere and confines
writes only. It does not hide `auth.json` from approved bash reads. No Node
bridge or JSON parser solves this. Preserve the existing threat-model limitation
and keep any stronger secret-read isolation requirement explicitly unmet until
an accepted design proves it; do not silently lower issue #19 A7.

The production CLI constructs Tokio before applying Landlock. Existing T02
probes reproduce hazards in startup inheritance, outside-root session/refresh
writes and naive pipe reachability. Passing those probes means the hazards
were reproduced, not repaired. T02 remains open and production auth blocked.

The next proof is test-only: a native pre-confinement storage process with
fixed roots/verbs, guarded parent descriptors, bounded private IPC, fake
concurrent refresh/logout and cleanup. No arbitrary-command/path broker, shared
unconfined Tokio pool, changed sysctl or model-tool credential-directory grant.
Disabling dumpability is a candidate to test for descriptor/ptrace protection,
not a production decision. Production startup/session changes need a reviewed
boundary and owning Linux evidence before dependent auth slices.

## Added acceptance criteria

These extend, rather than replace, A1–A7 in issue #19.

- **N1 Native/layering:** no Node/Pi runtime dependency or bridge; the Rust loop
  stays neutral and existing providers remain selectable.
- **N2 Config compatibility:** pinned synthetic `models.json` fixtures cover
  supported APIs/overrides, explicit identities, invalid inputs and unsupported
  selected behavior. Metadata inspection performs no auth read, refresh,
  command execution or HTTP. No whole-Pi catalog/extension parity claim.
- **N3 Auth compatibility:** selected API-key/OAuth shapes are fixture-tested;
  supported updates preserve other providers/fields. Secrets never reach model
  messages, sessions, events, ordinary logs, errors or tool environments. Failed
  OAuth never silently falls back to API billing.
- **N4 Shared refresh:** a Rust writer and the concrete pinned Pi file backend
  contend on the same synthetic file. Prove one effective refresh, locked
  reload/merge, logout ordering, heartbeat/stale/compromised-lock behavior,
  cancellation-safe persistence and unrelated-provider preservation. A fake
  store or Rust-only lock test is not sufficient interoperability evidence.
- **N5 Linux/storage:** actual owning Linux CI exercises the guarded process,
  private bounded IPC, confined tool writes, refresh/logout/crash and owned
  cleanup. Resolve or explicitly retain startup/session-write and read-isolation
  blockers. Neither diagnostic success nor file locking completes A3/A7.
- **N6 Gates/live scope:** focused tests, `make check`, `make validate`, strict
  tutorial build and fresh independent verification at the reviewed revision.
  Live Coding Plan calls still require custom-client eligibility evidence and
  specific owner approval; live ChatGPT use also requires separate approval.

## Ordered slices

One behavior per slice, one writer per checkout. Asobi
`nahida:two-provider-support:task-4` owns remaining T02 work; issue #19 owns
acceptance. Keep the existing task dependencies and replay contract.

### S0 — Reconcile acceptance before code

Record native scope/N1–N6 in issue #19 without deleting A1–A7. Retain T02 first
and dependent T04/T05/T07 blockers. Remove bridge/runtime requirements from
current execution; Node may be a pinned interoperability test tool later, not
an application dependency. Preserve existing reviewed decisions and history.

### S1 — Prove guarded descriptors and lifetime (T02, test-only)

Extend the isolated Linux probes, retaining the unguarded positive control.
Test non-dumpable trusted processes against confined shell procfs reopening and
ptrace, including a parent attaching its child so distro Yama policy alone is
not mistaken for protection. Trusted fake traffic still works; EOF/cancellation
closes and reaps task-owned helpers under a watchdog. No production changes.

Exit: focused tests and owning Linux CI at exact revision; independent review.
This is a partial T02 result, not a safe credential-storage implementation.

### S2 — Prove fake storage transactions (remaining T02)

Fixed roots/verbs and bounded frames. Independent fake writers serialize reload,
refresh and atomic paired replacement; logout prevents stale refresh resurrection.
Test truncated/oversized/malformed frames, cancellation, crash, write failure,
cleanup and unchanged outside-root tool denial. No real tokens or network.

Exit: Linux proof and accepted small boundary, or a bounded owner-visible blocker.
Do not implement production helper/storage or unblock T04/T05/T07 automatically.

### S3 — Prove Pi storage interoperability

Implement a test-only Rust protocol sketch against the actual pinned Pi file
backend using synthetic records. Cover N4 and atomic-write/cache behavior.
Provision any test-only Node/Pi packages through an explicit pinned test setup;
no machine-specific committed paths or production sidecar. Source inspection
alone is not the runtime proof. Reconcile concrete findings with T02/A3/A7.

### S4 — Implement native config and provider integration

After prerequisites pass, implement N2/N3 in existing Rust modules. Preserve
explicit provider selection and old environment behavior. Resume T04/T05's
accepted replay/durability work and provider-specific offline wire tests.
The original account/login acceptance remains distinct from Pi credential reuse.

### S5 — Implement native refresh and finish offline acceptance

After reviewed storage and interoperability proof, wire Rust refresh through
the accepted bounded storage lifecycle. Exercise N4/N5 end to end with fake
services. Run owning gates and fresh independent verification. Record remaining
original A3/A4 and external eligibility/live gates honestly; no premature general
ChatGPT/Coding Plan support claim.

## Sources

- [Pi source/storage research](../research/2026-10-10-pi-layers-and-auth.md).
- [Two-provider plan](2026-10-08-two-provider-support.md),
  [ADR-0002](../decisions/0002-bind-replay-and-separate-storage-from-tool-permissions.md)
  and [issue #19](https://github.com/azusachino/nahida/issues/19).
- Owning `AGENTS.md`, current storage probes and `os_sandbox.rs` govern code and
  gates. There is no inherited authorization for Herdr, releases or deployment.
