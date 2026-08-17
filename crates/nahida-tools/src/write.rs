use async_trait::async_trait;
use nahida_agent::{Tool, ToolOutcome, required_str};

use crate::sandbox::Sandbox;

/// Whole-file write.
///
/// Deliberately has no staleness check — that invariant belongs to [`crate::Edit`]
/// now, which refuses a replacement when the text it expected is no longer
/// there instead of silently overwriting whatever changed.
pub struct Write {
    sandbox: Sandbox,
}

impl Write {
    pub fn new(sandbox: Sandbox) -> Self {
        Self { sandbox }
    }
}

#[async_trait]
impl Tool for Write {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        include_str!("write.md")
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to the workspace root."
                },
                "content": {
                    "type": "string",
                    "description": "The complete new contents of the file."
                }
            },
            "required": ["path", "content"],
            "additionalProperties": false
        })
    }

    async fn call(&self, input: serde_json::Value) -> ToolOutcome {
        let path = match required_str(&input, "path") {
            Ok(p) => p,
            Err(e) => return ToolOutcome::err(e),
        };
        // Not `required_str`: an empty file is legitimate, a missing key is not.
        let Some(content) = input.get("content").and_then(serde_json::Value::as_str) else {
            return ToolOutcome::err("missing required string parameter `content`");
        };

        let resolved = match self.sandbox.resolve(&path) {
            Ok(p) => p,
            Err(e) => return ToolOutcome::err(e),
        };

        if let Some(parent) = resolved.parent()
            && let Err(e) = tokio::fs::create_dir_all(parent).await
        {
            return ToolOutcome::err(format!(
                "cannot create directory `{}`: {e}",
                self.sandbox.display(parent)
            ));
        }

        let existed = resolved.exists();
        if let Err(e) = tokio::fs::write(&resolved, content).await {
            return ToolOutcome::err(format!("cannot write `{path}`: {e}"));
        }

        let verb = if existed { "overwrote" } else { "created" };
        ToolOutcome::ok(format!(
            "{verb} {} ({} bytes, {} lines)",
            self.sandbox.display(&resolved),
            content.len(),
            content.lines().count()
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

    async fn write(sb: &Sandbox, input: serde_json::Value) -> ToolOutcome {
        Write::new(sb.clone()).call(input).await
    }

    #[tokio::test]
    async fn creates_a_new_file() {
        let (dir, sb) = sandbox();

        let out = write(&sb, serde_json::json!({"path": "f.txt", "content": "hi\n"})).await;

        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.starts_with("created"), "got {}", out.content);
        assert_eq!(std::fs::read_to_string(dir.path().join("f.txt")).unwrap(), "hi\n");
    }

    #[tokio::test]
    async fn overwrites_an_existing_file() {
        let (dir, sb) = sandbox();
        std::fs::write(dir.path().join("f.txt"), "old\n").expect("seed");

        let out = write(&sb, serde_json::json!({"path": "f.txt", "content": "new\n"})).await;

        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.starts_with("overwrote"), "got {}", out.content);
        assert_eq!(std::fs::read_to_string(dir.path().join("f.txt")).unwrap(), "new\n");
    }

    #[tokio::test]
    async fn creates_missing_parent_directories() {
        let (dir, sb) = sandbox();

        let out = write(&sb, serde_json::json!({"path": "a/b/c.txt", "content": "deep\n"})).await;

        assert!(!out.is_error, "{}", out.content);
        assert_eq!(std::fs::read_to_string(dir.path().join("a/b/c.txt")).unwrap(), "deep\n");
    }

    #[tokio::test]
    async fn an_empty_file_is_legitimate() {
        let (dir, sb) = sandbox();

        let out = write(&sb, serde_json::json!({"path": "empty.txt", "content": ""})).await;

        assert!(!out.is_error, "{}", out.content);
        assert_eq!(std::fs::read_to_string(dir.path().join("empty.txt")).unwrap(), "");
    }

    #[tokio::test]
    async fn missing_content_key_is_an_error_distinct_from_empty_content() {
        let (_dir, sb) = sandbox();

        let out = write(&sb, serde_json::json!({"path": "f.txt"})).await;

        assert!(out.is_error);
        assert!(out.content.contains("content"), "got {}", out.content);
    }

    #[tokio::test]
    async fn a_path_outside_the_sandbox_is_refused() {
        let (_dir, sb) = sandbox();

        let out = write(&sb, serde_json::json!({"path": "../outside.txt", "content": "x"})).await;

        assert!(out.is_error);
    }
}
