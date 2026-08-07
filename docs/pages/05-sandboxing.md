!!! note "Outline — not yet expanded to full prose"
    This chapter is scaffolded with the real shape and the real traps. Ask
    for it to be written in full when you're ready to go deeper than the
    outline.

# 5. Sandboxing

**Where:** `crates/nahida-tools/src/sandbox.rs`

This is the one place in the whole codebase where the "untrusted input"
framing from a normal web app applies almost unchanged: "a path from the
model is untrusted input in exactly the way user input is" (`AGENTS.md`,
rule 4 for agents working on this repo).

## What this chapter will cover

- **`Sandbox::resolve` rejects `..` outright rather than normalizing it.**
  The file's own comment explains why normalizing isn't enough:
  `a/../../etc/passwd` collapses to something outside the root, and a
  symlink in the middle makes textual normalization unsound anyway — reject
  is the only version of this that's actually safe.
- **The target doesn't have to exist** — `write` creates files — so
  `resolve` canonicalizes the *deepest existing ancestor* of the requested
  path and checks that, then re-attaches the non-existent tail. Canonicalizing
  only the immediate parent would miss a symlinked grandparent.
- **`display()` is the other half of the contract**: paths shown back to the
  model are relative to the sandbox root, so the transcript never leaks the
  host machine's real directory layout.
- The test suite (`sandbox.rs`'s own `#[cfg(test)]` module) is short and
  worth reading end to end — four tests, each one a specific attack shape:
  relative-inside-root (accept), `../../etc/passwd` (reject), an absolute
  path outside the root (reject), an empty path (reject).

Next: [testing the loop](06-testing-the-loop.md) — how the loop's own
invariants (not the sandbox's) get proven without ever calling a real API.
