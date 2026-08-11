Search file contents for a pattern, recursively.

Call this to find where something is defined, used, or mentioned across the
workspace, instead of opening files one at a time to check. Respects
`.gitignore` and skips hidden files. `pattern` is a regex by default; pass
`literal: true` to search for exact text without escaping regex
metacharacters. Use `glob` to restrict the search to matching files (e.g.
`*.rs`), and `context` to see surrounding lines. Paths are relative to the
workspace root; `..` is refused. Large result sets are truncated — narrow the
pattern, `glob`, or `path` rather than assuming you saw every match.
