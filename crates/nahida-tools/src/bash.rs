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
