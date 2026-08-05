Read a UTF-8 text file from the workspace.

Call this before editing or writing any file that already exists — you cannot
reason about a change to content you have not seen.

Output is line-numbered, `cat -n` style, so you can refer to a specific line and
so `offset` lines up with what you were shown. Paths are relative to the
workspace root; `..` is refused. Large files are truncated and the response says
so — use `offset` and `limit` to page through the rest rather than assuming you
saw everything.
