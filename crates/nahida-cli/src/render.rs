//! Turning [`AgentEvent`]s into terminal output.
//!
//! This is the only place that knows about ANSI codes. The loop emits events and
//! stays ignorant of whether anyone is watching, which is what lets the same loop
//! back a test or a server later.

use std::io::Write as _;

use nahida_agent::AgentEvent;
use nahida_llm::Usage;

const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const CYAN: &str = "\x1b[36m";
const RESET: &str = "\x1b[0m";

#[derive(Default)]
pub struct Renderer {
    /// Whether the cursor is mid-line, so we know if a newline is owed before
    /// printing a tool call on its own line.
    dirty: bool,
    thinking: bool,
    verbose: bool,
}

impl Renderer {
    pub fn new(verbose: bool) -> Self {
        Self { verbose, ..Self::default() }
    }

    fn newline_if_needed(&mut self) {
        if self.dirty {
            println!();
            self.dirty = false;
        }
    }

    pub fn handle(&mut self, event: &AgentEvent) {
        match event {
            AgentEvent::TurnStart { turn } => {
                if self.verbose && *turn > 1 {
                    self.newline_if_needed();
                    println!("{DIM}— turn {turn} —{RESET}");
                }
            }

            AgentEvent::Thinking { delta } => {
                if !self.thinking {
                    self.newline_if_needed();
                    print!("{DIM}thinking: ");
                    self.thinking = true;
                }
                print!("{delta}");
                self.dirty = true;
                let _ = std::io::stdout().flush();
            }

            AgentEvent::Text { delta } => {
                if self.thinking {
                    print!("{RESET}");
                    self.thinking = false;
                    self.newline_if_needed();
                }
                print!("{delta}");
                self.dirty = true;
                // Without this the whole turn appears at once and streaming
                // stops being streaming.
                let _ = std::io::stdout().flush();
            }

            AgentEvent::ToolCall { name, input } => {
                self.newline_if_needed();
                println!("{CYAN}▸ {name}{RESET} {DIM}{}{RESET}", summarize(input));
            }

            AgentEvent::ToolResult { name, is_error, content } => {
                if *is_error {
                    let first = content.lines().next().unwrap_or("").trim();
                    println!("{RED}  ✗ {name}{RESET} {DIM}{}{RESET}", truncate(first, 120));
                } else if self.verbose {
                    let lines = content.lines().count();
                    println!("{DIM}  ✓ {name} — {lines} lines{RESET}");
                }
            }

            AgentEvent::TurnEnd { usage } => {
                if self.verbose {
                    self.newline_if_needed();
                    println!("{DIM}{}{RESET}", format_usage(usage));
                }
            }

            AgentEvent::Compacting => {
                self.newline_if_needed();
                println!("{DIM}▸ compacting context…{RESET}");
            }

            AgentEvent::Compacted => {
                self.newline_if_needed();
                println!("{DIM}  ✓ context compacted{RESET}");
            }

            AgentEvent::Done { .. } => self.newline_if_needed(),
        }
    }

    /// Close out any partial line at the end of a run.
    pub fn finish(&mut self) {
        if self.thinking {
            print!("{RESET}");
            self.thinking = false;
        }
        self.newline_if_needed();
        let _ = std::io::stdout().flush();
    }
}

pub fn format_usage(usage: &Usage) -> String {
    format!(
        "{} in ({} cached read, {} cache write) / {} out",
        usage.input_tokens,
        usage.cache_read_input_tokens,
        usage.cache_creation_input_tokens,
        usage.output_tokens
    )
}

/// A one-line gist of a tool's input, for the call line.
pub(crate) fn summarize(input: &serde_json::Value) -> String {
    for key in ["path", "command"] {
        if let Some(v) = input.get(key).and_then(serde_json::Value::as_str) {
            return truncate(v.trim(), 100);
        }
    }
    truncate(&input.to_string(), 100)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{head}…")
}
