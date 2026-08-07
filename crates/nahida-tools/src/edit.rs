use async_trait::async_trait;
use nahida_agent::{Tool, ToolOutcome, required_str};

use crate::sandbox::Sandbox;

/// A targeted string replacement, not a whole-file overwrite.
///
/// The staleness guarantee [`crate::Write`]'s doc comment calls out is a side
/// effect of matching on content rather than a separate hash/mtime check: if
/// the file changed enough that `old_string` no longer matches — or now
/// matches more than once — the edit refuses rather than silently landing on
/// the wrong version. That is a stronger guarantee than snapshotting "the
/// file I read looked like X" would give, and costs nothing extra to
/// implement: it falls straight out of doing a content-addressed replacement
/// instead of a blind overwrite.
pub struct Edit {
    sandbox: Sandbox,
}

impl Edit {
    pub fn new(sandbox: Sandbox) -> Self {
        Self { sandbox }
    }
}

#[async_trait]
impl Tool for Edit {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        include_str!("edit.md")
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to the workspace root. Must already exist."
                },
                "old_string": {
                    "type": "string",
                    "description": "Exact text to replace. Must match the file's current content \
                                     exactly, including whitespace — read the file first."
                },
                "new_string": {
                    "type": "string",
                    "description": "Text to put in its place."
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace every occurrence instead of requiring exactly one \
                                     match (default false)."
                }
            },
            "required": ["path", "old_string", "new_string"],
            "additionalProperties": false
        })
    }

    async fn call(&self, input: serde_json::Value) -> ToolOutcome {
        let path = match required_str(&input, "path") {
            Ok(p) => p,
            Err(e) => return ToolOutcome::err(e),
        };
        let old_string = match required_str(&input, "old_string") {
            Ok(s) => s,
            Err(e) => return ToolOutcome::err(e),
        };
        // Not `required_str`: an empty new_string is legitimate (deleting
        // old_string outright), a missing key is not.
        let Some(new_string) = input.get("new_string").and_then(serde_json::Value::as_str) else {
            return ToolOutcome::err("missing required string parameter `new_string`");
        };
        let replace_all =
            input.get("replace_all").and_then(serde_json::Value::as_bool).unwrap_or(false);

        if old_string.is_empty() {
            return ToolOutcome::err(
                "`old_string` must not be empty; use `write` to create a new file",
            );
        }
        if old_string == new_string {
            return ToolOutcome::err(
                "`old_string` and `new_string` are identical; nothing to change",
            );
        }

        let resolved = match self.sandbox.resolve(&path) {
            Ok(p) => p,
            Err(e) => return ToolOutcome::err(e),
        };

        let content = match tokio::fs::read_to_string(&resolved).await {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return ToolOutcome::err(format!(
                    "`{path}` does not exist; use `write` to create it"
                ));
            }
            Err(e) => return ToolOutcome::err(format!("cannot read `{path}`: {e}")),
        };

        let occurrences = content.matches(old_string.as_str()).count();
        if occurrences == 0 {
            return ToolOutcome::err(format!(
                "`old_string` was not found in `{path}` — read the file again to see its \
                 current content"
            ));
        }
        if occurrences > 1 && !replace_all {
            return ToolOutcome::err(format!(
                "`old_string` appears {occurrences} times in `{path}`; include more \
                 surrounding context to make it unique, or pass replace_all: true"
            ));
        }

        let updated = if replace_all {
            content.replace(old_string.as_str(), new_string)
        } else {
            content.replacen(old_string.as_str(), new_string, 1)
        };

        if let Err(e) = tokio::fs::write(&resolved, &updated).await {
            return ToolOutcome::err(format!("cannot write `{path}`: {e}"));
        }

        let replaced = if replace_all { occurrences } else { 1 };
        ToolOutcome::ok(format!(
            "edited {} ({replaced} replacement{})",
            self.sandbox.display(&resolved),
            if replaced == 1 { "" } else { "s" }
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox() -> (tempfile::TempDir, Sandbox) {
        let dir = tempfile::tempdir().expect("tempdir");
        let sb = Sandbox::new(dir.path()).expect("sandbox");
        (dir, sb)
    }

    async fn edit(sb: &Sandbox, input: serde_json::Value) -> ToolOutcome {
        Edit::new(sb.clone()).call(input).await
    }

    #[tokio::test]
    async fn replaces_a_unique_match() {
        let (dir, sb) = sandbox();
        std::fs::write(dir.path().join("f.txt"), "hello world\n").expect("seed");

        let out = edit(
            &sb,
            serde_json::json!({"path": "f.txt", "old_string": "world", "new_string": "nahida"}),
        )
        .await;

        assert!(!out.is_error, "{}", out.content);
        assert_eq!(std::fs::read_to_string(dir.path().join("f.txt")).unwrap(), "hello nahida\n");
    }

    #[tokio::test]
    async fn refuses_when_old_string_is_not_found() {
        let (dir, sb) = sandbox();
        std::fs::write(dir.path().join("f.txt"), "hello world\n").expect("seed");

        let out = edit(
            &sb,
            serde_json::json!({"path": "f.txt", "old_string": "goodbye", "new_string": "hi"}),
        )
        .await;

        assert!(out.is_error);
        assert!(out.content.contains("was not found"), "got {}", out.content);
        // The file must be untouched on a refused edit.
        assert_eq!(std::fs::read_to_string(dir.path().join("f.txt")).unwrap(), "hello world\n");
    }

    #[tokio::test]
    async fn refuses_an_ambiguous_match_without_replace_all() {
        let (dir, sb) = sandbox();
        std::fs::write(dir.path().join("f.txt"), "a\na\n").expect("seed");

        let out =
            edit(&sb, serde_json::json!({"path": "f.txt", "old_string": "a", "new_string": "b"}))
                .await;

        assert!(out.is_error);
        assert!(out.content.contains("appears 2 times"), "got {}", out.content);
        assert_eq!(std::fs::read_to_string(dir.path().join("f.txt")).unwrap(), "a\na\n");
    }

    #[tokio::test]
    async fn replace_all_replaces_every_occurrence() {
        let (dir, sb) = sandbox();
        std::fs::write(dir.path().join("f.txt"), "a\na\n").expect("seed");

        let out = edit(
            &sb,
            serde_json::json!({
                "path": "f.txt", "old_string": "a", "new_string": "b", "replace_all": true
            }),
        )
        .await;

        assert!(!out.is_error, "{}", out.content);
        assert_eq!(std::fs::read_to_string(dir.path().join("f.txt")).unwrap(), "b\nb\n");
        assert!(out.content.contains("2 replacements"), "got {}", out.content);
    }

    #[tokio::test]
    async fn refuses_a_missing_file() {
        let (_dir, sb) = sandbox();

        let out = edit(
            &sb,
            serde_json::json!({"path": "nope.txt", "old_string": "a", "new_string": "b"}),
        )
        .await;

        assert!(out.is_error);
        assert!(out.content.contains("does not exist"), "got {}", out.content);
    }

    #[tokio::test]
    async fn refuses_identical_old_and_new() {
        let (dir, sb) = sandbox();
        std::fs::write(dir.path().join("f.txt"), "a\n").expect("seed");

        let out =
            edit(&sb, serde_json::json!({"path": "f.txt", "old_string": "a", "new_string": "a"}))
                .await;

        assert!(out.is_error);
        assert!(out.content.contains("identical"), "got {}", out.content);
    }
}
