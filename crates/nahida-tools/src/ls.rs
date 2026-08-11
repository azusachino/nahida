use std::fmt::Write as _;

use async_trait::async_trait;
use nahida_agent::{Tool, ToolOutcome, optional_usize};

use crate::sandbox::Sandbox;

const DEFAULT_LIMIT: usize = 500;

/// Non-recursive directory listing.
///
/// One level only, unfiltered — [`crate::Find`] is for a recursive,
/// gitignore-aware search. This is for "what's in this directory," which
/// does not need a glob or a walk.
pub struct Ls {
    sandbox: Sandbox,
}

impl Ls {
    pub fn new(sandbox: Sandbox) -> Self {
        Self { sandbox }
    }
}

#[async_trait]
impl Tool for Ls {
    fn name(&self) -> &str {
        "ls"
    }

    fn description(&self) -> &str {
        include_str!("ls.md")
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory to list, relative to the workspace root (default: root itself)."
                },
                "limit": {
                    "type": "integer",
                    "description": format!("Maximum entries to return (default {DEFAULT_LIMIT}).")
                }
            },
            "additionalProperties": false
        })
    }

    async fn call(&self, input: serde_json::Value) -> ToolOutcome {
        let path = input.get("path").and_then(serde_json::Value::as_str).unwrap_or(".").to_string();
        let limit = match optional_usize(&input, "limit") {
            Ok(v) => v.unwrap_or(DEFAULT_LIMIT).max(1),
            Err(e) => return ToolOutcome::err(e),
        };

        let resolved = match self.sandbox.resolve(&path) {
            Ok(p) => p,
            Err(e) => return ToolOutcome::err(e),
        };

        let mut read_dir = match tokio::fs::read_dir(&resolved).await {
            Ok(r) => r,
            Err(e) => return ToolOutcome::err(format!("cannot list `{path}`: {e}")),
        };

        let mut entries = Vec::new();
        loop {
            match read_dir.next_entry().await {
                Ok(Some(entry)) => {
                    let is_dir = entry.file_type().await.is_ok_and(|t| t.is_dir());
                    let mut name = entry.file_name().to_string_lossy().into_owned();
                    if is_dir {
                        name.push('/');
                    }
                    entries.push(name);
                }
                Ok(None) => break,
                Err(e) => return ToolOutcome::err(format!("cannot list `{path}`: {e}")),
            }
        }
        entries.sort();

        let total = entries.len();
        let shown = total.min(limit);
        entries.truncate(limit);

        let mut out = entries.join("\n");
        if shown < total {
            let _ = write!(out, "\n\n[showed {shown} of {total} entries]");
        }
        if out.is_empty() {
            out = "(empty directory)".to_string();
        }

        ToolOutcome::ok(out)
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

    #[tokio::test]
    async fn lists_files_and_marks_directories() {
        let (dir, sb) = sandbox();
        std::fs::write(dir.path().join("b.txt"), "").expect("seed");
        std::fs::create_dir(dir.path().join("a_dir")).expect("seed");

        let out = Ls::new(sb).call(serde_json::json!({})).await;

        assert!(!out.is_error, "{}", out.content);
        assert_eq!(out.content, "a_dir/\nb.txt");
    }

    #[tokio::test]
    async fn truncates_past_the_limit() {
        let (dir, sb) = sandbox();
        for n in 0..5 {
            std::fs::write(dir.path().join(format!("f{n}.txt")), "").expect("seed");
        }

        let out = Ls::new(sb).call(serde_json::json!({"limit": 2})).await;

        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("showed 2 of 5 entries"), "got {}", out.content);
    }

    #[tokio::test]
    async fn refuses_a_path_outside_the_root() {
        let (_dir, sb) = sandbox();

        let out = Ls::new(sb).call(serde_json::json!({"path": "/etc"})).await;

        assert!(out.is_error);
    }
}
