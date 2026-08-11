use std::fmt::Write as _;

use async_trait::async_trait;
use globset::Glob;
use nahida_agent::{Tool, ToolOutcome, optional_usize, required_str};

use crate::sandbox::Sandbox;

const DEFAULT_LIMIT: usize = 1_000;

/// Recursive, gitignore-aware file search by glob pattern.
///
/// Walks with `ignore::WalkBuilder`, so `.gitignore`/`.ignore`/hidden files
/// are skipped the same way `git status` would skip them — a `find` that
/// turned up `target/` or `node_modules/` on every call would train the
/// model to filter results itself instead of trusting the tool.
pub struct Find {
    sandbox: Sandbox,
}

impl Find {
    pub fn new(sandbox: Sandbox) -> Self {
        Self { sandbox }
    }
}

#[async_trait]
impl Tool for Find {
    fn name(&self) -> &str {
        "find"
    }

    fn description(&self) -> &str {
        include_str!("find.md")
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match file paths, e.g. `**/*.rs` or `src/**/*.md`."
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search from, relative to the workspace root (default: root itself)."
                },
                "limit": {
                    "type": "integer",
                    "description": format!("Maximum results to return (default {DEFAULT_LIMIT}).")
                }
            },
            "required": ["pattern"],
            "additionalProperties": false
        })
    }

    async fn call(&self, input: serde_json::Value) -> ToolOutcome {
        let pattern = match required_str(&input, "pattern") {
            Ok(p) => p,
            Err(e) => return ToolOutcome::err(e),
        };
        let path = input.get("path").and_then(serde_json::Value::as_str).unwrap_or(".").to_string();
        let limit = match optional_usize(&input, "limit") {
            Ok(v) => v.unwrap_or(DEFAULT_LIMIT).max(1),
            Err(e) => return ToolOutcome::err(e),
        };

        let glob = match Glob::new(&pattern) {
            Ok(g) => g.compile_matcher(),
            Err(e) => return ToolOutcome::err(format!("invalid glob pattern `{pattern}`: {e}")),
        };

        let resolved = match self.sandbox.resolve(&path) {
            Ok(p) => p,
            Err(e) => return ToolOutcome::err(e),
        };
        if !resolved.is_dir() {
            return ToolOutcome::err(format!("`{path}` is not a directory"));
        }

        let sandbox = self.sandbox.clone();
        let root = resolved.clone();
        // `ignore::WalkBuilder` is a blocking, synchronous walk — off the
        // async executor so a large tree doesn't stall other work.
        let hits: Vec<String> = tokio::task::spawn_blocking(move || {
            let mut hits = Vec::new();
            for entry in ignore::WalkBuilder::new(&root).build().filter_map(Result::ok) {
                if !entry.file_type().is_some_and(|t| t.is_file()) {
                    continue;
                }
                let rel = entry.path().strip_prefix(&root).unwrap_or(entry.path());
                if glob.is_match(rel) {
                    hits.push(sandbox.display(entry.path()));
                }
            }
            hits
        })
        .await
        .unwrap_or_default();

        let total = hits.len();
        let shown = total.min(limit);
        let mut body: Vec<String> = hits;
        body.truncate(limit);
        let mut out = body.join("\n");
        if shown < total {
            let _ = write!(out, "\n\n[showed {shown} of {total} matches]");
        }
        if out.is_empty() {
            out = format!("no files matched `{pattern}` under `{path}`");
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
    async fn finds_files_matching_a_glob() {
        let (dir, sb) = sandbox();
        std::fs::create_dir(dir.path().join("src")).expect("seed");
        std::fs::write(dir.path().join("src/main.rs"), "").expect("seed");
        std::fs::write(dir.path().join("README.md"), "").expect("seed");

        let out = Find::new(sb).call(serde_json::json!({"pattern": "**/*.rs"})).await;

        assert!(!out.is_error, "{}", out.content);
        assert_eq!(out.content, "src/main.rs");
    }

    #[tokio::test]
    async fn respects_gitignore() {
        let (dir, sb) = sandbox();
        std::fs::write(dir.path().join(".gitignore"), "ignored.txt\n").expect("seed");
        std::fs::write(dir.path().join("ignored.txt"), "").expect("seed");
        std::fs::write(dir.path().join("kept.txt"), "").expect("seed");
        // A real git repo, since `ignore` only honours `.gitignore` inside one.
        std::fs::create_dir(dir.path().join(".git")).expect("seed");

        let out = Find::new(sb).call(serde_json::json!({"pattern": "*.txt"})).await;

        assert!(!out.is_error, "{}", out.content);
        assert_eq!(out.content, "kept.txt");
    }

    #[tokio::test]
    async fn reports_no_matches() {
        let (_dir, sb) = sandbox();

        let out = Find::new(sb).call(serde_json::json!({"pattern": "*.nope"})).await;

        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("no files matched"), "got {}", out.content);
    }

    #[tokio::test]
    async fn refuses_a_path_outside_the_root() {
        let (_dir, sb) = sandbox();

        let out = Find::new(sb).call(serde_json::json!({"pattern": "*", "path": "/etc"})).await;

        assert!(out.is_error);
    }
}
