use std::fmt::Write as _;
use std::path::PathBuf;

use async_trait::async_trait;
use nahida_agent::{Tool, ToolOutcome, optional_usize, required_str};
use regex::RegexBuilder;

use crate::clamp_output;
use crate::sandbox::Sandbox;

const DEFAULT_LIMIT: usize = 200;
const MAX_OUTPUT_CHARS: usize = 30_000;

/// Recursive, gitignore-aware content search.
///
/// Same walk as [`crate::Find`], but matches line content instead of paths.
/// `literal: true` searches for `pattern` as plain text rather than a regex —
/// useful when the string itself contains regex metacharacters the model
/// would otherwise have to escape.
pub struct Grep {
    sandbox: Sandbox,
}

impl Grep {
    pub fn new(sandbox: Sandbox) -> Self {
        Self { sandbox }
    }
}

#[async_trait]
impl Tool for Grep {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        include_str!("grep.md")
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to search for (or literal text if `literal` is true)."
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file to search, relative to the workspace root (default: root itself)."
                },
                "glob": {
                    "type": "string",
                    "description": "Restrict the search to files matching this glob, e.g. `*.rs`."
                },
                "ignore_case": {
                    "type": "boolean",
                    "description": "Case-insensitive match (default false)."
                },
                "literal": {
                    "type": "boolean",
                    "description": "Treat `pattern` as literal text instead of a regex (default false)."
                },
                "context": {
                    "type": "integer",
                    "description": "Lines of context to show before and after each match (default 0)."
                },
                "limit": {
                    "type": "integer",
                    "description": format!("Maximum matches to return (default {DEFAULT_LIMIT}).")
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
        let glob_filter = input.get("glob").and_then(serde_json::Value::as_str).map(str::to_string);
        let ignore_case =
            input.get("ignore_case").and_then(serde_json::Value::as_bool).unwrap_or(false);
        let literal = input.get("literal").and_then(serde_json::Value::as_bool).unwrap_or(false);
        let context = match optional_usize(&input, "context") {
            Ok(v) => v.unwrap_or(0),
            Err(e) => return ToolOutcome::err(e),
        };
        let limit = match optional_usize(&input, "limit") {
            Ok(v) => v.unwrap_or(DEFAULT_LIMIT).max(1),
            Err(e) => return ToolOutcome::err(e),
        };

        let glob_matcher = match glob_filter.as_deref().map(globset::Glob::new) {
            Some(Ok(g)) => Some(g.compile_matcher()),
            Some(Err(e)) => return ToolOutcome::err(format!("invalid glob pattern: {e}")),
            None => None,
        };

        let needle = if literal { regex::escape(&pattern) } else { pattern.clone() };
        let regex = match RegexBuilder::new(&needle).case_insensitive(ignore_case).build() {
            Ok(r) => r,
            Err(e) => return ToolOutcome::err(format!("invalid pattern `{pattern}`: {e}")),
        };

        let resolved = match self.sandbox.resolve(&path) {
            Ok(p) => p,
            Err(e) => return ToolOutcome::err(e),
        };

        let sandbox = self.sandbox.clone();
        let root = resolved.clone();
        // Blocking (directory walk, file reads) — off the async executor.
        let matches: Vec<String> = tokio::task::spawn_blocking(move || {
            let mut matches = Vec::new();
            let files: Vec<PathBuf> = if root.is_file() {
                vec![root.clone()]
            } else {
                ignore::WalkBuilder::new(&root)
                    .build()
                    .filter_map(Result::ok)
                    .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
                    .map(|e| e.path().to_path_buf())
                    .collect()
            };

            'files: for file in files {
                if let Some(g) = &glob_matcher {
                    let rel = file.strip_prefix(&root).unwrap_or(&file);
                    if !g.is_match(rel) {
                        continue;
                    }
                }
                let Ok(text) = std::fs::read_to_string(&file) else { continue };
                let lines: Vec<&str> = text.lines().collect();
                for (i, line) in lines.iter().enumerate() {
                    if !regex.is_match(line) {
                        continue;
                    }
                    let start = i.saturating_sub(context);
                    let end = (i + context + 1).min(lines.len());
                    let mut block = String::new();
                    for (n, l) in lines[start..end].iter().enumerate() {
                        let marker = if start + n == i { ':' } else { '-' };
                        let _ = writeln!(
                            block,
                            "{}{marker}{}{marker} {l}",
                            sandbox.display(&file),
                            start + n + 1
                        );
                    }
                    matches.push(block);
                    if matches.len() >= limit {
                        break 'files;
                    }
                }
            }
            matches
        })
        .await
        .unwrap_or_default();

        if matches.is_empty() {
            return ToolOutcome::ok(format!("no matches for `{pattern}` under `{path}`"));
        }

        ToolOutcome::ok(clamp_output(&matches.join(""), MAX_OUTPUT_CHARS))
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
    async fn finds_a_matching_line() {
        let (dir, sb) = sandbox();
        std::fs::write(dir.path().join("f.rs"), "fn main() {}\n").expect("seed");

        let out = Grep::new(sb).call(serde_json::json!({"pattern": "fn main"})).await;

        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("f.rs:1:"), "got {}", out.content);
    }

    #[tokio::test]
    async fn literal_mode_does_not_treat_pattern_as_regex() {
        let (dir, sb) = sandbox();
        std::fs::write(dir.path().join("f.txt"), "a.b\n").expect("seed");

        let out = Grep::new(sb).call(serde_json::json!({"pattern": "a.b", "literal": true})).await;

        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("a.b"), "got {}", out.content);
    }

    #[tokio::test]
    async fn ignore_case_matches_regardless_of_case() {
        let (dir, sb) = sandbox();
        std::fs::write(dir.path().join("f.txt"), "HELLO\n").expect("seed");

        let out =
            Grep::new(sb).call(serde_json::json!({"pattern": "hello", "ignore_case": true})).await;

        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("HELLO"), "got {}", out.content);
    }

    #[tokio::test]
    async fn reports_no_matches() {
        let (dir, sb) = sandbox();
        std::fs::write(dir.path().join("f.txt"), "hello\n").expect("seed");

        let out = Grep::new(sb).call(serde_json::json!({"pattern": "nope"})).await;

        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("no matches"), "got {}", out.content);
    }

    #[tokio::test]
    async fn glob_filters_which_files_are_searched() {
        let (dir, sb) = sandbox();
        std::fs::write(dir.path().join("a.rs"), "needle\n").expect("seed");
        std::fs::write(dir.path().join("a.md"), "needle\n").expect("seed");

        let out =
            Grep::new(sb).call(serde_json::json!({"pattern": "needle", "glob": "*.rs"})).await;

        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("a.rs"), "got {}", out.content);
        assert!(!out.content.contains("a.md"), "got {}", out.content);
    }
}
