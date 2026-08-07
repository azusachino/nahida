Write a UTF-8 text file in the workspace, replacing it if it already exists.

This overwrites the whole file, so `read` it first unless you are creating it —
otherwise you will silently discard content you never saw. Missing parent
directories are created for you.

Prefer `edit` for changing part of a file that already exists — it fails
loudly if the text you expected to be there is not, instead of silently
discarding whatever else changed. Reach for `write` when you are creating a
new file or genuinely replacing the whole thing.

Paths are relative to the workspace root; `..` is refused.
