Run a shell command in the workspace root and get back its combined output and
exit status.

This is the general-purpose escape hatch: use it for anything the dedicated tools
do not cover — listing files, searching, running the build, running tests, git.
Prefer `read` over `cat` and `write` over a heredoc, because those report
failures in a form you can act on.

Notes that affect how you should call it:

- The working directory is the workspace root and does not persist between calls.
  `cd foo && ls` works; two separate calls do not.
- stdout and stderr come back interleaved, followed by the exit status. A
  non-zero status is reported, not hidden — read it before deciding the command
  worked.
- Long-running commands are killed at the timeout. Prefer bounded commands over
  anything that waits for input, and never start something that blocks forever.
