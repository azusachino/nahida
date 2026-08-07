# What's next

Everything in `AGENTS.md`'s concept map is now "done" except prompt caching
(still "partial" — it works on first-party Anthropic and silently no-ops on
any `Compat`-dialect gateway, which is how the dialect system in chapter 1 is
supposed to behave, not a bug to fix). Two gaps surfaced by comparing this
project against a much larger one, plus the tool-set roadmap this project's
own code already names.

## Two gaps, found by reading `refs/pi`

`refs/pi` (`earendil-works/pi`) is a production-grade agent toolkit with the
same shape as `nahida` — a provider layer, a loop, tools, a terminal front
end — but thirty-plus provider adapters, session persistence, telemetry, a
client/server protocol. Almost none of that is worth copying into `nahida`;
copying it would be exactly the mistake [chapter 0](00-why-nahida.md) is
about not making. Two pieces of it *are* worth borrowing as concepts, though,
because they're gaps `nahida` genuinely has:

**Retry with backoff.** `nahida` has none today — any transient network
error or a provider's `5xx` kills the whole `Agent::run()` outright. `pi`'s
`packages/ai/src/utils/retry.ts` classifies an error as retryable
(rate-limits, `5xx`, transport failures, premature stream endings) versus
non-retryable (quota/billing exhaustion — retrying won't help, fail fast),
then applies bounded exponential backoff only to the retryable class. The
concept ports cleanly onto `nahida-llm`'s existing `Error` enum
(`Api{status,..}` / `Transport` / `Stream`); classification would be simpler
than `pi`'s, since there are two providers here, not thirty.

**Reactive overflow detection.** `nahida`'s compaction (chapter 7) is
proactive — a token threshold you set upfront. If it's unset, wrong, or a
smaller-context model is in play, a real overflow just surfaces as a hard
`AgentError::Llm` and ends the run. `pi`'s `packages/ai/src/utils/overflow.ts`
parses provider error messages for context-overflow shapes (Anthropic's
`"prompt is too long: X tokens > Y maximum"`, and eleven other providers'
equivalents) so it can catch that specific failure and attempt one bounded
compact-and-retry instead of dying. A good hardening pass on top of
chapter 7, and its own issue.

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
