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
