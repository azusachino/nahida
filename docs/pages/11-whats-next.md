# What's next

Everything in `AGENTS.md`'s concept map is now "done" except macOS
sandboxing (deliberately deferred — see below).

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

## Shipped: a staleness-checked `edit`

`write.rs`'s own doc comment used to name this as "the natural first
extension" — a blind whole-file overwrite has no way to know whether what
it's replacing is what the model actually saw. `Edit`
(`crates/nahida-tools/src/edit.rs`) is a targeted `old_string` →
`new_string` replacement instead, and the staleness guarantee falls out of
that for free: if the file changed enough that `old_string` no longer
matches — or now matches more than once — the edit refuses rather than
guessing, so it's a stronger check than snapshotting "the file looked like X
when I read it" would give, at no extra implementation cost.

Building it surfaced a real bug in [`Sandbox::resolve`](05-sandboxing.md),
not a new one it introduced: resolving a path that *already exists in full*
— the ordinary case for both `read` and `edit`, not an edge case — left an
empty `tail` after `strip_prefix`, and `real.join(tail)` on an empty tail
still adds a trailing separator rather than being a no-op. That turns a
plain file's path into one `open(2)` refuses with `ENOTDIR`. No prior test
had ever resolved a path that already fully existed as a file — `edit`'s
tests were the first to, which is exactly the value of writing the tests
before trusting the code: this failed loudly rather than silently.

## Shipped: the second cache breakpoint

`Agent::system()`'s breakpoint (chapter 1) only ever covered the *static*
prefix — tools and system, since caching is a prefix match and render order
is tools → system → messages. What it never touched was the part that
actually dominates cost in an agentic loop: `messages` itself, which grows
every turn and was rebuilt from scratch, with zero cache annotation, on
every single request. For a multi-turn REPL session or any tool-calling
sequence, that meant reprocessing the entire prior conversation at full
price on every turn, even though each turn's `messages` is just the last
turn's `messages` plus a few new blocks.

The fix is a *moving* breakpoint: `Agent::request()` marks the last content
block of the last message on every outgoing request
(`ContentBlock::mark_cached`, `nahida-llm/src/types.rs`). `transcript`
itself is never mutated with `cache_control` — only the per-request clone
is — so each turn's prefix, up to wherever the *previous* turn's breakpoint
landed, is byte-identical to what got cached last time and is served from
cache; only the newly appended tail costs full price, and a fresh breakpoint
lands on the new tail's end for the next turn to reuse.

This only needed the field on two of `ContentBlock`'s five variants —
`Text` and `ToolResult` — because `request()` is only ever called with
`transcript` ending in a user turn (the initial prompt, or a batch of tool
results); it never runs mid-assistant-turn, so `Thinking` and `ToolUse`
never appear as the last block going out. Discovering that invariant was
also what made the *test* for this straightforward instead of speculative:
`tests/agent_loop.rs`'s `the_cache_breakpoint_moves_to_the_newest_message_each_turn`
scripts a tool-use turn followed by a text turn, and asserts the breakpoint
is on turn 1's only message, then has moved off it — not lingering — by
turn 2.

One thing this reopened: [chapter 1](01-the-provider-layer.md)'s
`Dialect::adapt` already stripped `cache_control` from `system` and `tools`
for a `Compat` gateway, specifically because an unknown field can cause a
hard rejection depending on the gateway. Adding `cache_control` to
`ContentBlock` without teaching `adapt` to strip it there too would have
quietly reintroduced the exact bug that method exists to prevent — so it
now strips it from `messages` as well. The test harness itself needed a
small addition to catch this class of mistake: `FakeProvider` hardcoded
`Compat` dialect, which would have stripped the field before any test could
ever observe it, so `FakeProvider::start_with_dialect` exists now for the
one test that specifically needs `Anthropic` dialect to see the breakpoint
survive to the wire.

## The tool-set roadmap that's still open

`crates/nahida-tools/src/lib.rs`'s own doc comment names what's left:
`grep`, `glob`, and a *gated* `git push`.

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
