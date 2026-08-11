Find files by name using a glob pattern, recursively.

Call this to locate files by name or path shape (`**/*.rs`, `src/**/*test*`)
when you don't already know the exact path — use `read` once you do. Respects
`.gitignore` and skips hidden files, the same way `git status` would, so
results don't need filtering by hand. Paths are relative to the workspace
root; `..` is refused. Large result sets are truncated and the response says
so — narrow `pattern` or pass `path` to search a subdirectory instead of
assuming you saw everything.
