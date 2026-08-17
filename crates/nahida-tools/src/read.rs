use std::fmt::Write as _;

use async_trait::async_trait;
use nahida_agent::{Tool, ToolOutcome, optional_usize, required_str};

use crate::sandbox::Sandbox;

/// Default page size. Big enough for most source files, small enough that one
/// careless `read` on a lockfile does not eat the context window.
const DEFAULT_LIMIT: usize = 2_000;

/// A single absurdly long line (minified JS, a base64 blob) would otherwise
/// blow past the line budget on its own.
const MAX_LINE_CHARS: usize = 2_000;

pub struct Read {
    sandbox: Sandbox,
}

impl Read {
    pub fn new(sandbox: Sandbox) -> Self {
        Self { sandbox }
    }
}

#[async_trait]
impl Tool for Read {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        include_str!("read.md")
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to the workspace root."
                },
                "offset": {
                    "type": "integer",
                    "description": "1-based line to start from. Omit to start at the top."
                },
                "limit": {
                    "type": "integer",
                    "description": format!("Maximum lines to return (default {DEFAULT_LIMIT}).")
                }
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    async fn call(&self, input: serde_json::Value) -> ToolOutcome {
        let path = match required_str(&input, "path") {
            Ok(p) => p,
            Err(e) => return ToolOutcome::err(e),
        };
        let offset = match optional_usize(&input, "offset") {
            Ok(v) => v.unwrap_or(1).max(1),
            Err(e) => return ToolOutcome::err(e),
        };
        let limit = match optional_usize(&input, "limit") {
            Ok(v) => v.unwrap_or(DEFAULT_LIMIT).max(1),
            Err(e) => return ToolOutcome::err(e),
        };

        let resolved = match self.sandbox.resolve(&path) {
            Ok(p) => p,
            Err(e) => return ToolOutcome::err(e),
        };

        let bytes = match tokio::fs::read(&resolved).await {
            Ok(b) => b,
            Err(e) => return ToolOutcome::err(format!("cannot read `{path}`: {e}")),
        };

        // Say what happened rather than returning mojibake — the model can then
        // reach for `bash` if it really wants the bytes.
        let Ok(text) = String::from_utf8(bytes) else {
            return ToolOutcome::err(format!(
                "`{path}` is not valid UTF-8; this tool only reads text files"
            ));
        };

        let all: Vec<&str> = text.lines().collect();
        let total = all.len();

        if offset > total {
            return ToolOutcome::err(format!(
                "`{path}` has {total} lines; offset {offset} is past the end"
            ));
        }

        let end = (offset - 1 + limit).min(total);
        let mut out = String::new();
        for (i, line) in all[offset - 1..end].iter().enumerate() {
            let n = offset + i;
            if line.chars().count() > MAX_LINE_CHARS {
                let head: String = line.chars().take(MAX_LINE_CHARS).collect();
                let _ = writeln!(out, "{n:>6}\t{head}… [line truncated]");
            } else {
                let _ = writeln!(out, "{n:>6}\t{line}");
            }
        }

        if end < total {
            let _ = writeln!(
                out,
                "\n[showed lines {offset}-{end} of {total}; call again with offset {} for more]",
                end + 1
            );
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

    async fn read(sb: &Sandbox, input: serde_json::Value) -> ToolOutcome {
        Read::new(sb.clone()).call(input).await
    }

    #[tokio::test]
    async fn reads_a_whole_small_file_with_line_numbers() {
        let (dir, sb) = sandbox();
        std::fs::write(dir.path().join("f.txt"), "one\ntwo\nthree\n").expect("seed");

        let out = read(&sb, serde_json::json!({"path": "f.txt"})).await;

        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("1\tone"), "got {}", out.content);
        assert!(out.content.contains("3\tthree"), "got {}", out.content);
    }

    #[tokio::test]
    async fn paginates_with_offset_and_limit() {
        let (dir, sb) = sandbox();
        let mut body = String::new();
        for n in 1..=10 {
            writeln!(body, "line{n}").expect("write");
        }
        std::fs::write(dir.path().join("f.txt"), body).expect("seed");

        let out = read(&sb, serde_json::json!({"path": "f.txt", "offset": 3, "limit": 2})).await;

        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("3\tline3"), "got {}", out.content);
        assert!(out.content.contains("4\tline4"), "got {}", out.content);
        assert!(!out.content.contains("5\tline5"), "got {}", out.content);
        assert!(out.content.contains("call again with offset 5"), "got {}", out.content);
    }

    #[tokio::test]
    async fn offset_past_the_end_is_an_error() {
        let (dir, sb) = sandbox();
        std::fs::write(dir.path().join("f.txt"), "one\ntwo\n").expect("seed");

        let out = read(&sb, serde_json::json!({"path": "f.txt", "offset": 50})).await;

        assert!(out.is_error);
        assert!(out.content.contains("past the end"), "got {}", out.content);
    }

    #[tokio::test]
    async fn missing_path_is_an_error() {
        let (_dir, sb) = sandbox();

        let out = read(&sb, serde_json::json!({})).await;

        assert!(out.is_error);
        assert!(out.content.contains("path"), "got {}", out.content);
    }

    #[tokio::test]
    async fn a_missing_file_is_an_error_not_a_panic() {
        let (_dir, sb) = sandbox();

        let out = read(&sb, serde_json::json!({"path": "nope.txt"})).await;

        assert!(out.is_error);
        assert!(out.content.contains("cannot read"), "got {}", out.content);
    }

    #[tokio::test]
    async fn non_utf8_content_is_refused_not_returned_as_mojibake() {
        let (dir, sb) = sandbox();
        std::fs::write(dir.path().join("bin.dat"), [0xFF, 0xFE, 0x00, 0xFF]).expect("seed");

        let out = read(&sb, serde_json::json!({"path": "bin.dat"})).await;

        assert!(out.is_error);
        assert!(out.content.contains("not valid UTF-8"), "got {}", out.content);
    }
}
