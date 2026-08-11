# 1. The provider layer

**Where:** `crates/nahida-llm/`
**Knows:** the Anthropic Messages API — wire types, auth, streaming, dialects.
**Does not know:** what an agent is. There is no `Agent`, no `Tool`, no loop
anywhere in this crate. It only knows how to turn a `Request` into bytes on
the wire and bytes on the wire back into a `Response`.

## Why raw HTTP

Rust has no official Anthropic SDK. `nahida-llm`'s own doc comment calls this
out as a feature, not a gap: "for a project whose point is learning, that is
the feature — the wire format is visible instead of behind a generated
client." Every field you'll read in this chapter is something a generated
client would have hidden from you.

## The wire types encode constraints the compiler enforces

Open `crates/nahida-llm/src/types.rs`. The `Request` struct is what you'd
expect — `model`, `max_tokens`, `system`, `messages`, `tools` — but look at
what's *missing*: no `temperature`, no `top_p`, no `top_k`, no
`thinking: {budget_tokens: N}`. That's not an oversight. The file's own
opening comment explains:

> These mirror the JSON shapes exactly. Two things are deliberately *absent*,
> because sending them to `claude-opus-5` is a 400: `temperature`/`top_p`/
> `top_k` — removed on Opus 4.7 and later. `thinking: {type: "enabled",
> budget_tokens: N}` — removed on the same models. Depth is controlled by
> `Effort` instead.

The lesson generalizes: when a constraint can be enforced by a type not
existing, that beats enforcing it at runtime. A field you *could* set and
have the API reject is a mistake waiting for someone to make it, months from
now, without this comment in view. A field that doesn't compile is a mistake
that can't happen.

## `Client::from_env` — one function, three providers

```rust
pub fn from_env() -> Result<Self> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());

    let (auth, mut profile) = if let Some(key) = env("ANTHROPIC_API_KEY") {
        (Auth::ApiKey(key), anthropic_profile())
    } else if let Some(token) = env("ANTHROPIC_AUTH_TOKEN") {
        let oauth = env("ANTHROPIC_BASE_URL").is_none();
        (Auth::Bearer { token, oauth }, anthropic_profile())
    } else if let Some(key) = env("ZAI_API_KEY") {
        (Auth::Bearer { token: key, oauth: false }, zai_profile())
    } else {
        return Err(Error::NoCredentials);
    };
    // ...
}
```

First match wins, checked in that order. Two things worth noticing:

**There are two different auth schemes, and mixing them up is a silent
401.** An API key goes on the `x-api-key` header; a bearer token (OAuth, or a
gateway) goes on `Authorization: Bearer`. `client.rs`'s own comment flags
this directly: "an OAuth or gateway token sent as `x-api-key` is a 401, which
is a confusing way to learn this." The `Auth` enum exists specifically so
this can't be gotten wrong by construction — you can't accidentally build an
`ApiKey` from a bearer token, because they're different variants with
different shapes.

**A `Profile` carries everything downstream code needs to not care which
provider it's talking to** — base URL, default model, default max tokens,
and a `Dialect`. This is what lets `nahida-cli` never branch on "am I talking
to Anthropic or a gateway."

## `Dialect` — the same wire format, a smaller feature set

This is the one that actually bit this project, for real, mid-session — worth
reading closely rather than taking on faith.

```rust
pub enum Dialect {
    /// First-party. Everything in [`Request`] is understood.
    Anthropic,
    /// Core Messages API only.
    Compat,
}
```

A gateway like Z.ai's coding-plan endpoint speaks the same Messages API
shape, but doesn't understand `output_config.effort`, adaptive `thinking`, or
`cache_control` — fields that post-date the shape everyone cloned. Whether an
unknown field is silently ignored or causes a hard rejection depends on the
gateway, so `nahida` doesn't gamble on either — it strips:

```rust
fn adapt(self, req: &mut Request) {
    if self == Self::Anthropic {
        return;
    }
    req.output_config = None;
    req.thinking = None;
    for block in &mut req.system {
        block.cache_control = None;
    }
    for tool in &mut req.tools {
        tool.cache_control = None;
    }
    for message in &mut req.messages {
        for block in &mut message.content {
            if let ContentBlock::Text { cache_control, .. }
            | ContentBlock::ToolResult { cache_control, .. } = block
            {
                *cache_control = None;
            }
        }
    }
}
```

That last loop is a lesson in its own right, from [chapter 11](11-whats-next.md):
`cache_control` first existed only on `system`/`tools`, and this function
stripped it from both. When a second breakpoint was added on the growing
`messages` transcript, this function needed a third loop, or a `Compat`
gateway would suddenly start receiving a field it might reject — silently,
until someone hit exactly that gateway. Adding an Anthropic-only field
anywhere in `Request` means coming back here.

The dialect is inferred from the host — anything not ending in
`anthropic.com` is treated as `Compat` — and can be overridden with
`NAHIDA_DIALECT`. The practical consequence, confirmed the hard way while
building this project: if you're on a Z.ai (or any compatible-gateway) key,
`Agent::effort(...)`, `.show_thinking(true)`, and both prompt-cache
breakpoints (system prompt, and the moving one on `messages`) are all
**silent no-ops**. Not errors — the request still succeeds — just nothing
you asked for actually happens. If you ever wonder why a flag you passed
didn't change anything, check which dialect you're on before you check
anything else.

## What you should be able to answer now

- Why does `Request` have no `temperature` field, and what would happen if
  someone added one back?
- You're getting silent 401s. What's the first thing to check?
- You set `--effort high` and nothing changed. What's the first thing to
  check? *(If chapter 1 landed, you already know: which `Dialect` you're on.)*

Next: [streaming and accumulation](02-streaming-and-accumulation.md) — the
response comes back as a stream of bytes, not a JSON object. Turning that
into the `Response` this chapter's types describe is its own small, sharp
problem.
