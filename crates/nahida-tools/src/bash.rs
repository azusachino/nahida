use std::process::Stdio;
use std::time::Duration;

use std::fmt::Write as _;

use async_trait::async_trait;
use nahida_agent::{Tool, ToolOutcome, optional_u64, required_str};

use crate::clamp_output;
use crate::sandbox::Sandbox;

const DEFAULT_TIMEOUT_MS: u64 = 120_000;
const MAX_TIMEOUT_MS: u64 = 600_000;
const MAX_OUTPUT_CHARS: usize = 30_000;

/// Shell access.
///
/// This runs whatever the model asks for, with this process's privileges. That
/// is the honest starting point — breadth first — but it is also the reason
/// permission gating is a real piece of work and not a nicety: the harness sees
/// only an opaque command string here, so it cannot tell a `grep` from a
/// `git push --force`. Promoting the dangerous actions to their own tools is
/// what makes them gateable.
pub struct Bash {
    sandbox: Sandbox,
}

impl Bash {
    pub fn new(sandbox: Sandbox) -> Self {
        Self { sandbox }
    }
}

#[async_trait]
impl Tool for Bash {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        include_str!("bash.md")
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to run, as you would type it."
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": format!(
                        "Kill the command after this many milliseconds \
                         (default {DEFAULT_TIMEOUT_MS}, max {MAX_TIMEOUT_MS})."
                    )
                }
            },
            "required": ["command"],
            "additionalProperties": false
        })
    }

    /// Always gated: the harness sees only an opaque command string, so it
    /// cannot tell a `grep` from a `git push --force` — see the module doc.
    /// All-or-nothing until dangerous actions get split into their own tools.
    fn requires_confirmation(&self, _input: &serde_json::Value) -> bool {
        true
    }

    async fn call(&self, input: serde_json::Value) -> ToolOutcome {
        let command = match required_str(&input, "command") {
            Ok(c) => c,
            Err(e) => return ToolOutcome::err(e),
        };
        let timeout_ms = match optional_u64(&input, "timeout_ms") {
            Ok(v) => v.unwrap_or(DEFAULT_TIMEOUT_MS).clamp(1, MAX_TIMEOUT_MS),
            Err(e) => return ToolOutcome::err(e),
        };

        // `kill_on_drop` is what makes the timeout below actually stop the
        // process: when the timeout drops the wait future, the child goes too.
        let child = tokio::process::Command::new("bash")
            .arg("-c")
            .arg(&command)
            .current_dir(self.sandbox.root())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn();

        let child = match child {
            Ok(c) => c,
            Err(e) => return ToolOutcome::err(format!("cannot start bash: {e}")),
        };

        let waited =
            tokio::time::timeout(Duration::from_millis(timeout_ms), child.wait_with_output()).await;

        let output = match waited {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => return ToolOutcome::err(format!("bash failed: {e}")),
            Err(_) => {
                return ToolOutcome::err(format!(
                    "command killed after {timeout_ms}ms without finishing:\n{command}"
                ));
            }
        };

        let mut body = String::new();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stdout.is_empty() {
            body.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !body.is_empty() && !body.ends_with('\n') {
                body.push('\n');
            }
            body.push_str(&stderr);
        }
        if body.trim().is_empty() {
            body.push_str("(no output)\n");
        }

        let mut body = clamp_output(&body, MAX_OUTPUT_CHARS);
        let code = output.status.code();

        match code {
            Some(0) => ToolOutcome::ok(body),
            Some(c) => {
                let _ = write!(body, "\n[exit status {c}]");
                // A non-zero exit is a result, not a tool malfunction — flagged
                // so the model notices, but the loop carries on.
                ToolOutcome::err(body)
            }
            None => {
                body.push_str("\n[killed by signal]");
                ToolOutcome::err(body)
            }
        }
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

    async fn bash(sb: &Sandbox, input: serde_json::Value) -> ToolOutcome {
        Bash::new(sb.clone()).call(input).await
    }

    #[tokio::test]
    async fn captures_stdout_on_success() {
        let (_dir, sb) = sandbox();

        let out = bash(&sb, serde_json::json!({"command": "echo hi"})).await;

        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("hi"), "got {}", out.content);
    }

    #[tokio::test]
    async fn a_non_zero_exit_is_an_error_result_not_a_tool_malfunction() {
        let (_dir, sb) = sandbox();

        let out = bash(&sb, serde_json::json!({"command": "exit 7"})).await;

        assert!(out.is_error);
        assert!(out.content.contains("[exit status 7]"), "got {}", out.content);
    }

    #[tokio::test]
    async fn stderr_is_captured_alongside_stdout() {
        let (_dir, sb) = sandbox();

        let out = bash(&sb, serde_json::json!({"command": "echo err 1>&2"})).await;

        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("err"), "got {}", out.content);
    }

    #[tokio::test]
    async fn no_output_is_reported_explicitly() {
        let (_dir, sb) = sandbox();

        let out = bash(&sb, serde_json::json!({"command": "true"})).await;

        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("(no output)"), "got {}", out.content);
    }

    #[tokio::test]
    async fn runs_with_the_sandbox_root_as_cwd() {
        let (dir, sb) = sandbox();
        std::fs::write(dir.path().join("marker.txt"), "").expect("seed");

        let out = bash(&sb, serde_json::json!({"command": "ls marker.txt"})).await;

        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("marker.txt"), "got {}", out.content);
    }

    #[tokio::test]
    async fn missing_command_is_an_error() {
        let (_dir, sb) = sandbox();

        let out = bash(&sb, serde_json::json!({})).await;

        assert!(out.is_error);
        assert!(out.content.contains("command"), "got {}", out.content);
    }

    #[tokio::test]
    async fn a_command_over_its_timeout_is_killed() {
        let (_dir, sb) = sandbox();

        let out = bash(&sb, serde_json::json!({"command": "sleep 5", "timeout_ms": 50})).await;

        assert!(out.is_error);
        assert!(out.content.contains("killed after 50ms"), "got {}", out.content);
    }

    #[test]
    fn always_requires_confirmation_regardless_of_input() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sb = Sandbox::new(dir.path()).expect("sandbox");
        let bash = Bash::new(sb);

        assert!(bash.requires_confirmation(&serde_json::json!({"command": "ls"})));
        assert!(bash.requires_confirmation(&serde_json::json!({})));
    }
}
