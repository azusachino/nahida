List a directory's immediate contents.

Call this to see what's in a directory one level deep — for a recursive
search by name use `find` instead. Directories are suffixed with `/`. Paths
are relative to the workspace root; `..` is refused. Large directories are
truncated and the response says so.
