!!! note "Outline — not yet expanded to full prose"
    This chapter is scaffolded with the real shape and the real traps. Ask
    for it to be written in full when you're ready to go deeper than the
    outline.

# 9. Evals

**Where:** `crates/nahida-cli/tests/evals.rs`, `make eval`

Chapter 6's tests prove the loop is *mechanically* correct against a fake
provider. That says nothing about whether the agent actually gets a real
task right — which only a real model, talking to a real provider, can
answer.

## What this chapter will cover

- **Every eval is `#[ignore]`d, on purpose.** `cargo test --workspace`
  (what `make check` and CI run) still *compiles* `evals.rs`, so a broken
  eval fails the build the same as broken code — but nothing executes
  without `--ignored`, which only `make eval` passes. These tests cost real
  tokens and money; CI must never spend money on every push.
- **`run_eval(prompt, setup, check)`** is the whole harness: `setup` seeds a
  scratch `tempfile::tempdir()` as the workspace, the real `Agent` +
  `Client::from_env()` + `nahida_tools::default_set()` stack runs the
  prompt against it, `check` inspects the result afterward. Checks stay as
  simple as "does the file have the expected content" on purpose — that
  catches a real regression without inventing a task-description format
  nobody has needed yet.
- **A structural reason this lives in `nahida-cli`, not `nahida-agent`**:
  evals need the full stack (real client, real tools, real sandbox), and
  `nahida-cli` is the only crate that already depends on all three without
  creating a new dependency edge. It's also a bin-only crate (no `[lib]`),
  so the eval harness can't reach into `main.rs`'s private modules — it's
  self-contained rather than reusing, say, `render.rs`'s formatting.
- **What was deliberately *not* built**: no task-description mini-language,
  no aggregated pass/fail reporting across many tasks or models. `refs/pi`
  has a much larger eval package (`packages/evals`, with its own reporter
  and cross-harness comparison table) — worth a look for what that grows
  into, but over-built for two tasks.

Next: [the CLI](10-the-cli.md) — the crate that's allowed to know all of
this exists.
