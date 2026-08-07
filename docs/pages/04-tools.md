!!! note "Outline — not yet expanded to full prose"
    This chapter is scaffolded with the real shape and the real traps. Ask
    for it to be written in full when you're ready to go deeper than the
    outline.

# 4. Tools

**Where:** `crates/nahida-agent/src/tool.rs` (the trait), `crates/nahida-tools/`
(the implementations)

The loop knows about tools only through one trait — it has no idea a `read`
tool touches a filesystem or a `bash` tool spawns a process.

## What this chapter will cover

- **The `Tool` trait's four methods**: `name`, `description`, `input_schema`,
  `call`. The interesting one is `description` — the file's own comment
  says it's "shown to the model verbatim," and that it should be
  "prescriptive about *when* to call, not just what the tool does — that is
  what drives the decision to use it." A tool description is a prompt, not
  documentation, and it's worth reading `crates/nahida-tools/src/write.md`
  as an example of that distinction.
- **`ToolOutcome` is never a Rust `Err`.** A failed tool call — file not
  found, a non-zero exit code — is a normal `ToolOutcome` with `is_error:
  true`, handed back to the model so it can adapt. Propagating it as a real
  `Err` would end the turn, which is exactly backwards: "file not found" is
  information the model should act on, not a crash.
- **`spec()` is where a `Tool` becomes a `ToolSpec`** — the wire type from
  chapter 1 — turning the trait into exactly what gets sent in
  `Request.tools`.
- **The starting set is three tools on purpose**: `read`, `write`, `bash`,
  because together they close the loop — look at the workspace, change it,
  check the work. `nahida-tools/src/lib.rs`'s own comment calls out what's
  *not* here yet (grep, glob, a staleness-checked edit, a gated `git push`)
  as additions to a working thing, not prerequisites — see
  [what's next](11-whats-next.md).

Next: [sandboxing](05-sandboxing.md) — every one of these tools receives a
path from the model, and a path from the model is untrusted input.
