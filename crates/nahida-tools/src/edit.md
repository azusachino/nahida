Replace exact text in an existing file — a targeted change, not a full
rewrite.

`old_string` must match the file's current content exactly, including
whitespace, and must be unique unless you pass `replace_all: true`. If it
isn't found, or matches more than once without `replace_all`, this refuses
rather than guessing — read the file again (it may have changed since you
last saw it) or add more surrounding context to make the match unique.

Prefer this over `write` for changing part of an existing file: `write`
replaces everything and cannot tell whether what it's overwriting is what
you think is there.

Paths are relative to the workspace root; `..` is refused. The file must
already exist — use `write` to create a new one.
