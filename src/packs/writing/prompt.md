Use the writing tools to work on Markdown drafts in the current workspace.
Read a draft before editing it. For edit_draft, copy an exact, unique oldText
passage and supply its replacement as newText; include surrounding text if the
tool reports ambiguity. create_draft creates a new file and refuses to overwrite
an existing one. Paths must stay inside the workspace and end in .md or
.markdown. Symlinks and hard-linked files are unavailable. Treat a tool error
as a failed operation: inspect it and correct the request before trying again.
