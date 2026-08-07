# What's next

Everything in `AGENTS.md`'s concept map is now "done" except prompt caching
(still "partial" — it works on first-party Anthropic and silently no-ops on
any `Compat`-dialect gateway, which is how the dialect system in chapter 1 is
supposed to behave, not a bug to fix) and macOS sandboxing (deliberately
deferred — see below).

## Shipped: retry with backoff, reactive overflow recovery

Both came from reading `refs/pi` (`earendil-works/pi`) — a production-grade
agent toolkit with the same shape as `nahida` but thirty-plus provider
adapters, session persistence, telemetry, a client/server protocol. Almost
none of that was worth copying (that would be exactly the mistake
[chapter 0](00-why-nahida.md) is about not making); these two pieces were,
because they were gaps `nahida` genuinely had.

**Retry with backoff** (`nahida_llm::Error::is_retryable`, `Agent::max_retries`
/ `retry_base_delay_ms`) — on by default, unlike `compact_at`/`confirm`.
There's no scenario where "one transient failure ends the whole run" is what
anyone actually wants, so this doesn't need an opt-in the way a behavior
*change* would.

**Reactive overflow recovery** (`nahida_llm::Error::is_context_overflow`,
`Agent::recover_from_overflow`) — independent of [compaction](07-context-compaction.md)'s
proactive `compact_at` threshold. A real overflow is its own signal, so this
needs no threshold at all: on that specific error shape, `Agent::run` compacts
the transcript and retries the same turn exactly once. If it overflows again,
that error propagates for real — recovery is a safety net, not a loop.

Both are pure hardening: same observable behavior on success, fewer ways a
transient blip or a misjudged `compact_at` ends the whole session.

## Shipped: OS-level write confinement (Landlock, Linux only)

Found by reading a second reference, `refs/grok-build` (`xai-org/grok-build`)
— xAI's `grok` CLI, the first *Rust* reference in the catalog, which makes it
more directly comparable than `pi` was. Its `xai-grok-sandbox` crate applies
kernel-enforced isolation (Landlock on Linux, Seatbelt on macOS, via the
`nono` crate) once at process startup, plus per-subprocess seccomp network
blocking.

`nono` turned out not to be something to depend on blindly, though — the
ecosystem is three separately-versioned crates (`nono`, `nono-cli`,
`nono-rs`) with overlapping names, and `nono-rs`'s own docs call it "early
alpha... not undergone comprehensive security audit." Worse, macOS's half of
the story is actively risky, not just deprecated: `sandbox-exec` has no
documented replacement for headless process sandboxing, and there's a
current, real bug where a Seatbelt sandbox can block the
`com.apple.SystemConfiguration.configd` Mach service — which crashes any
Rust process using the `system-configuration` crate, which `reqwest`'s
default macOS features pull in. `nahida-llm` uses `reqwest` for every API
call. A naive Seatbelt sandbox risks breaking the exact thing the agent
needs to function, on the platform this project is actually developed and
run on.

So the shipped scope is narrower than "cross-platform OS sandboxing":
`nahida_tools::confine_writes` (`nahida-tools/src/os_sandbox.rs`), Linux
only, via the mature `landlock` crate directly (no `nono`), applied once at
`nahida-cli` startup. It denies write/create/delete/rename access anywhere
outside the workspace root — for this process and every child `bash`
spawns, with no API to lift it afterward — while leaving reads and execution
completely unrestricted. That's a scoped decision: confining reads too would
need a comprehensive allowlist of whatever paths the host distro's dynamic
linker, DNS resolution, and TLS trust store need, which varies enough
between distros (notably Nix, where almost everything lives under
content-addressed `/nix/store` paths rather than a fixed `/usr` layout) that
getting it wrong would break `bash` outright.

What this does *not* cover: reading a secret the model was never supposed to
see, or exfiltrating it over the network — [permission gating](08-permission-gating.md)
is the layer meant to catch a call before it runs at all, and gating plus
write confinement are complementary, not substitutes for each other. And
macOS gets none of this yet; it stays on gating alone until Seatbelt has a
real answer, or the `nono`/similar ecosystem matures enough to depend on.

## The tool-set roadmap that's already written down

`crates/nahida-tools/src/lib.rs`'s own doc comment names what's missing from
the current three tools (`read`, `write`, `bash`): `grep`, `glob`, a
staleness-checked `edit` (so an edit fails loudly if the file changed since
it was last read, instead of blindly overwriting like `write` does today),
and a *gated* `git push`.

That last one connects directly to [chapter 8](08-permission-gating.md):
`bash`'s permission gating is all-or-nothing today specifically because the
harness only sees an opaque command string — it can't tell a `grep` from a
`git push --force`. Splitting the genuinely dangerous actions into their own
typed tools is what makes gating *selective* instead of blocking every
single `bash` call, confirmation-fatiguing the one person actually reading
the prompts.

## If you want to keep going

The concept map in `AGENTS.md` is still the source of truth for what's
actually built versus planned — check it before trusting anything in this
tutorial that starts to feel stale, this page especially.
