Write a UTF-8 text file in the workspace, replacing it if it already exists.

This overwrites the whole file, so `read` it first unless you are creating it —
otherwise you will silently discard content you never saw. Missing parent
directories are created for you.

Paths are relative to the workspace root; `..` is refused.
